use std::ffi::c_void;

use dispatch2::DispatchQueue;
use std::sync::Mutex;

use objc2_core_graphics::{CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation};
use scribe_core::insert::KeySender;

use crate::mac_keys::{modifier_mask, paste_keycode, KeyMap, KEYCODE_V};

/// `EventSourceUserData` of the events Scribe posts: the hook lets them through untouched.
pub const INJECTED_MARK: i64 = 0x5343_5249;

/// Shared with the hook, which records the keycode behind each virtual-key code.
pub static KEY_MAP: Mutex<KeyMap> = Mutex::new(KeyMap::new());

pub struct MacKeySender;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *const c_void;
    fn LMGetKbdType() -> u8;
    #[allow(clippy::too_many_arguments)]
    fn UCKeyTranslate(
        layout: *const u8,
        keycode: u16,
        action: u16,
        modifiers: u32,
        keyboard_type: u32,
        options: u32,
        dead_key_state: *mut u32,
        max_len: usize,
        len: *mut usize,
        chars: *mut u16,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(cf: *const c_void);
}

const K_UC_KEY_ACTION_DOWN: u16 = 0;
const K_UC_KEY_TRANSLATE_NO_DEAD_KEYS: u32 = 1;

/// The paste key on the active keyboard layout (`mac_keys::paste_keycode`).
fn active_paste_keycode() -> u16 {
    let mut keycode = KEYCODE_V;
    // TIS asserts it runs on the main queue (crash otherwise). Never call this from the main thread: exec_sync
    // would deadlock; the paste runs on a worker thread the main thread never waits for.
    DispatchQueue::main().exec_sync(|| keycode = layout_paste_keycode());
    keycode
}

fn layout_paste_keycode() -> u16 {
    unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return KEYCODE_V;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let keycode = if data.is_null() {
            KEYCODE_V
        } else {
            let layout = CFDataGetBytePtr(data);
            let keyboard_type = u32::from(LMGetKbdType());
            paste_keycode(&|keycode| {
                let (mut dead, mut len, mut buf) = (0u32, 0usize, [0u16; 4]);
                let status = UCKeyTranslate(
                    layout,
                    keycode,
                    K_UC_KEY_ACTION_DOWN,
                    0,
                    keyboard_type,
                    K_UC_KEY_TRANSLATE_NO_DEAD_KEYS,
                    &mut dead,
                    buf.len(),
                    &mut len,
                    buf.as_mut_ptr(),
                );
                if status != 0 {
                    return None;
                }
                char::decode_utf16(buf[..len.min(buf.len())].iter().copied()).next()?.ok()
            })
        };
        CFRelease(source);
        keycode
    }
}

/// Physical key state (any thread).
pub fn is_key_pressed(vk: u32) -> bool {
    if let Some(mask) = modifier_mask(vk) {
        return CGEventSource::flags_state(CGEventSourceStateID::HIDSystemState).0 & mask != 0;
    }
    let keycode = KEY_MAP.lock().unwrap_or_else(|p| p.into_inner()).keycode(vk);
    keycode.is_some_and(|k| CGEventSource::key_state(CGEventSourceStateID::HIDSystemState, k))
}

impl KeySender for MacKeySender {
    fn send_paste(&self) -> Result<(), String> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
        let keycode = active_paste_keycode();
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.as_deref(), keycode, down)
                .ok_or("création de l'événement Cmd+V impossible")?;
            CGEvent::set_flags(Some(&event), CGEventFlags::MaskCommand);
            // Never set the unicode string: macOS 27 then types a plain « v » (or beeps) instead of pasting.
            CGEvent::set_integer_value_field(Some(&event), CGEventField::EventSourceUserData, INJECTED_MARK);
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
        Ok(())
    }
}
