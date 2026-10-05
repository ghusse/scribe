//! Decision logic of the low-level keyboard hook, kept OS-independent so it is unit-testable everywhere.
//! `windows/hook.rs` only decodes `KBDLLHOOKSTRUCT`, calls [`KeyFilter::on_event`] and applies the decision.
//!
//! The trigger is a combination (`scribe_core::chord`). It is pressed when all its keys are held and no
//! other key is (strict), and released as soon as one of its keys is. The gesture detector only sees
//! these two edges, as if the combination were a single key.
use std::sync::atomic::Ordering;

use scribe_core::chord;
use scribe_core::gesture::{KeyEvent, KeyRole};

use crate::{HookConfig, HookEvent, RawKey};

const VK_LCONTROL: u32 = 0xA2;
/// Scan code of the left Ctrl key-down/up that Windows sends before Alt droit on AltGr layouts (AZERTY…).
pub const ALTGR_FAKE_CTRL_SCAN: u32 = 0x21D;

/// What the hook does with one keyboard event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDecision {
    /// Event for the controller (`None` for injected events, e.g. our own Ctrl+V).
    pub event: Option<HookEvent>,
    /// True: the event is swallowed (not passed to the focused application).
    pub swallow: bool,
    /// True: inject `chord::VK_MENU_MASK` (the combination holds Alt or Win, which would open a menu
    /// when released without another key in between).
    pub inject_mask: bool,
}

#[derive(Debug, Default)]
pub struct KeyFilter {
    /// Keys physically down, as seen by the hook.
    held: Vec<u32>,
    /// Keys whose key-down was swallowed: their auto-repeats and key-up are swallowed too, so the
    /// application never sees half a keystroke.
    swallowed: Vec<u32>,
    /// The packed combination currently pressed (trigger-down sent, trigger-up not yet).
    active: Option<u32>,
}

impl KeyFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `is_pressed(vk)` queries the physical key state. A key-up can be lost (Win+L switches to the secure
    /// desktop while keys are held): a stale « held » key would block or fake the combination, and a stale
    /// active combination would swallow the lock key (Space) system-wide. A `&dyn` rather than a generic, so
    /// the hook does not compile a second, untested copy of this function.
    ///
    /// `scan` is the hardware scan code: the left Ctrl that AltGr adds (`ALTGR_FAKE_CTRL_SCAN`) is ignored like
    /// an injected event, so AltGr reads as Alt droit alone (as a trigger, and in a capture).
    pub fn on_event(
        &mut self,
        cfg: &HookConfig,
        vk: u32,
        scan: u32,
        down: bool,
        injected: bool,
        t_ms: u64,
        is_pressed: &dyn Fn(u32) -> bool,
    ) -> KeyDecision {
        if injected || (vk == VK_LCONTROL && scan == ALTGR_FAKE_CTRL_SCAN) {
            return KeyDecision { event: None, swallow: false, inject_mask: false };
        }
        let trigger = cfg.trigger.load(Ordering::Relaxed);
        let lock = cfg.lock_vk.load(Ordering::Relaxed);
        let paused = cfg.paused.load(Ordering::Relaxed);
        let capturing = cfg.capturing.load(Ordering::Relaxed);
        let mut gestures = Vec::new();
        let edge = |role, down| KeyEvent { role, down, t_ms };

        // Combination changed in the settings while held, or one of its keys released unseen: end the press.
        // The key of a key-up event is still down for `is_pressed`; a key-down of a combination key that is
        // physically up is a new press after a lost key-up.
        if let Some(active) = self.active {
            let lost = |k| (down || !chord::same_key(active, k, vk)) && !chord::key_pressed(active, k, is_pressed);
            if active != trigger || chord::keys(active).any(lost) {
                self.active = None;
                gestures.push(edge(KeyRole::Trigger, false));
            }
        }

        let in_trigger = chord::contains(trigger, vk);
        let is_lock = lock != 0 && vk == lock && !in_trigger;
        let mut inject_mask = false;
        // A key-down for a key already held is an auto-repeat, unless its key-up was lost: then the key is
        // physically up (the hook runs before the key state is updated) and this is a new press.
        let repeat = down && self.held.contains(&vk) && is_pressed(vk);
        let swallow = if repeat {
            // No edge.
            self.swallowed.contains(&vk)
        } else if down {
            if !self.held.contains(&vk) {
                self.held.push(vk);
            }
            let mut swallow = capturing;
            if in_trigger && self.active.is_none() && self.completes(trigger, vk, is_pressed) {
                self.active = Some(trigger);
                gestures.push(edge(KeyRole::Trigger, true));
                if !paused && !capturing {
                    // Modifiers pass, so the system never sees them stuck; the other keys are the hotkey's own.
                    swallow |= !chord::is_modifier(vk);
                    inject_mask = chord::needs_mask(trigger);
                }
            }
            if is_lock {
                gestures.push(edge(KeyRole::Lock, true));
                swallow |= self.active.is_some() && !paused;
            }
            self.swallowed.retain(|&k| k != vk);
            if swallow {
                self.swallowed.push(vk);
            }
            swallow
        } else {
            self.held.retain(|&k| k != vk);
            if in_trigger && self.active.take().is_some() {
                gestures.push(edge(KeyRole::Trigger, false));
            }
            if is_lock {
                gestures.push(edge(KeyRole::Lock, false));
            }
            let was_swallowed = self.swallowed.contains(&vk);
            self.swallowed.retain(|&k| k != vk);
            was_swallowed
        };
        KeyDecision { event: Some(HookEvent { key: RawKey { vk, down, repeat, t_ms }, gestures }), swallow, inject_mask }
    }

    /// Whether `vk` going down completes the combination: all its keys held and no other key. Held keys that
    /// are physically up (lost key-up) are forgotten first.
    fn completes(&mut self, trigger: u32, vk: u32, is_pressed: &dyn Fn(u32) -> bool) -> bool {
        self.held.retain(|&k| k == vk || is_pressed(k));
        chord::keys(trigger).all(|k| self.held.iter().any(|&h| chord::same_key(trigger, k, h)))
            && self.held.iter().all(|&h| chord::contains(trigger, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RCTRL: u32 = 0xA3;
    const LCTRL: u32 = 0xA2;
    const LSHIFT: u32 = 0xA0;
    const LALT: u32 = 0xA4;
    const LWIN: u32 = 0x5B;
    const SPACE: u32 = 0x20;
    const A: u32 = 0x41;
    const B: u32 = 0x42;

    fn pressed(_: u32) -> bool {
        true
    }

    fn trig(down: bool) -> KeyEvent {
        KeyEvent { role: KeyRole::Trigger, down, t_ms: 7 }
    }

    fn lock(down: bool) -> KeyEvent {
        KeyEvent { role: KeyRole::Lock, down, t_ms: 7 }
    }

    struct H {
        cfg: HookConfig,
        f: KeyFilter,
    }

    impl H {
        fn new(trigger: &[u32], lock: u32) -> Self {
            Self { cfg: HookConfig::new(trigger, lock), f: KeyFilter::new() }
        }
        fn with(&mut self, vk: u32, down: bool, is_pressed: &dyn Fn(u32) -> bool) -> KeyDecision {
            self.f.on_event(&self.cfg, vk, 0, down, false, 7, is_pressed)
        }
        fn ev(&mut self, vk: u32, down: bool) -> KeyDecision {
            self.with(vk, down, &pressed)
        }
        fn gestures(&mut self, vk: u32, down: bool) -> Vec<KeyEvent> {
            self.ev(vk, down).event.unwrap().gestures
        }
    }

    #[test]
    fn forwards_every_real_event_with_its_time() {
        let mut h = H::new(&[RCTRL], SPACE);
        let d = h.f.on_event(&h.cfg, A, 0, true, false, 42, &pressed);
        assert_eq!(
            d,
            KeyDecision {
                event: Some(HookEvent { key: RawKey { vk: A, down: true, repeat: false, t_ms: 42 }, gestures: vec![] }),
                swallow: false,
                inject_mask: false
            }
        );
        let d = h.f.on_event(&h.cfg, A, 0, false, false, 43, &pressed);
        assert_eq!(d.event.unwrap().key, RawKey { vk: A, down: false, repeat: false, t_ms: 43 });
    }

    #[test]
    fn single_modifier_trigger_passes_through_and_emits_edges() {
        let mut h = H::new(&[RCTRL], SPACE);
        let d = h.ev(RCTRL, true);
        assert!(!d.swallow, "a modifier is never swallowed");
        assert!(!d.inject_mask, "Ctrl opens no menu");
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)]);
        assert_eq!(h.gestures(RCTRL, true), vec![], "auto-repeat: no edge");
        assert_eq!(h.gestures(RCTRL, false), vec![trig(false)]);
        assert_eq!(h.gestures(RCTRL, false), vec![], "a second key-up is no edge");
    }

    #[test]
    fn combination_fires_when_complete_in_any_order() {
        for order in [[LCTRL, LSHIFT, A], [A, LSHIFT, LCTRL], [LSHIFT, A, LCTRL]] {
            let mut h = H::new(&[LCTRL, LSHIFT, A], SPACE);
            assert_eq!(h.gestures(order[0], true), vec![]);
            assert_eq!(h.gestures(order[1], true), vec![]);
            assert_eq!(h.gestures(order[2], true), vec![trig(true)], "{order:?}");
            assert_eq!(h.gestures(order[1], false), vec![trig(false)], "any key released ends it");
            assert_eq!(h.gestures(order[0], false), vec![]);
            assert_eq!(h.gestures(order[2], false), vec![]);
        }
    }

    #[test]
    fn non_modifier_key_is_swallowed_down_repeat_and_up_only_when_it_completes_the_combination() {
        let mut h = H::new(&[LCTRL, LSHIFT, A], SPACE);
        assert!(!h.ev(A, true).swallow, "A alone types an a");
        assert!(!h.ev(A, false).swallow);
        h.ev(LCTRL, true);
        h.ev(LSHIFT, true);
        let d = h.ev(A, true);
        assert!(d.swallow);
        assert!(h.ev(A, true).swallow, "auto-repeat");
        assert!(!h.ev(LSHIFT, false).swallow, "modifiers pass");
        assert!(h.ev(A, false).swallow, "its key-up too, even after the combination ended");
        assert!(!h.ev(A, true).swallow, "Ctrl + A afterwards is not the combination");
        assert!(!h.ev(A, false).swallow);
    }

    #[test]
    fn a_key_held_before_the_modifiers_is_not_swallowed() {
        let mut h = H::new(&[LCTRL, A], SPACE);
        assert!(!h.ev(A, true).swallow);
        let d = h.ev(LCTRL, true);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)]);
        assert!(!d.swallow);
        assert!(!h.ev(A, false).swallow, "its key-down reached the application, so does its key-up");
    }

    #[test]
    fn an_extra_key_held_blocks_the_combination() {
        let mut h = H::new(&[LCTRL, LSHIFT], SPACE);
        h.ev(B, true);
        h.ev(LCTRL, true);
        assert_eq!(h.gestures(LSHIFT, true), vec![], "B is held");
        h.ev(LSHIFT, false);
        h.ev(B, false);
        assert_eq!(h.gestures(LSHIFT, true), vec![trig(true)], "B released: Ctrl + Maj fires");
        // A key pressed while the combination is active does not end it.
        assert_eq!(h.gestures(B, true), vec![]);
        assert_eq!(h.gestures(LSHIFT, false), vec![trig(false)]);
    }

    #[test]
    fn re_pressing_one_key_is_a_second_tap() {
        let mut h = H::new(&[LCTRL, LSHIFT, A], SPACE);
        h.ev(LCTRL, true);
        h.ev(LSHIFT, true);
        assert_eq!(h.gestures(A, true), vec![trig(true)]);
        assert_eq!(h.gestures(A, false), vec![trig(false)]);
        let d = h.ev(A, true);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)]);
        assert!(d.swallow);
    }

    #[test]
    fn combinations_with_alt_or_win_inject_the_menu_mask_once_per_press() {
        let mut h = H::new(&[LALT, LWIN], SPACE);
        assert!(!h.ev(LWIN, true).inject_mask);
        let d = h.ev(LALT, true);
        assert!(d.inject_mask);
        assert!(!d.swallow);
        assert!(!h.ev(LALT, true).inject_mask, "auto-repeat");
        h.ev(LALT, false);
        assert!(h.ev(LALT, true).inject_mask, "second tap");
        let mut h = H::new(&[LCTRL, LSHIFT, A], SPACE);
        h.ev(LCTRL, true);
        h.ev(LSHIFT, true);
        assert!(h.ev(A, true).inject_mask, "Ctrl + Maj released alone would switch the keyboard layout");
        let mut h = H::new(&[LCTRL, A], SPACE);
        h.ev(LCTRL, true);
        assert!(!h.ev(A, true).inject_mask, "Ctrl + A: nothing to mask");
        // No mask during a capture: every key is swallowed, the system sees neither Alt nor Win.
        let mut h = H::new(&[LALT, LWIN], SPACE);
        h.cfg.capturing.store(true, Ordering::Relaxed);
        h.ev(LWIN, true);
        let d = h.ev(LALT, true);
        assert!(d.swallow && !d.inject_mask);
    }

    #[test]
    fn swallows_lock_down_and_up_while_the_combination_is_held() {
        let mut h = H::new(&[RCTRL], SPACE);
        h.ev(RCTRL, true);
        let d = h.ev(SPACE, true);
        assert!(d.swallow);
        assert_eq!(d.event.unwrap().gestures, vec![lock(true)], "a swallowed key still reaches the controller");
        assert!(h.ev(SPACE, true).swallow, "auto-repeat");
        let d = h.ev(SPACE, false);
        assert!(d.swallow);
        assert_eq!(d.event.unwrap().gestures, vec![lock(false)]);
        assert!(!h.ev(A, true).swallow, "other keys pass");
    }

    #[test]
    fn lock_released_after_the_trigger_is_still_swallowed() {
        // Otherwise the application gets an orphan Space key-up.
        let mut h = H::new(&[RCTRL], SPACE);
        h.ev(RCTRL, true);
        assert!(h.ev(SPACE, true).swallow);
        h.ev(RCTRL, false);
        assert!(h.ev(SPACE, false).swallow);
        assert!(!h.ev(SPACE, true).swallow, "without the trigger, Space types a space");
    }

    #[test]
    fn lock_passes_without_the_trigger_and_still_reports_its_edges() {
        let mut h = H::new(&[RCTRL], SPACE);
        let d = h.ev(SPACE, true);
        assert!(!d.swallow);
        assert_eq!(d.event.unwrap().gestures, vec![lock(true)]);
        assert_eq!(h.gestures(SPACE, true), vec![], "auto-repeat");
    }

    #[test]
    fn paused_never_swallows_nor_masks_but_keeps_tracking() {
        let mut h = H::new(&[LALT, A], SPACE);
        h.cfg.paused.store(true, Ordering::Relaxed);
        h.ev(LALT, true);
        let d = h.ev(A, true);
        assert!(!d.swallow);
        assert!(!d.inject_mask);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)], "the controller drops it while paused");
        assert!(!h.ev(SPACE, true).swallow);
        h.cfg.paused.store(false, Ordering::Relaxed);
        assert!(!h.ev(A, false).swallow, "its key-down passed");
    }

    #[test]
    fn capture_swallows_every_key_pressed_during_it() {
        let mut h = H::new(&[RCTRL], SPACE);
        h.ev(B, true); // held before the capture
        h.cfg.capturing.store(true, Ordering::Relaxed);
        let d = h.ev(LWIN, true);
        assert!(d.swallow);
        assert!(!d.inject_mask, "the system never sees Win: nothing to mask");
        assert!(h.ev(LWIN, true).swallow, "auto-repeat");
        assert!(!h.ev(B, false).swallow, "its key-down reached the application");
        assert!(h.ev(RCTRL, true).swallow, "even the current trigger");
        h.cfg.capturing.store(false, Ordering::Relaxed);
        assert!(h.ev(LWIN, false).swallow, "key-up of a key swallowed during the capture");
        assert!(h.ev(RCTRL, false).swallow);
    }

    #[test]
    fn no_lock_key_never_swallows() {
        let mut h = H::new(&[RCTRL], 0);
        h.ev(RCTRL, true);
        assert!(!h.ev(0, true).swallow, "vk 0 is not a lock key");
        let d = h.ev(SPACE, true);
        assert!(!d.swallow);
        assert_eq!(d.event.unwrap().gestures, vec![]);
    }

    #[test]
    fn a_lock_key_inside_the_combination_is_part_of_the_combination() {
        // Rejected by the settings; the hook must still not emit both roles.
        let mut h = H::new(&[LCTRL, SPACE], SPACE);
        h.ev(LCTRL, true);
        let d = h.ev(SPACE, true);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)]);
    }

    #[test]
    fn injected_events_are_ignored_and_do_not_change_state() {
        let mut h = H::new(&[RCTRL], SPACE);
        let d = h.f.on_event(&h.cfg, RCTRL, 0, true, true, 1, &pressed);
        assert_eq!(d, KeyDecision { event: None, swallow: false, inject_mask: false });
        assert!(!h.ev(SPACE, true).swallow, "an injected trigger-down does not arm the lock");
        h.ev(SPACE, false);
        h.ev(RCTRL, true);
        let d = h.f.on_event(&h.cfg, SPACE, 0, true, true, 1, &pressed);
        assert_eq!(d, KeyDecision { event: None, swallow: false, inject_mask: false });
        h.f.on_event(&h.cfg, RCTRL, 0, false, true, 1, &pressed);
        assert!(h.ev(SPACE, true).swallow, "an injected trigger-up does not disarm the lock");
    }

    #[test]
    fn lost_key_up_of_the_trigger_ends_the_press() {
        // Win+L while holding the trigger: the key-up goes to the secure desktop, never to the hook.
        let mut h = H::new(&[RCTRL], SPACE);
        h.ev(RCTRL, true);
        let d = h.with(SPACE, true, &|vk| vk != RCTRL);
        assert!(!d.swallow, "the trigger is physically up: Space must reach the application");
        assert_eq!(d.event.unwrap().gestures, vec![trig(false), lock(true)], "the detector sees the release");
        // The stale state is cleared: no more queries needed.
        let d = h.with(SPACE, false, &|_| panic!("state should be reset"));
        assert!(!d.swallow);
        // The next real press works again.
        assert_eq!(h.with(RCTRL, true, &|_| false).event.unwrap().gestures, vec![trig(true)]);
    }

    #[test]
    fn asks_the_physical_state_of_the_combination_keys_only() {
        let mut h = H::new(&[LCTRL, LSHIFT], SPACE);
        h.ev(LCTRL, true);
        h.ev(LSHIFT, true);
        let d = h.with(SPACE, true, &|vk| vk == LCTRL || vk == LSHIFT);
        assert!(d.swallow);
        h.ev(SPACE, false);
        let d = h.with(SPACE, true, &|vk| vk == LCTRL);
        assert!(!d.swallow, "Maj is up");
    }

    #[test]
    fn stale_held_keys_neither_block_nor_fake_the_combination() {
        let mut h = H::new(&[LWIN, LALT], SPACE);
        // Win + L: both key-ups lost.
        h.ev(LWIN, true);
        h.ev(0x4C, true);
        let d = h.with(LALT, true, &|_| false);
        assert_eq!(d.event.unwrap().gestures, vec![], "Win is physically up: Alt alone is not Win + Alt");
        h.ev(LALT, false);
        h.with(LWIN, true, &|_| false);
        let d = h.with(LALT, true, &|vk| vk == LWIN);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)], "the stale L does not block it");
    }

    #[test]
    fn trigger_changed_while_held_ends_the_press() {
        let mut h = H::new(&[RCTRL], SPACE);
        h.ev(RCTRL, true);
        h.cfg.set_trigger(&[A]);
        let d = h.ev(SPACE, true);
        assert!(!d.swallow, "the new trigger was never pressed");
        assert_eq!(d.event.unwrap().gestures, vec![trig(false), lock(true)]);
        h.ev(SPACE, false);
        assert_eq!(h.gestures(RCTRL, false), vec![], "the old trigger's key-up is no edge");
        assert_eq!(h.gestures(A, true), vec![trig(true)], "the new trigger works");
    }

    #[test]
    fn a_new_press_after_a_lost_key_up_is_not_an_auto_repeat() {
        let mut h = H::new(&[LCTRL, A], SPACE);
        let a_up = |vk| vk != A;
        h.ev(A, true); // key-up lost
        assert_eq!(h.with(LCTRL, true, &a_up).event.unwrap().gestures, vec![], "A is physically up");
        let d = h.with(A, true, &a_up);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)]);
        assert!(d.swallow);
    }

    #[test]
    fn a_combination_accepts_either_side_of_its_modifiers() {
        let mut h = H::new(&[LCTRL, LSHIFT, A], SPACE);
        h.ev(RCTRL, true);
        h.ev(0xA1, true);
        let d = h.ev(A, true);
        assert_eq!(d.event.unwrap().gestures, vec![trig(true)]);
        assert!(d.swallow);
        assert_eq!(h.gestures(RCTRL, false), vec![trig(false)], "releasing the right Ctrl ends it");
        h.ev(RCTRL, true);
        h.ev(A, false);
        assert_eq!(h.gestures(A, true), vec![trig(true)]);
        // The lock press checks that the right-hand modifiers are still down.
        assert!(h.with(SPACE, true, &|vk| [RCTRL, 0xA1, A].contains(&vk)).swallow);
        h.ev(SPACE, false);
        assert!(!h.with(SPACE, true, &|vk| [RCTRL, A].contains(&vk)).swallow, "no Maj on either side");
    }

    #[test]
    fn a_single_modifier_trigger_is_side_specific() {
        let mut h = H::new(&[RCTRL], SPACE);
        assert_eq!(h.gestures(LCTRL, true), vec![], "the left Ctrl of Ctrl+C is not the trigger");
        h.ev(LCTRL, false);
        assert_eq!(h.gestures(RCTRL, true), vec![trig(true)]);
    }

    #[test]
    fn altgr_reads_as_alt_droit_alone() {
        // AltGr = fake left Ctrl (scan 0x21D) then Alt droit.
        let altgr = |h: &mut H, down: bool| {
            let fake = h.f.on_event(&h.cfg, LCTRL, ALTGR_FAKE_CTRL_SCAN, down, false, 7, &pressed);
            assert_eq!(fake, KeyDecision { event: None, swallow: false, inject_mask: false }, "ignored, passed through");
            h.ev(0xA5, down)
        };
        let mut h = H::new(&[0xA5], SPACE);
        assert_eq!(altgr(&mut h, true).event.unwrap().gestures, vec![trig(true)], "AltGr as the trigger fires");
        assert_eq!(altgr(&mut h, false).event.unwrap().gestures, vec![trig(false)]);
        // With Ctrl + Alt as the trigger, AltGr + @ is not the combination.
        let mut h = H::new(&[LCTRL, LALT], SPACE);
        assert_eq!(altgr(&mut h, true).event.unwrap().gestures, vec![]);
        // The real left Ctrl (scan 0x1D) is a key like any other.
        let d = h.f.on_event(&h.cfg, LCTRL, 0x1D, true, false, 7, &pressed);
        assert!(d.event.is_some());
    }

    #[test]
    fn a_lost_key_up_of_the_completing_key_makes_the_next_press_a_new_tap() {
        let mut h = H::new(&[LCTRL, A], SPACE);
        h.ev(LCTRL, true);
        assert_eq!(h.gestures(A, true), vec![trig(true)]); // A's key-up is lost
        let d = h.with(A, true, &|vk| vk != A);
        assert_eq!(d.event.as_ref().unwrap().gestures, vec![trig(false), trig(true)], "release then a new press");
        assert!(d.swallow);
        assert!(!d.event.unwrap().key.repeat);
        let d = h.ev(A, true);
        assert!(d.event.unwrap().key.repeat, "a real auto-repeat");
    }

    #[test]
    fn a_swallowed_key_whose_key_up_was_lost_types_normally_afterwards() {
        let mut h = H::new(&[LCTRL, A], SPACE);
        h.ev(LCTRL, true);
        assert!(h.ev(A, true).swallow); // A's key-up is lost, then Ctrl is released
        h.ev(LCTRL, false);
        // A pressed alone: the stale swallow must not carry over to its down or its up.
        let d = h.with(A, true, &|vk| vk != A);
        assert!(!d.swallow);
        assert!(!h.ev(A, false).swallow);
        // Same when the stale key is dropped while checking another completion.
        let mut h = H::new(&[LCTRL, A], SPACE);
        h.ev(LCTRL, true);
        assert!(h.ev(A, true).swallow); // lost key-up
        h.ev(LCTRL, false);
        h.with(LCTRL, true, &|vk| vk != A); // forgets A
        h.ev(LCTRL, false);
        h.ev(0x42, true);
        assert!(!h.ev(A, true).swallow, "B is held: no combination");
        assert!(!h.ev(A, false).swallow, "the key-up of an unswallowed press passes");
    }

    #[test]
    fn an_empty_combination_never_fires() {
        let mut h = H::new(&[], SPACE);
        assert_eq!(h.gestures(RCTRL, true), vec![]);
        assert!(!h.ev(A, true).swallow);
    }
}
