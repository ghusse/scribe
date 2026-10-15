use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    VK_V,
};

use scribe_core::insert::{KeySender, NavKey};

use crate::win_keys::arrow_strokes;

pub struct WinKeySender;

/// Physical key state (any thread; async state of the input desktop, all keys up while the session is locked).
pub fn is_key_pressed(vk: u32) -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) < 0 }
}

/// Taps `chord::VK_MENU_MASK` so that releasing Alt or Win opens no menu. Injected, so the hook ignores it.
pub fn send_menu_mask() {
    let vk = VIRTUAL_KEY(scribe_core::chord::VK_MENU_MASK as u16);
    unsafe {
        SendInput(&[key(vk, false), key(vk, true)], std::mem::size_of::<INPUT>() as i32);
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    key_with(vk, up, KEYBD_EVENT_FLAGS(0))
}

fn key_with(vk: VIRTUAL_KEY, up: bool, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { flags | KEYEVENTF_KEYUP } else { flags },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!("SendInput sent only {sent}/{} events", inputs.len()))
    }
}

impl KeySender for WinKeySender {
    fn send_paste(&self) -> Result<(), String> {
        send(&[key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)])
    }

    fn press(&self, nav: NavKey, shift: bool, times: usize) -> Result<(), String> {
        let inputs: Vec<INPUT> = arrow_strokes(nav, shift, times)
            .iter()
            .map(|s| key_with(VIRTUAL_KEY(s.vk), false, KEYBD_EVENT_FLAGS(s.flags)))
            .collect();
        send(&inputs)
    }
}
