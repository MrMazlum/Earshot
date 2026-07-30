//! Reorder window — the front half of the jitter buffer.
//!
//! Packets arrive out of order, twice, or not at all. This releases them **in sequence order** and
//! reports gaps so the caller can conceal them. The delay itself lives in the ring
//! (`ring.rs`); this only decides *what* comes out and *when it is too late to matter*.
//!
//! Two rules, both from "audio is never retransmitted" (`protocol/README.md`):
//!   - a packet that arrives after its slot has already played is **discarded, never played late**
//!   - gaps are declared, never waited for indefinitely

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

/// How far ahead of the playhead a sequence number may be before it stops being "a gap" and starts
/// being "a different stream". 50 frames is one second.
///
/// This bound is load-bearing, and not only for tidiness. [`Reorder::pop`] fills a gap by declaring
/// every missing sequence lost, **one at a time**, and the caller turns each one into a frame of
/// concealment silence. Without a cap, a single datagram claiming a sequence far in the future —
/// which anyone on the LAN can send, and which a `u32` lets reach two billion — would make the
/// receive thread sit in that loop for hours. A malformed 16-byte header should not be able to do
/// that.
///
/// It is also the right behaviour for the honest case. A Wi-Fi dropout of several seconds leaves a
/// gap of hundreds of frames; concealing all of them would push seconds of silence into the ring
/// and put the audio permanently behind. Jumping to the new sequence resumes at once instead.
const MAX_AHEAD: i64 = 50;

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
        let ahead = seq_diff(sequence, playhead);

        // Too far ahead to be a gap in this stream. Jump to it rather than concealing every
        // sequence in between, one frame at a time, for as long as that takes.
        if ahead > MAX_AHEAD {
            self.reset_to(ssrc, sequence);
            self.slots.insert(sequence, payload.to_vec());
            self.stats.accepted += 1;
            return;
        }

        if ahead < 0 {
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

    /// The packets that used to hang the receive thread.
    ///
    /// `pop` declares one loss per missing sequence, and only starts doing so once the backlog is
    /// deeper than `depth`. So a handful of datagrams claiming sequences two billion ahead — which
    /// anyone on the LAN can send, and which need no reply and no reachable port of their own —
    /// used to put the caller in that loop for something over two billion iterations, each one
    /// pushing a frame of silence. No crash, no over-read, just a receiver that never speaks again.
    #[test]
    fn sequence_numbers_from_the_far_future_cannot_stall_the_loop() {
        let depth = 3;
        let mut r = Reorder::new(depth);
        r.push(S, 0, &frame(0));
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));

        // Enough of them to push the backlog past `depth`, which is what arms the loop.
        for i in 0..(depth as u32 + 2) {
            r.push(S, 2_000_000_000 + i, &frame(7));
        }

        // Bounded work, and the audio carries on from the new position.
        let mut releases = 0;
        while let Some(release) = r.pop() {
            releases += 1;
            assert!(releases < 100, "pop is still declaring losses after {releases}");
            if let Release::Frame(f) = release {
                assert_eq!(f, frame(7));
            }
        }
        assert_eq!(releases, depth + 2, "every frame should have come out");
        assert_eq!(r.stats.resets, 1, "the jump should resync, not conceal");
    }

    /// The same bound, arrived at honestly: a Wi-Fi dropout leaves a gap of hundreds of frames.
    /// Concealing every one of them would push seconds of silence into the ring and leave the call
    /// permanently behind, so the receiver skips to the live edge instead.
    #[test]
    fn a_long_dropout_resumes_at_the_live_edge_rather_than_concealing_all_of_it() {
        let mut r = Reorder::new(3);
        r.push(S, 0, &frame(0));
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));

        r.push(S, 500, &frame(5)); // ten seconds later
        let mut lost = 0;
        while let Some(release) = r.pop() {
            if matches!(release, Release::Lost(_)) {
                lost += 1;
            }
        }
        assert_eq!(lost, 0, "concealed {lost} frames instead of skipping");
        assert!(r.held() == 0);
    }

    /// The bound must not eat an ordinary gap. A handful of dropped packets on a busy Wi-Fi is
    /// exactly what concealment is for, and resyncing past it would be wrong.
    #[test]
    fn an_ordinary_gap_is_still_concealed_frame_by_frame() {
        let mut r = Reorder::new(2);
        r.push(S, 0, &frame(0));
        assert_eq!(r.pop(), Some(Release::Frame(frame(0))));

        // 1..=5 are lost; 6, 7, 8 arrive.
        for seq in 6..=8 {
            r.push(S, seq, &frame(6));
        }
        let mut lost = 0;
        while let Some(release) = r.pop() {
            if matches!(release, Release::Lost(_)) {
                lost += 1;
            }
        }
        assert_eq!(lost, 5, "the five missing frames should each be concealed");
        assert_eq!(r.stats.resets, 0, "a five-frame gap is not a new stream");
    }

    /// Whatever a sender does, the held set stays small enough to be irrelevant to memory.
    #[test]
    fn a_flood_of_scattered_sequences_cannot_grow_the_held_set() {
        let mut r = Reorder::new(16);
        r.push(S, 0, &frame(0));
        for seq in (0..u32::MAX).step_by(7_919_311).take(400) {
            r.push(S, seq, &frame(1));
            assert!(r.held() <= MAX_AHEAD as usize + 1, "holding {}", r.held());
        }
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
