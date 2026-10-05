//! Hotkey combinations: 1 to 4 virtual-key codes held together, in any order (`Ctrl + Maj + A`,
//! `Win + Alt`, or a single key). Stored in canonical order (Ctrl, Maj, Alt, Win, then the other keys);
//! the keyboard hook reads them packed into one `u32` (8 bits per key) so it never takes a lock.
//!
//! A single key is matched exactly (« Ctrl droit » alone is not the left Ctrl used by shortcuts). In a
//! combination of several keys, a modifier matches either side (`Ctrl + Maj + A` with any Ctrl, any Maj).

pub const MAX_KEYS: usize = 4;
pub const VK_ESCAPE: u32 = 0x1B;
/// Unassigned key injected when the combination is pressed (see [`needs_mask`]), so that Windows sees a key
/// between the modifiers' press and release (AutoHotkey's « menu mask key », vkE8).
pub const VK_MENU_MASK: u32 = 0xE8;

const CTRL: [u32; 3] = [0x11, 0xA2, 0xA3];
const SHIFT: [u32; 3] = [0x10, 0xA0, 0xA1];
const ALT: [u32; 3] = [0x12, 0xA4, 0xA5];
const WIN: [u32; 2] = [0x5B, 0x5C];
const VK_L: u32 = 0x4C;

/// Ctrl, Maj, Alt and Windows (generic, left or right).
pub fn is_modifier(vk: u32) -> bool {
    rank(vk) < 4
}

/// Alt and Windows open a menu when released alone.
pub fn opens_menu(vk: u32) -> bool {
    ALT.contains(&vk) || WIN.contains(&vk)
}

/// Whether pressing the combination must inject `VK_MENU_MASK`: the hook lets modifiers through and swallows
/// the other keys, so Windows would see Alt or Win released alone (menu, Start menu) or Ctrl + Maj pressed
/// and released alone (keyboard layout switch).
pub fn needs_mask(packed: u32) -> bool {
    let has = |group: &[u32]| keys(packed).any(|k| group.contains(&k));
    keys(packed).any(opens_menu) || (has(&CTRL) && has(&SHIFT))
}

const GROUPS: [&[u32]; 4] = [&CTRL, &SHIFT, &ALT, &WIN];
/// The code a side-less modifier is stored as: its left key.
const LEFT: [u32; 4] = [0xA2, 0xA0, 0xA4, 0x5B];

fn rank(vk: u32) -> u8 {
    GROUPS.iter().position(|group| group.contains(&vk)).unwrap_or(4) as u8
}

fn is_combination(packed: u32) -> bool {
    keys(packed).nth(1).is_some()
}

/// Canonical order (Ctrl, Maj, Alt, Win, then the other keys by code), duplicates removed. In a combination
/// of several keys, modifiers are side-less and stored as their left key.
pub fn normalize(keys: &[u32]) -> Vec<u32> {
    let mut v = keys.to_vec();
    v.sort_by_key(|&k| (rank(k), k));
    v.dedup();
    if v.len() > 1 {
        for k in v.iter_mut() {
            if let Some(&left) = LEFT.get(rank(*k) as usize) {
                *k = left;
            }
        }
        v.dedup();
    }
    v
}

/// Whether the event key `vk` is the combination key `key` (either side for a modifier in a combination).
pub fn same_key(packed: u32, key: u32, vk: u32) -> bool {
    key == vk || (is_combination(packed) && is_modifier(key) && rank(key) == rank(vk))
}

/// Whether `vk` is one of the keys of the combination.
pub fn contains(packed: u32, vk: u32) -> bool {
    vk != 0 && keys(packed).any(|k| same_key(packed, k, vk))
}

/// Whether the combination key `key` is physically down, per `is_pressed` (either side when side-less).
pub fn key_pressed(packed: u32, key: u32, is_pressed: &dyn Fn(u32) -> bool) -> bool {
    if is_combination(packed) && is_modifier(key) {
        GROUPS[rank(key) as usize].iter().any(|&k| is_pressed(k))
    } else {
        is_pressed(key)
    }
}

/// Rejects what the hook cannot honour. `lock_vk` 0 means no lock key.
pub fn validate(keys: &[u32], lock_vk: u32) -> Result<(), String> {
    if keys.is_empty() {
        return Err("choisissez un raccourci".into());
    }
    if keys.len() > MAX_KEYS {
        return Err(format!("un raccourci compte au plus {MAX_KEYS} touches"));
    }
    if keys.iter().any(|&k| k == 0 || k > 0xFE) || normalize(keys).len() != keys.len() {
        return Err("raccourci invalide".into());
    }
    if keys.contains(&VK_ESCAPE) {
        return Err("Échap ne peut pas faire partie du raccourci".into());
    }
    // Side-aware: in `Ctrl + A`, « Ctrl droit » as the lock key would always be read as part of the combination.
    if contains(pack(keys), lock_vk) {
        return Err("la touche de verrouillage ne peut pas faire partie du raccourci".into());
    }
    Ok(())
}

/// Combinations Windows keeps for itself: they still work as a trigger, but with a side effect.
pub fn reserved_warning(keys: &[u32]) -> Option<&'static str> {
    let has = |group: &[u32]| keys.iter().any(|k| group.contains(k));
    let only = |groups: &[&[u32]]| keys.iter().all(|k| groups.iter().any(|g| g.contains(k)));
    if has(&WIN) && keys.contains(&VK_L) {
        return Some("Windows réserve Win + L (verrouillage de session) : ce raccourci ne peut pas être intercepté.");
    }
    if keys.len() == 2 && has(&ALT) && has(&SHIFT) && only(&[&ALT, &SHIFT]) {
        return Some("Windows peut utiliser Alt + Maj pour changer la langue du clavier.");
    }
    if keys.len() == 2 && has(&CTRL) && has(&SHIFT) && only(&[&CTRL, &SHIFT]) {
        return Some("Windows peut utiliser Ctrl + Maj pour changer la disposition du clavier.");
    }
    None
}

/// Packs a validated combination into one `u32` (0 = none).
pub fn pack(keys: &[u32]) -> u32 {
    keys.iter().take(MAX_KEYS).enumerate().fold(0, |acc, (i, &k)| acc | ((k & 0xFF) << (8 * i)))
}

/// The keys of a packed combination.
pub fn keys(packed: u32) -> impl Iterator<Item = u32> {
    (0..MAX_KEYS).map(move |i| (packed >> (8 * i)) & 0xFF).filter(|&k| k != 0)
}

/// How a capture ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// Not part of the capture: key-up or auto-repeat of a key already down when the capture started.
    Ignored,
    Pending,
    /// All keys released: the largest set held at once (the latest one on a tie), in canonical order.
    Done(Vec<u32>),
    /// Échap pressed.
    Cancelled,
}

/// Records the combination typed in the settings, from raw key events.
#[derive(Debug, Default)]
pub struct ChordRecorder {
    held: Vec<u32>,
    best: Vec<u32>,
}

impl ChordRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// `repeat`: an auto-repeat key-down (the key was already down).
    pub fn feed(&mut self, vk: u32, down: bool, repeat: bool) -> Recorded {
        if repeat && !self.held.contains(&vk) {
            // E.g. the Entrée that clicked « Changer », still held past the repeat delay.
            return Recorded::Ignored;
        }
        if down {
            if vk == VK_ESCAPE {
                return Recorded::Cancelled;
            }
            if !self.held.contains(&vk) {
                self.held.push(vk);
                // `>=`: after a slip (Ctrl+Maj, Maj released, then A) the latest full set wins.
                if self.held.len() >= self.best.len() {
                    self.best = self.held.clone();
                }
            }
            return Recorded::Pending;
        }
        // Key-up of a key held before the capture started (e.g. the Entrée that clicked « Changer »).
        let Some(i) = self.held.iter().position(|&k| k == vk) else {
            return Recorded::Ignored;
        };
        self.held.remove(i);
        if self.held.is_empty() {
            Recorded::Done(normalize(&self.best))
        } else {
            Recorded::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u32 = 0xA2;
    const RCTRL: u32 = 0xA3;
    const LSHIFT: u32 = 0xA0;
    const LALT: u32 = 0xA4;
    const LWIN: u32 = 0x5B;
    const A: u32 = 0x41;
    const SPACE: u32 = 0x20;

    #[test]
    fn modifiers_and_menu_keys() {
        for k in [0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C] {
            assert!(is_modifier(k), "{k:#x}");
        }
        for k in [A, SPACE, 0x5D, 0x14, 0x70] {
            assert!(!is_modifier(k), "{k:#x}");
        }
        for k in [0x12, 0xA4, 0xA5, 0x5B, 0x5C] {
            assert!(opens_menu(k), "{k:#x}");
        }
        for k in [0x11, 0xA2, 0xA3, 0x10, 0xA0, A] {
            assert!(!opens_menu(k), "{k:#x}");
        }
    }

    #[test]
    fn normalize_orders_ctrl_shift_alt_win_then_others_and_dedups() {
        assert_eq!(normalize(&[A, LWIN, LALT, LSHIFT, LCTRL]), vec![LCTRL, LSHIFT, LALT, LWIN, A]);
        assert_eq!(normalize(&[0x42, A, A]), vec![A, 0x42]);
        assert_eq!(normalize(&[]), Vec::<u32>::new());
    }

    #[test]
    fn a_single_key_keeps_its_side_a_combination_does_not() {
        assert_eq!(normalize(&[RCTRL]), vec![RCTRL]);
        assert_eq!(normalize(&[0xA5]), vec![0xA5]);
        assert_eq!(normalize(&[RCTRL, RCTRL]), vec![RCTRL]);
        assert_eq!(normalize(&[A, 0xA1, RCTRL]), vec![LCTRL, LSHIFT, A]);
        assert_eq!(normalize(&[0x5C, 0xA5]), vec![LALT, LWIN]);
        assert_eq!(normalize(&[0x11, 0x10, 0x12, A]), vec![LCTRL, LSHIFT, LALT, A], "generic codes");
        assert_eq!(normalize(&[RCTRL, LCTRL]), vec![LCTRL], "both Ctrl keys are one side-less Ctrl");
    }

    #[test]
    fn modifiers_match_either_side_only_in_a_combination() {
        let single = pack(&[RCTRL]);
        assert!(contains(single, RCTRL));
        assert!(!contains(single, LCTRL), "Ctrl droit alone is not the left Ctrl");
        let combo = pack(&[LCTRL, LSHIFT, A]);
        for vk in [LCTRL, RCTRL, 0x11, LSHIFT, 0xA1, A] {
            assert!(contains(combo, vk), "{vk:#x}");
        }
        for vk in [LALT, 0x42, LWIN] {
            assert!(!contains(combo, vk), "{vk:#x}");
        }
        assert!(same_key(combo, LCTRL, RCTRL));
        assert!(!same_key(combo, LCTRL, LSHIFT));
        assert!(!same_key(pack(&[A, 0x42]), A, 0x42), "non-modifiers match exactly");
        let win = pack(&[LALT, LWIN]);
        assert!(contains(win, 0x5C) && contains(win, 0xA5));
    }

    #[test]
    fn key_pressed_checks_both_sides_of_a_side_less_modifier() {
        let combo = pack(&[LCTRL, A]);
        assert!(key_pressed(combo, LCTRL, &|vk| vk == RCTRL));
        assert!(key_pressed(combo, LCTRL, &|vk| vk == LCTRL));
        assert!(!key_pressed(combo, LCTRL, &|vk| vk == A));
        assert!(key_pressed(combo, A, &|vk| vk == A));
        assert!(!key_pressed(combo, A, &|vk| vk != A));
        let single = pack(&[RCTRL]);
        assert!(!key_pressed(single, RCTRL, &|vk| vk == LCTRL), "a single key is checked exactly");
        assert!(key_pressed(single, RCTRL, &|vk| vk == RCTRL));
    }

    #[test]
    fn validate_accepts_one_to_four_distinct_keys() {
        assert_eq!(validate(&[RCTRL], SPACE), Ok(()));
        assert_eq!(validate(&[LCTRL, LSHIFT, LALT, A], SPACE), Ok(()));
        assert_eq!(validate(&[SPACE], 0), Ok(()), "no lock key: Space may be the trigger");
    }

    #[test]
    fn validate_rejects_bad_combinations() {
        assert_eq!(validate(&[], SPACE), Err("choisissez un raccourci".into()));
        assert_eq!(validate(&[LCTRL, LSHIFT, LALT, LWIN, A], SPACE), Err("un raccourci compte au plus 4 touches".into()));
        assert_eq!(validate(&[0], SPACE), Err("raccourci invalide".into()));
        assert_eq!(validate(&[0xFF], SPACE), Err("raccourci invalide".into()));
        assert_eq!(validate(&[A, A], SPACE), Err("raccourci invalide".into()));
        assert_eq!(validate(&[LCTRL, RCTRL, A], SPACE), Err("raccourci invalide".into()), "Ctrl twice");
        assert_eq!(validate(&[LCTRL, VK_ESCAPE], SPACE), Err("Échap ne peut pas faire partie du raccourci".into()));
        assert_eq!(
            validate(&[LCTRL, SPACE], SPACE),
            Err("la touche de verrouillage ne peut pas faire partie du raccourci".into())
        );
        assert_eq!(
            validate(&[LCTRL, A], RCTRL),
            Err("la touche de verrouillage ne peut pas faire partie du raccourci".into()),
            "Ctrl is side-less in a combination"
        );
        assert_eq!(validate(&[LCTRL], RCTRL), Ok(()), "a single key keeps its side");
    }

    #[test]
    fn reserved_combinations_are_flagged() {
        assert!(reserved_warning(&[LWIN, 0x4C]).unwrap().contains("Win + L"));
        assert!(reserved_warning(&[0x5C, 0x4C, LCTRL]).unwrap().contains("Win + L"));
        assert!(reserved_warning(&[LSHIFT, LALT]).unwrap().contains("Alt + Maj"));
        assert!(reserved_warning(&[0xA1, 0xA5]).unwrap().contains("Alt + Maj"));
        assert!(reserved_warning(&[LCTRL, 0xA1]).unwrap().contains("Ctrl + Maj"));
        assert_eq!(reserved_warning(&[LCTRL, LSHIFT, A]), None, "Ctrl + Maj + A is not a layout switch");
        assert_eq!(reserved_warning(&[LALT, LSHIFT, A]), None);
        assert_eq!(reserved_warning(&[LALT, LWIN]), None);
        assert_eq!(reserved_warning(&[0x4C]), None, "L alone");
        assert_eq!(reserved_warning(&[LALT, LALT]), None, "two keys but no Maj");
        assert_eq!(reserved_warning(&[RCTRL]), None);
    }

    #[test]
    fn pack_round_trips() {
        for c in [vec![RCTRL], vec![LCTRL, LSHIFT, A], vec![LCTRL, LSHIFT, LALT, LWIN], vec![0xFE]] {
            let p = pack(&c);
            assert_eq!(keys(p).collect::<Vec<_>>(), c);
            for &k in &c {
                assert!(contains(p, k));
            }
        }
        assert_eq!(pack(&[]), 0);
        assert_eq!(keys(0).count(), 0);
        assert!(!contains(pack(&[A]), 0x42));
        assert!(!contains(0, 0), "an empty combination contains no key, not even 0");
        assert_eq!(pack(&[1, 2, 3, 4, 5]), pack(&[1, 2, 3, 4]), "only 4 keys fit");
    }

    #[test]
    fn repeats_of_a_captured_key_change_nothing() {
        let mut r = ChordRecorder::new();
        r.feed(LCTRL, true, false);
        assert_eq!(r.feed(LCTRL, true, true), Recorded::Pending);
        assert_eq!(r.feed(LCTRL, false, false), Recorded::Done(vec![LCTRL]));
    }

    #[test]
    fn the_mask_is_needed_for_alt_win_and_ctrl_with_maj() {
        assert!(needs_mask(pack(&[LALT, A])));
        assert!(needs_mask(pack(&[LWIN, A])));
        assert!(needs_mask(pack(&[0xA5])), "AltGr alone");
        assert!(needs_mask(pack(&[LCTRL, LSHIFT, A])), "Ctrl + Maj would switch the layout");
        assert!(needs_mask(pack(&[RCTRL, 0xA1])));
        assert!(!needs_mask(pack(&[RCTRL])));
        assert!(!needs_mask(pack(&[LCTRL, A])));
        assert!(!needs_mask(pack(&[LSHIFT, A])));
        assert!(!needs_mask(pack(&[A])));
    }

    fn feed_all(r: &mut ChordRecorder, evs: &[(u32, bool)]) -> Vec<Recorded> {
        evs.iter().map(|&(k, d)| r.feed(k, d, false)).collect()
    }

    #[test]
    fn records_the_combination_on_full_release() {
        let mut r = ChordRecorder::new();
        let out = feed_all(&mut r, &[(LCTRL, true), (LSHIFT, true), (A, true), (A, true), (A, false), (LCTRL, false)]);
        assert!(out.iter().all(|o| *o == Recorded::Pending));
        assert_eq!(r.feed(LSHIFT, false, false), Recorded::Done(vec![LCTRL, LSHIFT, A]));
    }

    #[test]
    fn records_a_single_key_and_modifier_only_combinations() {
        let mut r = ChordRecorder::new();
        r.feed(RCTRL, true, false);
        assert_eq!(r.feed(RCTRL, false, false), Recorded::Done(vec![RCTRL]));
        let mut r = ChordRecorder::new();
        feed_all(&mut r, &[(LWIN, true), (LALT, true), (LWIN, false)]);
        assert_eq!(r.feed(LALT, false, false), Recorded::Done(vec![LALT, LWIN]));
        let mut r = ChordRecorder::new();
        feed_all(&mut r, &[(RCTRL, true), (0xA1, true), (A, true), (A, false), (0xA1, false)]);
        assert_eq!(r.feed(RCTRL, false, false), Recorded::Done(vec![LCTRL, LSHIFT, A]), "side-less in a combination");
    }

    #[test]
    fn the_latest_largest_set_wins() {
        let mut r = ChordRecorder::new();
        feed_all(&mut r, &[(LCTRL, true), (LSHIFT, true), (LSHIFT, false), (A, true), (A, false)]);
        assert_eq!(r.feed(LCTRL, false, false), Recorded::Done(vec![LCTRL, A]));
        let mut r = ChordRecorder::new();
        feed_all(&mut r, &[(LCTRL, true), (LSHIFT, true), (A, true), (A, false), (LSHIFT, false)]);
        assert_eq!(r.feed(LCTRL, false, false), Recorded::Done(vec![LCTRL, LSHIFT, A]), "a smaller set never replaces a larger one");
    }

    #[test]
    fn escape_cancels_and_stray_key_ups_are_ignored() {
        let mut r = ChordRecorder::new();
        assert_eq!(r.feed(0x0D, false, false), Recorded::Ignored, "key-up of the Entrée that clicked « Changer »");
        assert_eq!(r.feed(0x0D, true, true), Recorded::Ignored, "its auto-repeat");
        r.feed(LCTRL, true, false);
        assert_eq!(r.feed(VK_ESCAPE, true, false), Recorded::Cancelled);
        let mut r = ChordRecorder::new();
        assert_eq!(r.feed(VK_ESCAPE, true, false), Recorded::Cancelled);
    }
}
