use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Hold,
    Locked,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Hold => "hold",
            Mode::Locked => "locked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRole {
    Trigger,
    Lock,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub role: KeyRole,
    pub down: bool,
    pub t_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureCommand {
    Start,
    Lock,
    Stop,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GestureConfig {
    pub hold_threshold_ms: u64,
    pub double_tap_window_ms: u64,
    pub double_tap_enabled: bool,
    pub lock_key_enabled: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self { hold_threshold_ms: 300, double_tap_window_ms: 350, double_tap_enabled: true, lock_key_enabled: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Pressed { down_at: u64 },
    AwaitSecondTap { up_at: u64 },
    /// Locked, trigger still physically held (second tap or lock key).
    LockedHeld,
    Locked,
    /// Stop already emitted, waiting for the trigger release.
    StopHeld,
}

/// Turns raw trigger/lock key events into recording commands.
/// Recording starts optimistically on the first press; the gesture is resolved afterwards.
pub struct GestureDetector {
    cfg: GestureConfig,
    state: State,
}

impl GestureDetector {
    pub fn new(cfg: GestureConfig) -> Self {
        Self { cfg, state: State::Idle }
    }

    pub fn set_config(&mut self, cfg: GestureConfig) {
        self.cfg = cfg;
        self.state = State::Idle;
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    pub fn on_key(&mut self, ev: KeyEvent) -> Vec<GestureCommand> {
        use GestureCommand::*;
        match (self.state, ev.role, ev.down) {
            (_, KeyRole::Other, _) => vec![],
            (State::Idle, KeyRole::Trigger, true) => {
                self.state = State::Pressed { down_at: ev.t_ms };
                vec![Start]
            }
            (State::Pressed { down_at }, KeyRole::Trigger, false) => {
                if ev.t_ms.saturating_sub(down_at) >= self.cfg.hold_threshold_ms {
                    self.state = State::Idle;
                    vec![Stop]
                } else if self.cfg.double_tap_enabled {
                    self.state = State::AwaitSecondTap { up_at: ev.t_ms };
                    vec![]
                } else {
                    self.state = State::Idle;
                    vec![Cancel]
                }
            }
            (State::Pressed { .. }, KeyRole::Lock, true) if self.cfg.lock_key_enabled => {
                self.state = State::LockedHeld;
                vec![Lock]
            }
            (State::AwaitSecondTap { up_at }, KeyRole::Trigger, true) => {
                if ev.t_ms.saturating_sub(up_at) <= self.cfg.double_tap_window_ms {
                    self.state = State::LockedHeld;
                    vec![Lock]
                } else {
                    self.state = State::Pressed { down_at: ev.t_ms };
                    vec![Cancel, Start]
                }
            }
            (State::LockedHeld, KeyRole::Trigger, false) => {
                self.state = State::Locked;
                vec![]
            }
            (State::Locked, KeyRole::Trigger, true) => {
                self.state = State::StopHeld;
                vec![Stop]
            }
            (State::StopHeld, KeyRole::Trigger, false) => {
                self.state = State::Idle;
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn on_tick(&mut self, now_ms: u64) -> Vec<GestureCommand> {
        if let State::AwaitSecondTap { up_at } = self.state {
            if now_ms.saturating_sub(up_at) > self.cfg.double_tap_window_ms {
                self.state = State::Idle;
                return vec![GestureCommand::Cancel];
            }
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use GestureCommand::*;

    fn trig(down: bool, t: u64) -> KeyEvent { KeyEvent { role: KeyRole::Trigger, down, t_ms: t } }
    fn lock(down: bool, t: u64) -> KeyEvent { KeyEvent { role: KeyRole::Lock, down, t_ms: t } }
    fn det() -> GestureDetector { GestureDetector::new(GestureConfig::default()) }

    #[test]
    fn hold_starts_on_press_and_stops_on_release() {
        let mut d = det();
        assert_eq!(d.on_key(trig(true, 0)), vec![Start]);
        assert_eq!(d.on_key(trig(false, 800)), vec![Stop]);
    }

    #[test]
    fn autorepeat_keydown_is_ignored_while_held() {
        let mut d = det();
        assert_eq!(d.on_key(trig(true, 0)), vec![Start]);
        for t in [30, 60, 90, 500] {
            assert_eq!(d.on_key(trig(true, t)), vec![]);
        }
        assert_eq!(d.on_key(trig(false, 900)), vec![Stop]);
    }

    #[test]
    fn single_short_tap_is_cancelled_after_window() {
        let mut d = det();
        assert_eq!(d.on_key(trig(true, 0)), vec![Start]);
        assert_eq!(d.on_key(trig(false, 100)), vec![]);
        assert_eq!(d.on_tick(300), vec![]);
        assert_eq!(d.on_tick(451), vec![Cancel]);
        assert_eq!(d.on_tick(600), vec![]);
    }

    #[test]
    fn double_tap_locks_and_next_press_stops() {
        let mut d = det();
        assert_eq!(d.on_key(trig(true, 0)), vec![Start]);
        assert_eq!(d.on_key(trig(false, 100)), vec![]);
        assert_eq!(d.on_key(trig(true, 200)), vec![Lock]);
        assert_eq!(d.on_key(trig(true, 230)), vec![]); // autorepeat
        assert_eq!(d.on_key(trig(false, 260)), vec![]);
        assert_eq!(d.on_tick(5_000), vec![]);
        assert_eq!(d.on_key(trig(true, 9_000)), vec![Stop]);
        assert_eq!(d.on_key(trig(false, 9_100)), vec![]);
        assert_eq!(d.on_key(trig(true, 10_000)), vec![Start]);
    }

    #[test]
    fn late_second_press_without_tick_cancels_then_restarts() {
        let mut d = det();
        d.on_key(trig(true, 0));
        d.on_key(trig(false, 100));
        assert_eq!(d.on_key(trig(true, 1_000)), vec![Cancel, Start]);
        assert_eq!(d.on_key(trig(false, 2_000)), vec![Stop]);
    }

    #[test]
    fn lock_key_while_holding_locks() {
        let mut d = det();
        assert_eq!(d.on_key(trig(true, 0)), vec![Start]);
        assert_eq!(d.on_key(lock(true, 800)), vec![Lock]);
        assert_eq!(d.on_key(lock(false, 850)), vec![]);
        assert_eq!(d.on_key(trig(false, 900)), vec![]);
        assert_eq!(d.on_key(trig(true, 3_000)), vec![Stop]);
    }

    #[test]
    fn lock_key_ignored_when_disabled_or_not_holding() {
        let mut d = GestureDetector::new(GestureConfig { lock_key_enabled: false, ..Default::default() });
        d.on_key(trig(true, 0));
        assert_eq!(d.on_key(lock(true, 500)), vec![]);
        assert_eq!(d.on_key(trig(false, 900)), vec![Stop]);
        let mut d = det();
        assert_eq!(d.on_key(lock(true, 0)), vec![]);
    }

    #[test]
    fn short_tap_cancels_immediately_when_double_tap_disabled() {
        let mut d = GestureDetector::new(GestureConfig { double_tap_enabled: false, ..Default::default() });
        d.on_key(trig(true, 0));
        assert_eq!(d.on_key(trig(false, 100)), vec![Cancel]);
    }

    #[test]
    fn other_keys_are_ignored() {
        let mut d = det();
        let ev = KeyEvent { role: KeyRole::Other, down: true, t_ms: 0 };
        assert_eq!(d.on_key(ev), vec![]);
    }

    #[test]
    fn reset_returns_to_idle() {
        let mut d = det();
        d.on_key(trig(true, 0));
        d.reset();
        assert_eq!(d.on_key(trig(false, 900)), vec![]);
        assert_eq!(d.on_key(trig(true, 1_000)), vec![Start]);
    }
}
