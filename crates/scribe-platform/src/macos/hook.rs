use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use super::keys::{INJECTED_MARK, KEY_MAP};
use super::permissions::accessibility_granted;
use crate::key_filter::KeyFilter;
use crate::mac_keys::EventKind;
use crate::{HookConfig, KeyCallback};

/// How often the hook thread checks whether the Accessibility permission was granted.
const TRUST_POLL: Duration = Duration::from_secs(1);

struct Shared {
    on_key: KeyCallback,
    cfg: Arc<HookConfig>,
    filter: Mutex<KeyFilter>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
/// The tap, re-enabled when macOS disables it (a callback too slow, or secure input).
static TAP: AtomicPtr<CFMachPort> = AtomicPtr::new(ptr::null_mut());

struct RunLoop(CFRetained<CFRunLoop>);
// CFRunLoop is thread-safe, and only `CFRunLoopStop` is called through this handle.
unsafe impl Send for RunLoop {}
unsafe impl Sync for RunLoop {}

/// The run loop appears once the tap is installed, which waits for the Accessibility permission.
#[derive(Default)]
struct HookState {
    run_loop: Mutex<Option<RunLoop>>,
    stopped: AtomicBool,
}

pub struct HookHandle {
    state: Arc<HookState>,
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        if let Some(run_loop) = self.state.run_loop.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            run_loop.0.stop();
        }
    }
}

/// Without the Accessibility permission the tap cannot exist: the hook thread waits for it (the main window
/// shows how to grant it), so the shortcut works as soon as it is granted, without a restart.
pub fn start(cfg: Arc<HookConfig>, on_key: KeyCallback) -> Result<HookHandle, String> {
    SHARED
        .set(Shared { on_key, cfg, filter: Mutex::new(KeyFilter::new()) })
        .map_err(|_| "le hook clavier est déjà démarré".to_string())?;
    let state = Arc::new(HookState::default());
    let thread_state = state.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("scribe-keyboard-hook".into())
        .spawn(move || unsafe {
            if !accessibility_granted() {
                let _ = ready_tx.send(Ok(()));
                while !accessibility_granted() {
                    if thread_state.stopped.load(Ordering::Acquire) {
                        return;
                    }
                    std::thread::sleep(TRUST_POLL);
                }
            }
            let mask = (1u64 << CGEventType::KeyDown.0) | (1 << CGEventType::KeyUp.0) | (1 << CGEventType::FlagsChanged.0);
            let Some(tap) = CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(tap_proc),
                ptr::null_mut(),
            ) else {
                let _ = ready_tx.send(Err("installation du hook clavier impossible".into()));
                tracing::error!("installation du hook clavier impossible");
                return;
            };
            let (Some(source), Some(run_loop)) = (CFMachPort::new_run_loop_source(None, Some(&tap), 0), CFRunLoop::current())
            else {
                let _ = ready_tx.send(Err("installation du hook clavier impossible".into()));
                return;
            };
            run_loop.add_source(Some(&source), kCFRunLoopCommonModes);
            TAP.store(CFRetained::as_ptr(&tap).as_ptr(), Ordering::Release);
            {
                let mut slot = thread_state.run_loop.lock().unwrap_or_else(|p| p.into_inner());
                // Stopped while waiting for the permission: never run.
                if thread_state.stopped.load(Ordering::Acquire) {
                    return;
                }
                *slot = Some(RunLoop(run_loop));
            }
            let _ = ready_tx.send(Ok(()));
            CFRunLoop::run();
            TAP.store(ptr::null_mut(), Ordering::Release);
            CGEvent::tap_enable(&tap, false);
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(HookHandle { state })
}

fn typed_char(event: &CGEvent) -> Option<char> {
    let mut buf = [0u16; 4];
    let mut len = 0;
    unsafe { CGEvent::keyboard_get_unicode_string(Some(event), buf.len() as _, &mut len, buf.as_mut_ptr()) };
    char::decode_utf16(buf[..(len as usize).min(buf.len())].iter().copied()).next()?.ok()
}

// Decodes the event (`mac_keys::KeyMap`) and delegates every decision to `KeyFilter` (both unit-tested).
unsafe extern "C-unwind" fn tap_proc(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _user_info: *mut c_void,
) -> *mut CGEvent {
    let pass = event.as_ptr();
    let kind = match kind {
        CGEventType::KeyDown => EventKind::KeyDown,
        CGEventType::KeyUp => EventKind::KeyUp,
        CGEventType::FlagsChanged => EventKind::FlagsChanged,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            if let Some(tap) = TAP.load(Ordering::Acquire).as_ref() {
                CGEvent::tap_enable(tap, true);
            }
            return pass;
        }
        _ => return pass,
    };
    let Some(s) = SHARED.get() else { return pass };
    let ev = event.as_ref();
    if CGEvent::integer_value_field(Some(ev), CGEventField::EventSourceUserData) == INJECTED_MARK {
        return pass;
    }
    let keycode = CGEvent::integer_value_field(Some(ev), CGEventField::KeyboardEventKeycode) as u16;
    let typed = if kind == EventKind::KeyDown { typed_char(ev) } else { None };
    let decoded = KEY_MAP.lock().unwrap_or_else(|p| p.into_inner()).decode(kind, keycode, CGEvent::flags(Some(ev)).0, typed);
    let Some((vk, down)) = decoded else { return pass };
    let decision = s.filter.lock().unwrap_or_else(|p| p.into_inner()).on_event(
        &s.cfg,
        vk,
        0,
        down,
        false,
        scribe_core::clock::now_ms(),
        &super::keys::is_key_pressed,
    );
    // No menu mask: macOS opens no menu when Option or Command is released alone.
    if let Some(e) = decision.event {
        (s.on_key)(e);
    }
    if decision.swallow {
        ptr::null_mut()
    } else {
        pass
    }
}
