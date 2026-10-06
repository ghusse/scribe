use std::sync::Mutex;

use objc2_core_graphics::{CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation};
use scribe_core::insert::KeySender;

use crate::mac_keys::{KeyMap, KEYCODE_V};

/// `EventSourceUserData` of the events Scribe posts: the hook lets them through untouched.
pub const INJECTED_MARK: i64 = 0x5343_5249;

/// Shared with the hook, which records the keycode behind each virtual-key code.
pub static KEY_MAP: Mutex<KeyMap> = Mutex::new(KeyMap::new());

pub struct MacKeySender;

/// Physical key state (any thread).
pub fn is_key_pressed(vk: u32) -> bool {
    let keycode = KEY_MAP.lock().unwrap_or_else(|p| p.into_inner()).keycode(vk);
    keycode.is_some_and(|k| CGEventSource::key_state(CGEventSourceStateID::HIDSystemState, k))
}

impl KeySender for MacKeySender {
    fn send_paste(&self) -> Result<(), String> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
        let v: [u16; 1] = [u16::from(b'v')];
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.as_deref(), KEYCODE_V, down)
                .ok_or("création de l'événement Cmd+V impossible")?;
            CGEvent::set_flags(Some(&event), CGEventFlags::MaskCommand);
            // Shortcuts match the typed character: on a layout where kVK_ANSI_V is not « v » (Dvorak), Cmd+V still pastes.
            unsafe { CGEvent::keyboard_set_unicode_string(Some(&event), 1, v.as_ptr()) };
            CGEvent::set_integer_value_field(Some(&event), CGEventField::EventSourceUserData, INJECTED_MARK);
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
        Ok(())
    }
}
