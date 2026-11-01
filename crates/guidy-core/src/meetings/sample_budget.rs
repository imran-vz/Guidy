//! Bounded memory for a recording's audio queues (scorecard R20) that keeps
//! its tracks aligned. A queue of audio that isn't draining stops taking
//! samples past [`QUEUE_CAP_SAMPLES`] and counts what it turned away. Once it
//! has drained to [`RESUME_BELOW_SAMPLES`], the count comes back as silence of
//! that length, ahead of the next samples: a gap in the audio, but none in the
//! timeline.
//!
//! All atomics, as the audio threads must not wait. One thread offers samples
//! per track; another marks them done.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

/// Two minutes per track, at the 16 kHz of every capture buffer.
pub const QUEUE_CAP_SAMPLES: i64 = 120 * 16_000;
/// Half of it: resuming at the cap would chop the audio into alternating
/// buffers of sound and silence.
pub const RESUME_BELOW_SAMPLES: i64 = QUEUE_CAP_SAMPLES / 2;

/// What to do with samples just offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Queue them, after this many samples of silence for what was dropped.
    Queue { silence_first: i64 },
    /// Dropped and counted. `started`: the first since the queue was fine.
    Drop { started: bool },
}

/// One track's queue. See the module docs.
#[derive(Debug, Default)]
pub struct SampleBudget {
    queued: AtomicI64,
    dropped: AtomicI64,
    dropping: AtomicBool,
}

impl SampleBudget {
    pub fn offer(&self, samples: usize) -> Admission {
        self.offer_with_cap(samples, QUEUE_CAP_SAMPLES, RESUME_BELOW_SAMPLES)
    }

    fn offer_with_cap(&self, samples: usize, cap: i64, resume_below: i64) -> Admission {
        let samples = samples as i64;
        let queued = self.queued.load(Ordering::Relaxed);
        if self.dropping.load(Ordering::Relaxed) {
            if queued > resume_below || queued + samples > cap {
                self.dropped.fetch_add(samples, Ordering::Relaxed);
                return Admission::Drop { started: false };
            }
            self.dropping.store(false, Ordering::Relaxed);
            self.queued.fetch_add(samples, Ordering::Relaxed);
            return Admission::Queue {
                silence_first: self.dropped.swap(0, Ordering::Relaxed),
            };
        }
        if queued + samples > cap {
            self.dropping.store(true, Ordering::Relaxed);
            self.dropped.fetch_add(samples, Ordering::Relaxed);
            return Admission::Drop { started: true };
        }
        self.queued.fetch_add(samples, Ordering::Relaxed);
        Admission::Queue { silence_first: 0 }
    }

    /// `samples` left the queue.
    pub fn done(&self, samples: usize) {
        self.queued.fetch_sub(samples as i64, Ordering::Relaxed);
    }

    pub fn queued(&self) -> i64 {
        self.queued.load(Ordering::Relaxed)
    }

    /// Samples are being turned away now.
    pub fn is_dropping(&self) -> bool {
        self.dropping.load(Ordering::Relaxed)
    }

    /// The stream ended: what was dropped and not yet made up for, as silence
    /// to write after everything queued.
    pub fn finish(&self) -> i64 {
        self.dropping.store(false, Ordering::Relaxed);
        self.dropped.swap(0, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_at_the_cap_counts_and_resumes_with_the_gap() {
        let b = SampleBudget::default();
        assert_eq!(
            b.offer_with_cap(60, 100, 50),
            Admission::Queue { silence_first: 0 }
        );
        assert_eq!(
            b.offer_with_cap(40, 100, 50),
            Admission::Queue { silence_first: 0 }
        );
        assert!(!b.is_dropping());
        assert_eq!(
            b.offer_with_cap(1, 100, 50),
            Admission::Drop { started: true }
        );
        assert!(b.is_dropping());
        assert_eq!(
            b.offer_with_cap(9, 100, 50),
            Admission::Drop { started: false }
        );
        assert_eq!(b.queued(), 100);
        // Draining a little isn't enough.
        b.done(40);
        assert_eq!(
            b.offer_with_cap(5, 100, 50),
            Admission::Drop { started: false }
        );
        b.done(10);
        assert_eq!(
            b.offer_with_cap(7, 100, 50),
            Admission::Queue { silence_first: 15 }
        );
        assert!(!b.is_dropping());
        assert_eq!(b.queued(), 57);
        // Once made up for, it isn't owed again.
        assert_eq!(
            b.offer_with_cap(1, 100, 50),
            Admission::Queue { silence_first: 0 }
        );
    }

    #[test]
    fn recovery_still_respects_the_cap() {
        let b = SampleBudget::default();
        b.offer_with_cap(121, 100, 50);
        assert_eq!(
            b.offer_with_cap(121, 100, 50),
            Admission::Drop { started: false }
        );
        assert!(b.queued() <= 100);
    }

    #[test]
    fn finish_hands_back_what_is_still_owed() {
        let b = SampleBudget::default();
        assert_eq!(b.finish(), 0);
        b.offer_with_cap(10, 5, 2);
        assert_eq!(b.finish(), 10);
        assert_eq!(b.finish(), 0);
        assert!(!b.is_dropping());
    }

    #[test]
    fn the_cap_is_two_minutes_of_16_khz_audio() {
        let b = SampleBudget::default();
        let second = 16_000;
        for _ in 0..120 {
            assert_eq!(b.offer(second), Admission::Queue { silence_first: 0 });
        }
        assert_eq!(b.offer(second), Admission::Drop { started: true });
        assert_eq!(b.queued(), QUEUE_CAP_SAMPLES);
    }
}
