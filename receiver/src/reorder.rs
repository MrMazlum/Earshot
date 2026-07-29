//! Reorder window — the front half of the jitter buffer.
//!
//! Packets arrive out of order, twice, or not at all. This releases them **in sequence order** and
//! reports gaps so the caller can conceal them. The delay itself lives in the ring
//! (`ring.rs`); this only decides *what* comes out and *when it is too late to matter*.
//!
//! Rules it implements, from `~/EarshotBrain/Rules/udp-not-tcp.md`:
//!   - a packet that arrives after its slot has already played is **discarded, never played late**
//!   - gaps are declared, never waited for indefinitely
//!
//! Full design notes: `~/EarshotBrain/Concepts/jitter-buffer.md`.

use crate::proto::seq_diff;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub enum Release {
    /// A frame is ready, in order.
    Frame(Vec<u8>),
    /// Sequence `seq` is missing and we waited long enough. Conceal it.
    Lost(u32),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ReorderStats {
    pub accepted: u64,
    pub duplicates: u64,
    /// Arrived after its slot had already been released — dropped, not played late.
    pub too_late: u64,
    pub lost: u64,
    pub resets: u64,
}

/// Consecutive too-late packets that mean "this is a new stream, not a straggler". 25 frames is
/// half a second — long enough that ordinary reordering never reaches it.
const RESYNC_AFTER: u32 = 25;

pub struct Reorder {
    slots: BTreeMap<u32, Vec<u8>>,
    /// Sequence we will release next. `None` until the first packet arrives.
    playhead: Option<u32>,
    /// How many frames we are willing to hold while waiting for a gap to fill.
    depth: usize,
    ssrc: Option<u32>,
    /// How many packets in a row have arrived behind the playhead.
    behind_streak: u32,
    pub stats: ReorderStats,
}

impl Reorder {
    pub fn new(depth: usize) -> Self {
        Self {
            slots: BTreeMap::new(),
            playhead: None,
            depth: depth.max(1),
            ssrc: None,
            behind_streak: 0,
            stats: ReorderStats::default(),
        }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn held(&self) -> usize {
        self.slots.len()
    }

    /// Feeds one arrived packet in. Ignores packets from a different session.
    pub fn push(&mut self, ssrc: u32, sequence: u32, payload: &[u8]) {
        match self.ssrc {
            None => {
                self.ssrc = Some(ssrc);
                self.playhead = Some(sequence);
            }
            Some(known) if known != ssrc => {
                // A new session (the phone restarted). Start over rather than mixing two streams.
                self.reset_to(ssrc, sequence);
            }
            _ => {}
        }

        let playhead = self.playhead.unwrap_or(sequence);
        if seq_diff(sequence, playhead) < 0 {
            self.stats.too_late += 1;
            self.behind_streak += 1;
            // A single old packet is a straggler. A steady run of them means the sender restarted
            // its sequence — most likely the phone app was stopped and started — and waiting for
            // a sequence that will never come again would mean permanent silence.
            if self.behind_streak >= RESYNC_AFTER {
                self.reset_to(ssrc, sequence);
                self.slots.insert(sequence, payload.to_vec());
                self.stats.accepted += 1;
            }
            return;
        }
        self.behind_streak = 0;
        if self.slots.contains_key(&sequence) {
            self.stats.duplicates += 1;
            return;
        }
        self.slots.insert(sequence, payload.to_vec());
        self.stats.accepted += 1;
    }

    fn reset_to(&mut self, ssrc: u32, sequence: u32) {
        self.slots.clear();
        self.ssrc = Some(ssrc);
        self.playhead = Some(sequence);
        self.behind_streak = 0;
        self.stats.resets += 1;
    }

    /// Releases the next frame if it is ready, or declares it lost once we are holding more than
    /// `depth` frames behind the gap. Returns `None` when there is simply nothing to do yet.
    pub fn pop(&mut self) -> Option<Release> {
        let playhead = self.playhead?;

        if let Some(payload) = self.slots.remove(&playhead) {
            self.playhead = Some(playhead.wrapping_add(1));
            return Some(Release::Frame(payload));
        }

        // The frame is missing. Wait only while the backlog is small; past that, the gap is real.
        if self.slots.len() > self.depth {
            self.playhead = Some(playhead.wrapping_add(1));
            self.stats.lost += 1;
            return Some(Release::Lost(playhead));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u32 = 42; // ssrc

    fn frame(n: u8) -> Vec<u8> {
        vec![n; 4]
    }

    #[test]
    fn releases_in_order() {
        let mut r = Reorder::new(3);
        r.push(S, 0, &frame(0));
        r.push(S, 1, &frame(1));
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));
        assert_eq!(r.pop(), Some(Release::Frame(frame(1))));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn fixes_out_of_order_arrival() {
        let mut r = Reorder::new(3);
        r.push(S, 0, &frame(0));
        r.push(S, 2, &frame(2));
        r.push(S, 1, &frame(1)); // late but still in the future
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));
        assert_eq!(r.pop(), Some(Release::Frame(frame(1))));
        assert_eq!(r.pop(), Some(Release::Frame(frame(2))));
    }

    #[test]
    fn declares_a_gap_once_the_backlog_grows() {
        let mut r = Reorder::new(2);
        r.push(S, 0, &frame(0));
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));

        // 1 never arrives; 2, 3, 4 do.
        r.push(S, 2, &frame(2));
        r.push(S, 3, &frame(3));
        assert_eq!(r.pop(), None, "still willing to wait");
        r.push(S, 4, &frame(4));
        assert_eq!(r.pop(), Some(Release::Lost(1)), "waited long enough");
        assert_eq!(r.pop(), Some(Release::Frame(frame(2))));
        assert_eq!(r.stats.lost, 1);
    }

    #[test]
    fn a_frame_that_arrives_after_its_slot_is_dropped_not_played_late() {
        let mut r = Reorder::new(1);
        r.push(S, 0, &frame(0));
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));
        r.push(S, 1, &frame(1));
        assert_eq!(r.pop(), Some(Release::Frame(frame(1))));

        r.push(S, 0, &frame(0)); // ancient
        assert_eq!(r.stats.too_late, 1);
        assert_eq!(r.pop(), None, "never replay an old frame");
    }

    #[test]
    fn ignores_duplicates() {
        let mut r = Reorder::new(3);
        r.push(S, 0, &frame(0));
        r.push(S, 0, &frame(0));
        assert_eq!(r.stats.duplicates, 1);
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn a_new_session_resets_rather_than_mixing() {
        let mut r = Reorder::new(3);
        r.push(S, 100, &frame(1));
        r.push(999, 0, &frame(9)); // phone restarted: new ssrc, sequence back to 0
        assert_eq!(r.stats.resets, 1);
        assert_eq!(r.pop(), Some(Release::Frame(frame(9))));
    }

    /// The app being stopped and started again must not leave the receiver deaf forever.
    #[test]
    fn resyncs_when_the_sender_restarts_its_sequence() {
        let mut r = Reorder::new(3);
        for seq in 0..100 {
            r.push(S, seq, &frame(1));
            while r.pop().is_some() {}
        }

        // Same session id, but the sequence starts over.
        let mut heard_again = false;
        for seq in 0..40 {
            r.push(S, seq, &frame(9));
            while let Some(Release::Frame(f)) = r.pop() {
                assert_eq!(f, frame(9));
                heard_again = true;
            }
        }
        assert!(heard_again, "audio never resumed after the restart");
        assert_eq!(r.stats.resets, 1);
    }

    #[test]
    fn survives_sequence_wraparound() {
        let mut r = Reorder::new(3);
        let near_max = u32::MAX - 1;
        r.push(S, near_max, &frame(1));
        r.push(S, u32::MAX, &frame(2));
        r.push(S, 0, &frame(3)); // wrapped
        assert_eq!(r.pop(), Some(Release::Frame(frame(1))));
        assert_eq!(r.pop(), Some(Release::Frame(frame(2))));
        assert_eq!(r.pop(), Some(Release::Frame(frame(3))));
        assert_eq!(r.stats.too_late, 0, "the wrap must not look like an old packet");
    }
}
