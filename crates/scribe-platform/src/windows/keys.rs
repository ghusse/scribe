use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    VK_V,
};

use scribe_core::insert::KeySender;

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
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

impl KeySender for WinKeySender {
    fn send_paste(&self) -> Result<(), String> {
        let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err(format!("SendInput n'a envoyé que {sent}/{} événements", inputs.len()))
        }
    }
}
