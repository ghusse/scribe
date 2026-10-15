//! Key sequences Scribe injects on Windows. OS-independent so they are unit-tested everywhere; `windows/keys.rs`
//! only turns each [`Stroke`] into an `INPUT` for one `SendInput` call.

use scribe_core::insert::NavKey;

pub const VK_SHIFT: u16 = 0x10;
pub const VK_LEFT: u16 = 0x25;
pub const VK_RIGHT: u16 = 0x27;
/// `KEYBDINPUT::dwFlags` bits.
pub const KEYEVENTF_EXTENDEDKEY: u32 = 0x1;
pub const KEYEVENTF_KEYUP: u32 = 0x2;

/// One key event: virtual-key code and `KEYBDINPUT::dwFlags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    pub vk: u16,
    pub flags: u32,
}

pub fn arrow_vk(key: NavKey) -> u16 {
    match key {
        NavKey::Left => VK_LEFT,
        NavKey::Right => VK_RIGHT,
    }
}

/// An arrow pressed `times` times, with Shift held around the whole burst when `shift`. Arrows are extended keys:
/// without the flag they read as the numeric keypad's.
pub fn arrow_strokes(key: NavKey, shift: bool, times: usize) -> Vec<Stroke> {
    let vk = arrow_vk(key);
    let tap =
        [Stroke { vk, flags: KEYEVENTF_EXTENDEDKEY }, Stroke { vk, flags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP }];
    let burst = tap.iter().cycle().take(2 * times).copied();
    if !shift {
        return burst.collect();
    }
    std::iter::once(Stroke { vk: VK_SHIFT, flags: 0 })
        .chain(burst)
        .chain(std::iter::once(Stroke { vk: VK_SHIFT, flags: KEYEVENTF_KEYUP }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWN: u32 = KEYEVENTF_EXTENDEDKEY;
    const UP: u32 = KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP;

    #[test]
    fn arrows_are_the_extended_arrow_keys() {
        assert_eq!(arrow_vk(NavKey::Left), VK_LEFT);
        assert_eq!(
            arrow_strokes(NavKey::Right, false, 1),
            [Stroke { vk: VK_RIGHT, flags: DOWN }, Stroke { vk: VK_RIGHT, flags: UP }]
        );
    }

    #[test]
    fn shift_wraps_the_whole_burst() {
        let left = |flags| Stroke { vk: VK_LEFT, flags };
        assert_eq!(
            arrow_strokes(NavKey::Left, true, 2),
            [
                Stroke { vk: VK_SHIFT, flags: 0 },
                left(DOWN),
                left(UP),
                left(DOWN),
                left(UP),
                Stroke { vk: VK_SHIFT, flags: KEYEVENTF_KEYUP },
            ]
        );
        assert!(arrow_strokes(NavKey::Left, false, 0).is_empty());
    }
}
