use crate::gesture::{GestureCommand, Mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Recording { mode: Mode, started_at_ms: u64 },
    Processing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    BeginRecording,
    SetMode(Mode),
    FinishRecording,
    DiscardRecording,
}

/// Single source of truth for the dictation lifecycle. UI and overlay only observe it.
pub struct Session {
    state: SessionState,
    max_recording_ms: u64,
}

impl Session {
    pub fn new(max_recording_ms: u64) -> Self {
        Self { state: SessionState::Idle, max_recording_ms }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn set_max_recording_ms(&mut self, v: u64) {
        self.max_recording_ms = v;
    }

    pub fn on_gesture(&mut self, cmd: GestureCommand, now_ms: u64) -> Option<SessionAction> {
        match (self.state, cmd) {
            (SessionState::Idle, GestureCommand::Start) => {
                self.state = SessionState::Recording { mode: Mode::Hold, started_at_ms: now_ms };
                Some(SessionAction::BeginRecording)
            }
            (SessionState::Recording { started_at_ms, .. }, GestureCommand::Lock) => {
                self.state = SessionState::Recording { mode: Mode::Locked, started_at_ms };
                Some(SessionAction::SetMode(Mode::Locked))
            }
            (SessionState::Recording { .. }, GestureCommand::Stop) => {
                self.state = SessionState::Processing;
                Some(SessionAction::FinishRecording)
            }
            (SessionState::Recording { .. }, GestureCommand::Cancel) => {
                self.state = SessionState::Idle;
                Some(SessionAction::DiscardRecording)
            }
            _ => None,
        }
    }

    pub fn on_tick(&mut self, now_ms: u64) -> Option<SessionAction> {
        if let SessionState::Recording { started_at_ms, .. } = self.state {
            if now_ms.saturating_sub(started_at_ms) >= self.max_recording_ms {
                self.state = SessionState::Processing;
                return Some(SessionAction::FinishRecording);
            }
        }
        None
    }

    pub fn on_processing_done(&mut self) {
        if self.state == SessionState::Processing {
            self.state = SessionState::Idle;
        }
    }

    pub fn abort(&mut self) {
        self.state = SessionState::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::GestureCommand::*;

    #[test]
    fn start_then_stop_goes_to_processing() {
        let mut s = Session::new(600_000);
        assert_eq!(s.on_gesture(Start, 10), Some(SessionAction::BeginRecording));
        assert_eq!(s.state(), SessionState::Recording { mode: Mode::Hold, started_at_ms: 10 });
        assert_eq!(s.on_gesture(Stop, 900), Some(SessionAction::FinishRecording));
        assert_eq!(s.state(), SessionState::Processing);
        s.on_processing_done();
        assert_eq!(s.state(), SessionState::Idle);
    }

    #[test]
    fn lock_switches_mode() {
        let mut s = Session::new(600_000);
        s.on_gesture(Start, 0);
        assert_eq!(s.on_gesture(Lock, 200), Some(SessionAction::SetMode(Mode::Locked)));
        assert_eq!(s.state(), SessionState::Recording { mode: Mode::Locked, started_at_ms: 0 });
    }

    #[test]
    fn cancel_discards() {
        let mut s = Session::new(600_000);
        s.on_gesture(Start, 0);
        assert_eq!(s.on_gesture(Cancel, 450), Some(SessionAction::DiscardRecording));
        assert_eq!(s.state(), SessionState::Idle);
    }

    #[test]
    fn start_is_ignored_while_processing() {
        let mut s = Session::new(600_000);
        s.on_gesture(Start, 0);
        s.on_gesture(Stop, 500);
        assert_eq!(s.on_gesture(Start, 600), None);
        assert_eq!(s.on_gesture(Stop, 700), None);
        assert_eq!(s.state(), SessionState::Processing);
    }

    #[test]
    fn forgotten_lock_auto_stops_at_max_duration() {
        let mut s = Session::new(600_000);
        s.on_gesture(Start, 1_000);
        s.on_gesture(Lock, 1_200);
        assert_eq!(s.on_tick(600_999), None);
        assert_eq!(s.on_tick(601_000), Some(SessionAction::FinishRecording));
        assert_eq!(s.state(), SessionState::Processing);
        assert_eq!(s.on_tick(700_000), None);
    }

    #[test]
    fn stop_when_idle_is_ignored_and_abort_resets() {
        let mut s = Session::new(600_000);
        assert_eq!(s.on_gesture(Stop, 0), None);
        s.on_gesture(Start, 0);
        s.abort();
        assert_eq!(s.state(), SessionState::Idle);
    }
}
