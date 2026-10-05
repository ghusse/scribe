use crate::gesture::{GestureCommand, GestureDetector, Mode};

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

    /// Pausing Scribe drops an in-flight recording (it must never be transcribed and pasted
    /// later): returns DiscardRecording when there was one. Processing is left to finish.
    pub fn on_paused(&mut self) -> Option<SessionAction> {
        if let SessionState::Recording { .. } = self.state {
            self.state = SessionState::Idle;
            return Some(SessionAction::DiscardRecording);
        }
        None
    }
}

/// Feeds the detector's commands to the session and keeps both in step: as soon as the session
/// ignores a command (press during Processing, stray Stop…), the detector is told so and the rest
/// of the batch is dropped. Without this, a double-tap during Processing would leave the detector
/// in Locked while the session is Idle, and the next real press would emit an ignored Stop.
pub fn feed(
    gesture: &mut GestureDetector,
    session: &mut Session,
    cmds: Vec<GestureCommand>,
    now_ms: u64,
) -> Vec<SessionAction> {
    let mut actions = Vec::new();
    for cmd in cmds {
        match session.on_gesture(cmd, now_ms) {
            Some(a) => actions.push(a),
            None => {
                gesture.on_command_ignored();
                break;
            }
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::GestureCommand::*;
    use crate::gesture::{GestureConfig, KeyEvent, KeyRole};

    fn trig(down: bool, t: u64) -> KeyEvent {
        KeyEvent { role: KeyRole::Trigger, down, t_ms: t }
    }

    fn key(g: &mut GestureDetector, s: &mut Session, down: bool, t: u64) -> Vec<SessionAction> {
        let cmds = g.on_key(trig(down, t));
        feed(g, s, cmds, t)
    }

    fn tick(g: &mut GestureDetector, s: &mut Session, t: u64) -> Vec<SessionAction> {
        let cmds = g.on_tick(t);
        feed(g, s, cmds, t)
    }

    #[test]
    fn double_tap_during_processing_does_not_desync_next_press() {
        let mut g = GestureDetector::new(GestureConfig::default());
        let mut s = Session::new(600_000);
        assert_eq!(key(&mut g, &mut s, true, 0), vec![SessionAction::BeginRecording]);
        assert_eq!(key(&mut g, &mut s, false, 1_000), vec![SessionAction::FinishRecording]);
        // double-tap while processing: ignored
        for (down, t) in [(true, 1_200), (false, 1_250), (true, 1_350), (false, 1_400)] {
            assert_eq!(key(&mut g, &mut s, down, t), vec![]);
        }
        assert_eq!(tick(&mut g, &mut s, 2_000), vec![]);
        s.on_processing_done();
        assert_eq!(key(&mut g, &mut s, true, 5_000), vec![SessionAction::BeginRecording]);
        assert_eq!(key(&mut g, &mut s, false, 9_000), vec![SessionAction::FinishRecording]);
    }

    #[test]
    fn press_held_through_end_of_processing_does_not_start_on_autorepeat() {
        let mut g = GestureDetector::new(GestureConfig::default());
        let mut s = Session::new(600_000);
        key(&mut g, &mut s, true, 0);
        key(&mut g, &mut s, false, 1_000);
        assert_eq!(key(&mut g, &mut s, true, 1_200), vec![]);
        s.on_processing_done();
        assert_eq!(key(&mut g, &mut s, true, 1_230), vec![]); // autorepeat
        assert_eq!(key(&mut g, &mut s, false, 2_000), vec![]);
        assert_eq!(key(&mut g, &mut s, true, 3_000), vec![SessionAction::BeginRecording]);
    }

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
    fn pausing_discards_an_active_recording_so_max_duration_never_finishes_it() {
        let mut s = Session::new(600_000);
        s.on_gesture(Start, 0);
        s.on_gesture(Lock, 200);
        assert_eq!(s.on_paused(), Some(SessionAction::DiscardRecording));
        assert_eq!(s.state(), SessionState::Idle);
        assert_eq!(s.on_tick(700_000), None);
        assert_eq!(s.on_paused(), None);
    }

    #[test]
    fn pausing_lets_processing_finish() {
        let mut s = Session::new(600_000);
        s.on_gesture(Start, 0);
        s.on_gesture(Stop, 1_000);
        assert_eq!(s.on_paused(), None);
        assert_eq!(s.state(), SessionState::Processing);
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
