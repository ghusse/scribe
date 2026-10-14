//! When to transcribe the start of a recording to show its style while the user still speaks.

/// The peek fires at the latest this long into the recording.
pub const PEEK_AFTER_MS: u64 = 2_500;
/// A pause fires the peek only after this much recording: before, the keyword is rarely said yet.
pub const PEEK_PAUSE_MIN_MS: u64 = 1_000;
/// Quiet this long after speech counts as a pause.
pub const PEEK_PAUSE_QUIET_MS: u64 = 300;
/// Audio sent to the transcriber: the start of the recording, up to this long.
pub const PEEK_CLIP_MS: u64 = 3_000;

/// Decides, from the microphone level, when the peek is due. Fires once per recording.
#[derive(Debug, Clone, PartialEq)]
pub struct PeekTimer {
    started_at_ms: u64,
    threshold_rms: f32,
    speech_seen: bool,
    quiet_since: Option<u64>,
    fired: bool,
}

impl PeekTimer {
    /// `silence_threshold_dbfs`: the level under which the microphone is quiet (the settings' silence threshold).
    pub fn new(started_at_ms: u64, silence_threshold_dbfs: f32) -> Self {
        Self {
            started_at_ms,
            threshold_rms: 10f32.powf(silence_threshold_dbfs / 20.0),
            speech_seen: false,
            quiet_since: None,
            fired: false,
        }
    }

    /// `rms`: the latest level, 0.0–1.0.
    pub fn on_level(&mut self, now_ms: u64, rms: f32) {
        if rms >= self.threshold_rms {
            self.speech_seen = true;
            self.quiet_since = None;
        } else if self.speech_seen && self.quiet_since.is_none() {
            self.quiet_since = Some(now_ms);
        }
    }

    /// True once: at `PEEK_AFTER_MS`, or at the first pause after `PEEK_PAUSE_MIN_MS`.
    /// A pause is speech seen, then quiet for `PEEK_PAUSE_QUIET_MS`.
    pub fn take_due(&mut self, now_ms: u64) -> bool {
        if self.fired {
            return false;
        }
        let elapsed = now_ms.saturating_sub(self.started_at_ms);
        let paused = self.quiet_since.is_some_and(|q| now_ms.saturating_sub(q) >= PEEK_PAUSE_QUIET_MS);
        self.fired = elapsed >= PEEK_AFTER_MS || (elapsed >= PEEK_PAUSE_MIN_MS && paused);
        self.fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEECH: f32 = 0.1;
    const QUIET: f32 = 0.0001;

    /// Feeds `level` every 30 ms from `from` to `to` (excluded), like the controller tick; returns the first time
    /// the peek was due.
    fn feed(t: &mut PeekTimer, from: u64, to: u64, level: f32) -> Option<u64> {
        let mut due = None;
        for now in (from..to).step_by(30) {
            t.on_level(now, level);
            if t.take_due(now) && due.is_none() {
                due = Some(now);
            }
        }
        due
    }

    fn timer() -> PeekTimer {
        PeekTimer::new(1_000, -40.0)
    }

    #[test]
    fn fires_at_the_deadline_while_the_user_keeps_speaking() {
        let mut t = timer();
        assert_eq!(feed(&mut t, 1_000, 3_490, SPEECH), None);
        assert!(t.take_due(3_500));
    }

    #[test]
    fn fires_at_a_pause_after_the_minimum() {
        let mut t = timer();
        assert_eq!(feed(&mut t, 1_000, 2_200, SPEECH), None);
        assert_eq!(feed(&mut t, 2_200, 3_000, QUIET), Some(2_500));
    }

    #[test]
    fn a_pause_before_the_minimum_waits_for_the_minimum() {
        let mut t = timer();
        assert_eq!(feed(&mut t, 1_000, 1_300, SPEECH), None);
        assert_eq!(feed(&mut t, 1_300, 1_990, QUIET), None, "quiet long enough, but under 1 s in");
        assert!(t.take_due(2_000), "the pause still runs at 1 s");
    }

    #[test]
    fn a_short_pause_does_not_fire() {
        let mut t = timer();
        assert_eq!(feed(&mut t, 1_000, 2_200, SPEECH), None);
        assert_eq!(feed(&mut t, 2_200, 2_450, QUIET), None);
        assert_eq!(feed(&mut t, 2_450, 3_000, SPEECH), None);
        assert!(!t.take_due(3_000));
        assert!(t.take_due(3_500));
    }

    #[test]
    fn silence_without_speech_is_not_a_pause() {
        let mut t = timer();
        assert_eq!(feed(&mut t, 1_000, 3_490, QUIET), None);
        assert!(t.take_due(3_500), "the deadline still fires");
    }

    #[test]
    fn fires_once() {
        let mut t = timer();
        assert!(t.take_due(5_000));
        assert!(!t.take_due(5_030));
        assert_eq!(feed(&mut t, 5_000, 8_000, QUIET), None);
    }
}
