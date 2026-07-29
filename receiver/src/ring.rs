//! A lock-free single-producer/single-consumer ring of f32 samples.
//!
//! This exists so the audio callback never takes a lock: `~/EarshotBrain/Rules/no-blocking-audio-thread.md`.
//! The network thread is the only producer, the audio callback is the only consumer.
//!
//! Its fill level *is* the buffering latency — 960 samples at 48 kHz = 20 ms. Watch it in the stats
//! line: a level that keeps climbing is clock drift, not jitter
//! (`~/EarshotBrain/Concepts/clock-drift-and-resampling.md`).

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRing {
    buf: UnsafeCell<Box<[f32]>>,
    mask: usize,
    /// Next index to write. Written by the producer only.
    head: AtomicUsize,
    /// Next index to read. Written by the consumer only.
    tail: AtomicUsize,
}

// Safe because exactly one thread pushes and exactly one thread pops, and the indices are atomic.
unsafe impl Send for SpscRing {}
unsafe impl Sync for SpscRing {}

impl SpscRing {
    /// `capacity` is rounded up to a power of two so the wrap is a mask, not a modulo.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(2);
        Self {
            buf: UnsafeCell::new(vec![0.0; cap].into_boxed_slice()),
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.mask
    }

    /// Samples currently readable.
    pub fn len(&self) -> usize {
        self.head.load(Ordering::Acquire) - self.tail.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Producer side. Returns how many samples were written; drops the rest if the ring is full.
    ///
    /// Dropping is deliberate: a full ring means the consumer is not draining, and blocking the
    /// network thread would only pile up more latency.
    pub fn push(&self, src: &[f32]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let free = self.capacity() - (head - tail);
        let n = src.len().min(free);
        let buf = unsafe { &mut *self.buf.get() };
        for (i, s) in src[..n].iter().enumerate() {
            buf[(head + i) & self.mask] = *s;
        }
        self.head.store(head + n, Ordering::Release);
        n
    }

    /// Consumer side. Fills as much of `dst` as it can; returns how many samples were read.
    pub fn pop(&self, dst: &mut [f32]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let avail = head - tail;
        let n = dst.len().min(avail);
        let buf = unsafe { &*self.buf.get() };
        for (i, d) in dst[..n].iter_mut().enumerate() {
            *d = buf[(tail + i) & self.mask];
        }
        self.tail.store(tail + n, Ordering::Release);
        n
    }

    /// Consumer side. Throws away everything currently buffered (used when re-priming).
    pub fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_round_trip() {
        let r = SpscRing::new(8);
        assert!(r.is_empty());
        assert_eq!(r.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(r.len(), 3);

        let mut out = [0.0; 3];
        assert_eq!(r.pop(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        assert!(r.is_empty());
    }

    #[test]
    fn drops_when_full_instead_of_blocking() {
        let r = SpscRing::new(4); // capacity 3 usable
        let written = r.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(written, r.capacity());
        assert_eq!(r.len(), r.capacity());
    }

    #[test]
    fn partial_read_leaves_the_rest() {
        let r = SpscRing::new(16);
        r.push(&[1.0, 2.0, 3.0, 4.0]);
        let mut out = [0.0; 2];
        assert_eq!(r.pop(&mut out), 2);
        assert_eq!(out, [1.0, 2.0]);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn wraps_around_many_times() {
        let r = SpscRing::new(8);
        let mut out = [0.0; 4];
        for round in 0..100 {
            let v = [round as f32; 4];
            assert_eq!(r.push(&v), 4);
            assert_eq!(r.pop(&mut out), 4);
            assert_eq!(out, v);
        }
    }

    #[test]
    fn clear_empties_it() {
        let r = SpscRing::new(16);
        r.push(&[1.0, 2.0, 3.0]);
        r.clear();
        assert!(r.is_empty());
    }
}
