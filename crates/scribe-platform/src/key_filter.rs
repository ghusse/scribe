//! Decision logic of the low-level keyboard hook, kept OS-independent so it is unit-testable everywhere.
//! `windows/hook.rs` only decodes `KBDLLHOOKSTRUCT` and calls [`KeyFilter::on_event`].
use std::sync::atomic::Ordering;

use crate::{HookConfig, RawKey};

/// What the hook does with one keyboard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyDecision {
    /// Event to forward to the controller (`None` for injected events, e.g. our own Ctrl+V).
    pub key: Option<RawKey>,
    /// True: the event is swallowed (not passed to the focused application).
    pub swallow: bool,
}

/// Tracks the trigger key so the lock key is swallowed only while the trigger is held.
#[derive(Debug, Default)]
pub struct KeyFilter {
    /// The trigger vk that was down, if any. Remembering *which* vk (rather than a bool) means that
    /// changing the trigger in the settings while it is held cannot leave a stale "held" state.
    held_trigger: Option<u32>,
}

impl KeyFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `is_pressed(vk)` queries the physical key state; it is asked only before swallowing, to recover
    /// from a lost key-up (e.g. Win+L switches to the secure desktop while the trigger is held), which
    /// would otherwise swallow the lock key (Space) system-wide. A `&dyn` rather than a generic, so the hook
    /// does not compile a second, untested copy of this function.
    pub fn on_event(
        &mut self,
        cfg: &HookConfig,
        vk: u32,
        down: bool,
        injected: bool,
        t_ms: u64,
        is_pressed: &dyn Fn(u32) -> bool,
    ) -> KeyDecision {
        if injected {
            return KeyDecision { key: None, swallow: false };
        }
        let trigger = cfg.trigger_vk.load(Ordering::Relaxed);
        if vk == trigger {
            self.held_trigger = down.then_some(vk);
        }
        let key = Some(RawKey { vk, down, t_ms });
        let lock = cfg.lock_vk.load(Ordering::Relaxed);
        let wants_swallow = lock != 0
            && vk == lock
            && vk != trigger
            && self.held_trigger == Some(trigger)
            && !cfg.paused.load(Ordering::Relaxed);
        if !wants_swallow {
            return KeyDecision { key, swallow: false };
        }
        if !is_pressed(trigger) {
            self.held_trigger = None;
            return KeyDecision { key, swallow: false };
        }
        KeyDecision { key, swallow: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIGGER: u32 = 0xA3; // Right Ctrl
    const LOCK: u32 = 0x20; // Space
    const OTHER: u32 = 0x41; // A

    fn pressed(_: u32) -> bool {
        true
    }
    fn released(_: u32) -> bool {
        false
    }

    fn ev(f: &mut KeyFilter, cfg: &HookConfig, vk: u32, down: bool) -> KeyDecision {
        f.on_event(cfg, vk, down, false, 7, &pressed)
    }

    #[test]
    fn forwards_every_real_event_with_its_time() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        let d = f.on_event(&cfg, OTHER, true, false, 42, &pressed);
        assert_eq!(d, KeyDecision { key: Some(RawKey { vk: OTHER, down: true, t_ms: 42 }), swallow: false });
        let d = f.on_event(&cfg, OTHER, false, false, 43, &pressed);
        assert_eq!(d.key, Some(RawKey { vk: OTHER, down: false, t_ms: 43 }));
    }

    #[test]
    fn swallows_lock_down_and_up_while_trigger_held() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        assert!(!ev(&mut f, &cfg, TRIGGER, true).swallow, "the trigger itself is never swallowed");
        let d = ev(&mut f, &cfg, LOCK, true);
        assert!(d.swallow);
        assert_eq!(d.key, Some(RawKey { vk: LOCK, down: true, t_ms: 7 }), "a swallowed key still reaches the controller");
        assert!(ev(&mut f, &cfg, LOCK, false).swallow);
        assert!(!ev(&mut f, &cfg, OTHER, true).swallow, "other keys pass");
    }

    #[test]
    fn lock_passes_without_the_trigger() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        assert!(!ev(&mut f, &cfg, LOCK, true).swallow);
        ev(&mut f, &cfg, TRIGGER, true);
        ev(&mut f, &cfg, TRIGGER, false);
        assert!(!ev(&mut f, &cfg, LOCK, true).swallow, "released trigger");
    }

    #[test]
    fn paused_never_swallows() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        cfg.paused.store(true, Ordering::Relaxed);
        let mut f = KeyFilter::new();
        ev(&mut f, &cfg, TRIGGER, true);
        let d = ev(&mut f, &cfg, LOCK, true);
        assert!(!d.swallow);
        assert!(d.key.is_some());
    }

    #[test]
    fn no_lock_key_never_swallows() {
        let cfg = HookConfig::new(TRIGGER, 0);
        let mut f = KeyFilter::new();
        ev(&mut f, &cfg, TRIGGER, true);
        assert!(!ev(&mut f, &cfg, 0, true).swallow, "vk 0 is not a lock key");
        assert!(!ev(&mut f, &cfg, LOCK, true).swallow);
    }

    #[test]
    fn lock_equal_to_trigger_is_never_swallowed() {
        let cfg = HookConfig::new(TRIGGER, TRIGGER);
        let mut f = KeyFilter::new();
        assert!(!ev(&mut f, &cfg, TRIGGER, true).swallow);
        assert!(!ev(&mut f, &cfg, TRIGGER, true).swallow, "auto-repeat of the trigger");
    }

    #[test]
    fn injected_events_are_ignored_and_do_not_change_state() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        let d = f.on_event(&cfg, TRIGGER, true, true, 1, &pressed);
        assert_eq!(d, KeyDecision { key: None, swallow: false });
        assert!(!ev(&mut f, &cfg, LOCK, true).swallow, "an injected trigger-down does not arm the lock");
        ev(&mut f, &cfg, TRIGGER, true);
        let d = f.on_event(&cfg, LOCK, true, true, 1, &pressed);
        assert_eq!(d, KeyDecision { key: None, swallow: false }, "an injected lock is neither forwarded nor swallowed");
        f.on_event(&cfg, TRIGGER, false, true, 1, &pressed);
        assert!(ev(&mut f, &cfg, LOCK, true).swallow, "an injected trigger-up does not disarm the lock");
    }

    #[test]
    fn lost_trigger_key_up_does_not_swallow_the_lock_forever() {
        // Win+L while holding the trigger: the key-up goes to the secure desktop, never to the hook.
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        ev(&mut f, &cfg, TRIGGER, true);
        let d = f.on_event(&cfg, LOCK, true, false, 7, &released);
        assert!(!d.swallow, "the trigger is physically up: Space must reach the application");
        assert!(d.key.is_some());
        // The stale state is cleared: no more queries needed, still not swallowed.
        let d = f.on_event(&cfg, LOCK, true, false, 7, &|_| panic!("state should be reset"));
        assert!(!d.swallow);
    }

    #[test]
    fn asks_the_physical_state_of_the_current_trigger() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        ev(&mut f, &cfg, TRIGGER, true);
        assert!(f.on_event(&cfg, LOCK, true, false, 7, &|vk| vk == TRIGGER).swallow);
        assert!(!f.on_event(&cfg, LOCK, true, false, 7, &|vk| vk != TRIGGER).swallow);
    }

    #[test]
    fn trigger_changed_while_held_does_not_leave_a_stale_state() {
        let cfg = HookConfig::new(TRIGGER, LOCK);
        let mut f = KeyFilter::new();
        ev(&mut f, &cfg, TRIGGER, true);
        cfg.trigger_vk.store(OTHER, Ordering::Relaxed);
        // The old trigger's key-up is no longer seen as a trigger event.
        ev(&mut f, &cfg, TRIGGER, false);
        assert!(!ev(&mut f, &cfg, LOCK, true).swallow, "the new trigger was never pressed");
        ev(&mut f, &cfg, OTHER, true);
        assert!(ev(&mut f, &cfg, LOCK, true).swallow, "the new trigger arms the lock");
    }
}
