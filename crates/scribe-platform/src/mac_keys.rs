//! macOS keyboard events decoded into the Windows virtual-key codes used everywhere else (settings,
//! `scribe_core::chord`, `KeyFilter`, the UI). OS-independent so it is unit-testable everywhere; `macos/hook.rs`
//! only reads the event fields and calls [`KeyMap::decode`].
//!
//! Command is the Windows key group, Option the Alt group. Letters follow the active layout (the event's typed
//! character), other keys their physical position.

/// kVK_ANSI_V: the key of « v » on QWERTY and AZERTY.
pub const KEYCODE_V: u16 = 0x09;

/// `CGEventType` values the hook listens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    KeyDown,
    KeyUp,
    FlagsChanged,
}

/// Modifier keys: keycode, virtual-key code, device-dependent flag (`NX_DEVICE*KEYMASK`) set while it is down.
const MODIFIERS: [(u16, u32, u64); 8] = [
    (0x3B, 0xA2, 0x0000_0001), // Control
    (0x3E, 0xA3, 0x0000_2000), // right Control
    (0x38, 0xA0, 0x0000_0002), // Shift
    (0x3C, 0xA1, 0x0000_0004), // right Shift
    (0x37, 0x5B, 0x0000_0008), // Command
    (0x36, 0x5C, 0x0000_0010), // right Command
    (0x3A, 0xA4, 0x0000_0020), // Option
    (0x3D, 0xA5, 0x0000_0040), // right Option
];

/// Keycode (kVK_*) → virtual-key code by physical position (ANSI layout).
const POSITIONS: &[(u16, u32)] = &[
    (0x00, 0x41), (0x01, 0x53), (0x02, 0x44), (0x03, 0x46), (0x04, 0x48), (0x05, 0x47), (0x06, 0x5A), (0x07, 0x58),
    (0x08, 0x43), (0x09, 0x56), (0x0A, 0xE2), (0x0B, 0x42), (0x0C, 0x51), (0x0D, 0x57), (0x0E, 0x45), (0x0F, 0x52),
    (0x10, 0x59), (0x11, 0x54), (0x12, 0x31), (0x13, 0x32), (0x14, 0x33), (0x15, 0x34), (0x16, 0x36), (0x17, 0x35),
    (0x18, 0xBB), (0x19, 0x39), (0x1A, 0x37), (0x1B, 0xBD), (0x1C, 0x38), (0x1D, 0x30), (0x1E, 0xDD), (0x1F, 0x4F),
    (0x20, 0x55), (0x21, 0xDB), (0x22, 0x49), (0x23, 0x50), (0x24, 0x0D), (0x25, 0x4C), (0x26, 0x4A), (0x27, 0xDE),
    (0x28, 0x4B), (0x29, 0xBA), (0x2A, 0xDC), (0x2B, 0xBC), (0x2C, 0xBF), (0x2D, 0x4E), (0x2E, 0x4D), (0x2F, 0xBE),
    (0x30, 0x09), (0x31, 0x20), (0x32, 0xC0), (0x33, 0x08), (0x35, 0x1B), (0x40, 0x80), (0x41, 0x6E), (0x43, 0x6A),
    (0x45, 0x6B), (0x47, 0x0C), (0x4B, 0x6F), (0x4C, 0x0D), (0x4E, 0x6D), (0x4F, 0x81), (0x50, 0x82), (0x51, 0xBB),
    (0x52, 0x60), (0x53, 0x61), (0x54, 0x62), (0x55, 0x63), (0x56, 0x64), (0x57, 0x65), (0x58, 0x66), (0x59, 0x67),
    (0x5A, 0x83), (0x5B, 0x68), (0x5C, 0x69), (0x60, 0x74), (0x61, 0x75), (0x62, 0x76), (0x63, 0x72), (0x64, 0x77),
    (0x65, 0x78), (0x67, 0x7A), (0x69, 0x7C), (0x6A, 0x7F), (0x6B, 0x7D), (0x6D, 0x79), (0x6F, 0x7B), (0x71, 0x7E),
    (0x72, 0x2F), (0x73, 0x24), (0x74, 0x21), (0x75, 0x2E), (0x76, 0x73), (0x77, 0x23), (0x78, 0x71), (0x79, 0x22),
    (0x7A, 0x70), (0x7B, 0x25), (0x7C, 0x27), (0x7D, 0x28), (0x7E, 0x26),
];

fn by_position(keycode: u16) -> Option<u32> {
    POSITIONS.iter().find(|(k, _)| *k == keycode).map(|&(_, vk)| vk)
}

/// Keys of the typing area, whose meaning depends on the layout (letters, digits, punctuation).
fn is_layout_key(vk: u32) -> bool {
    (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) || (0xBA..=0xC0).contains(&vk) || (0xDB..=0xDE).contains(&vk) || vk == 0xE2
}

/// A letter typed by a layout key: the event's character, or the letter of a Control + letter control character.
fn typed_letter(typed: Option<char>) -> Option<u32> {
    let c = typed?;
    let c = match c as u32 {
        1..=26 => char::from(b'a' + (c as u32 - 1) as u8),
        _ => c,
    };
    c.is_ascii_alphabetic().then(|| c.to_ascii_uppercase() as u32)
}

/// The virtual-key code of a non-modifier key: the letter it types on the active layout (« A » on AZERTY is
/// the key at the ANSI Q position), else its ANSI position. Digits keep their position (AZERTY types « & »).
pub fn vk_for_key(keycode: u16, typed: Option<char>) -> Option<u32> {
    let vk = by_position(keycode)?;
    match typed_letter(typed) {
        Some(letter) if is_layout_key(vk) => Some(letter),
        _ => Some(vk),
    }
}

/// Device-dependent flag set while the modifier `vk` is down (`None`: not a modifier). macOS no longer reports
/// modifiers through the per-key state (`CGEventSourceKeyState`), only through the flags.
pub fn modifier_mask(vk: u32) -> Option<u64> {
    MODIFIERS.iter().find(|&&(_, v, _)| v == vk).map(|&(_, _, mask)| mask)
}

/// The key that types « v » on the active layout (Command + that key pastes: shortcuts follow the layout, Dvorak
/// included), `typed` giving a keycode's character without modifiers. kVK_ANSI_V when no key types it.
pub fn paste_keycode(typed: &dyn Fn(u16) -> Option<char>) -> u16 {
    std::iter::once(KEYCODE_V)
        .chain(POSITIONS.iter().map(|&(k, _)| k))
        .find(|&k| typed(k).is_some_and(|c| c.eq_ignore_ascii_case(&'v')))
        .unwrap_or(KEYCODE_V)
}

/// Keycode of a virtual-key code by physical position (the reverse of [`vk_for_key`] without a layout).
fn keycode_by_position(vk: u32) -> Option<u16> {
    MODIFIERS
        .iter()
        .map(|&(k, v, _)| (k, v))
        .chain(POSITIONS.iter().copied())
        .find(|&(_, v)| v == vk)
        .map(|(k, _)| k)
}

/// Turns raw events into (virtual-key code, down), and remembers which keycode each code came from: a key-up
/// carries no reliable character (its modifiers may have changed since the key-down), and the key state is
/// queried by keycode.
#[derive(Debug, Default)]
pub struct KeyMap {
    /// Virtual-key code given to each keycode at its last key-down.
    down_vk: Vec<(u16, u32)>,
}

impl KeyMap {
    pub const fn new() -> Self {
        Self { down_vk: Vec::new() }
    }

    /// `flags` is the event's `CGEventFlags`, `typed` its first typed character. `None`: not a key the app
    /// handles (Fn, Caps Lock, media keys, an unknown keycode).
    pub fn decode(&mut self, kind: EventKind, keycode: u16, flags: u64, typed: Option<char>) -> Option<(u32, bool)> {
        match kind {
            EventKind::FlagsChanged => {
                let &(_, vk, mask) = MODIFIERS.iter().find(|(k, _, _)| *k == keycode)?;
                Some((vk, flags & mask != 0))
            }
            EventKind::KeyDown => {
                let known = self.down_vk.iter().find(|(k, _)| *k == keycode).map(|&(_, vk)| vk);
                let vk = known.or_else(|| vk_for_key(keycode, typed))?;
                if known.is_none() {
                    self.down_vk.push((keycode, vk));
                }
                Some((vk, true))
            }
            EventKind::KeyUp => {
                let i = self.down_vk.iter().position(|(k, _)| *k == keycode);
                let vk = match i {
                    Some(i) => self.down_vk.swap_remove(i).1,
                    None => vk_for_key(keycode, None)?,
                };
                Some((vk, false))
            }
        }
    }

    /// The keycode to query for a virtual-key code: the key currently down with that code, else its position.
    pub fn keycode(&self, vk: u32) -> Option<u16> {
        self.down_vk.iter().find(|&&(_, v)| v == vk).map(|&(k, _)| k).or_else(|| keycode_by_position(vk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RCMD_DOWN: u64 = 0x0010_0010;

    #[test]
    fn modifiers_come_from_flags_changed_with_their_side() {
        let mut m = KeyMap::new();
        assert_eq!(m.decode(EventKind::FlagsChanged, 0x36, RCMD_DOWN, None), Some((0x5C, true)));
        assert_eq!(m.decode(EventKind::FlagsChanged, 0x36, 0x0000_0100, None), Some((0x5C, false)));
        assert_eq!(m.decode(EventKind::FlagsChanged, 0x37, 0x0010_0008, None), Some((0x5B, true)));
        assert_eq!(m.decode(EventKind::FlagsChanged, 0x37, RCMD_DOWN, None), Some((0x5B, false)), "right Command still down");
        for (keycode, vk, mask) in MODIFIERS {
            assert_eq!(m.decode(EventKind::FlagsChanged, keycode, mask, None), Some((vk, true)), "{keycode:#x}");
            assert_eq!(m.decode(EventKind::FlagsChanged, keycode, !mask, None), Some((vk, false)), "{keycode:#x}");
        }
    }

    #[test]
    fn fn_and_caps_lock_are_ignored() {
        let mut m = KeyMap::new();
        assert_eq!(m.decode(EventKind::FlagsChanged, 0x3F, 0x0080_0000, None), None, "Fn");
        assert_eq!(m.decode(EventKind::FlagsChanged, 0x39, 0x0001_0000, None), None, "Caps Lock");
        assert_eq!(m.decode(EventKind::KeyDown, 0x3F, 0, None), None);
        assert_eq!(m.decode(EventKind::KeyUp, 0x90, 0, None), None, "unknown keycode");
    }

    #[test]
    fn letters_follow_the_layout_and_other_keys_their_position() {
        assert_eq!(vk_for_key(0x00, Some('a')), Some(0x41), "QWERTY A");
        assert_eq!(vk_for_key(0x0C, Some('a')), Some(0x41), "AZERTY A, at the ANSI Q position");
        assert_eq!(vk_for_key(0x0C, None), Some(0x51), "no character: position");
        assert_eq!(vk_for_key(0x29, Some('m')), Some(0x4D), "AZERTY M, a punctuation position");
        assert_eq!(vk_for_key(0x12, Some('&')), Some(0x31), "AZERTY digit row");
        assert_eq!(vk_for_key(0x00, Some('å')), Some(0x41), "Option + letter: position");
        assert_eq!(vk_for_key(0x0C, Some('\u{1}')), Some(0x41), "Control + A gives a control character");
        assert_eq!(vk_for_key(0x0C, Some('\u{1b}')), Some(0x51), "other control characters: position");
        assert_eq!(vk_for_key(0x31, Some('a')), Some(0x20), "Space never becomes a letter");
        assert_eq!(vk_for_key(0x31, Some(' ')), Some(0x20));
        assert_eq!(vk_for_key(0x0A, Some('z')), Some(0x5A), "ISO key");
        assert_eq!(vk_for_key(0x7A, None), Some(0x70), "F1");
        assert_eq!(vk_for_key(0x7E, None), Some(0x26), "up arrow");
        assert_eq!(vk_for_key(0x35, None), Some(0x1B), "Escape");
        assert_eq!(vk_for_key(0x52, Some('a')), Some(0x60), "keypad 0 keeps its code");
        assert_eq!(vk_for_key(0x99, Some('a')), None);
    }

    #[test]
    fn a_key_up_gets_the_code_of_its_key_down() {
        let mut m = KeyMap::new();
        assert_eq!(m.decode(EventKind::KeyDown, 0x0C, 0, Some('a')), Some((0x41, true)));
        assert_eq!(m.decode(EventKind::KeyDown, 0x0C, 0, Some('å')), Some((0x41, true)), "auto-repeat, Option pressed since");
        assert_eq!(m.keycode(0x41), Some(0x0C), "the key down");
        assert_eq!(m.decode(EventKind::KeyUp, 0x0C, 0, Some('q')), Some((0x41, false)));
        assert_eq!(m.keycode(0x41), Some(0x00), "released: back to the position");
        assert_eq!(m.decode(EventKind::KeyUp, 0x0C, 0, Some('a')), Some((0x51, false)), "key-up never seen down: position");
    }

    #[test]
    fn keycodes_by_position_cover_modifiers_and_keys() {
        let m = KeyMap::new();
        assert_eq!(m.keycode(0x5C), Some(0x36), "right Command");
        assert_eq!(m.keycode(0xA2), Some(0x3B), "Control");
        assert_eq!(m.keycode(0x20), Some(0x31), "Space");
        assert_eq!(m.keycode(0x26), Some(0x7E), "up arrow");
        assert_eq!(m.keycode(0x70), Some(0x7A), "F1");
        assert_eq!(m.keycode(0x11), None, "generic Ctrl has no key");
    }

    #[test]
    fn paste_uses_the_key_that_types_v() {
        assert_eq!(paste_keycode(&|k| (k == 0x09).then_some('v')), 0x09, "QWERTY, AZERTY");
        assert_eq!(paste_keycode(&|k| [(0x09, '.'), (0x2F, 'v')].iter().find(|p| p.0 == k).map(|p| p.1)), 0x2F, "Dvorak");
        assert_eq!(paste_keycode(&|k| (k == 0x2F).then_some('V')), 0x2F, "Caps Lock");
        assert_eq!(paste_keycode(&|_| None), KEYCODE_V, "unreadable layout");
        assert_eq!(paste_keycode(&|_| Some('ф')), KEYCODE_V, "no « v » (Cyrillic): its position");
    }

    #[test]
    fn modifier_masks_are_their_device_flags() {
        assert_eq!(modifier_mask(0x5C), Some(0x10), "right Command");
        assert_eq!(modifier_mask(0x5B), Some(0x08), "Command");
        assert_eq!(modifier_mask(0xA5), Some(0x40), "right Option");
        assert_eq!(modifier_mask(0x41), None, "A");
        assert_eq!(modifier_mask(0x11), None, "generic Ctrl has no key");
    }

    #[test]
    fn every_position_round_trips() {
        for &(keycode, vk) in POSITIONS {
            assert_eq!(vk_for_key(keycode, None), Some(vk), "{keycode:#x}");
        }
        for (keycode, vk, _) in MODIFIERS {
            assert_eq!(keycode_by_position(vk), Some(keycode));
        }
    }
}
