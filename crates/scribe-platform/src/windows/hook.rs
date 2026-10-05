use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

use crate::{HookConfig, KeyCallback, RawKey};

struct Shared {
    on_key: KeyCallback,
    cfg: Arc<HookConfig>,
    trigger_down: AtomicBool,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

pub struct HookHandle {
    thread_id: u32,
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn start(cfg: Arc<HookConfig>, on_key: KeyCallback) -> Result<HookHandle, String> {
    SHARED
        .set(Shared { on_key, cfg, trigger_down: AtomicBool::new(false) })
        .map_err(|_| "le hook clavier est déjà démarré".to_string())?;
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    std::thread::Builder::new()
        .name("scribe-keyboard-hook".into())
        .spawn(move || unsafe {
            let module = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default();
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) {
                Ok(hook) => {
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    let mut msg = MSG::default();
                    // A low-level hook only runs while its thread pumps messages.
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                    let _ = UnhookWindowsHookEx(hook);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("installation du hook clavier impossible : {e}")));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let thread_id = ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(HookHandle { thread_id })
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(s) = SHARED.get() {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let injected = (kb.flags.0 & LLKHF_INJECTED.0) != 0;
            if !injected {
                let msg = wparam.0 as u32;
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                let vk = kb.vkCode;
                if vk == s.cfg.trigger_vk.load(Ordering::Relaxed) {
                    s.trigger_down.store(down, Ordering::Relaxed);
                }
                (s.on_key)(RawKey { vk, down, t_ms: scribe_core::clock::now_ms() });
                let lock = s.cfg.lock_vk.load(Ordering::Relaxed);
                if lock != 0
                    && vk == lock
                    && s.trigger_down.load(Ordering::Relaxed)
                    && !s.cfg.paused.load(Ordering::Relaxed)
                {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
