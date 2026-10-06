use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindowAsync, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    SW_SHOWNOACTIVATE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

/// The overlay must never take focus, or the paste would land in it instead of the user's field.
pub fn prepare_overlay(raw: isize) {
    unsafe {
        let h = hwnd(raw);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let flags = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize;
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | flags);
        // A style change only takes effect on the frame after SWP_FRAMECHANGED. Without it the overlay keeps the
        // frame computed at creation: a tool-window title bar and borders around the transparent pill. Called once
        // at setup on the main thread, which owns the window, so the synchronous call cannot deadlock.
        let _ = SetWindowPos(
            h,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

// show/hide are called from the controller and processing threads, while the overlay window belongs to the
// main (event-loop) thread. They must never wait for that thread: `ShowWindow`/`SetWindowPos` on another
// thread's window send synchronous messages, which deadlocks if the main thread is itself waiting (e.g. on
// the `Overlay` lock held by the caller). Hence `ShowWindowAsync` and `SWP_ASYNCWINDOWPOS`: they post.
pub fn show_overlay(raw: isize) {
    unsafe {
        let h = hwnd(raw);
        let _ = ShowWindowAsync(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            h,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_ASYNCWINDOWPOS,
        );
    }
}

pub fn hide_overlay(raw: isize) {
    unsafe {
        let _ = ShowWindowAsync(hwnd(raw), SW_HIDE);
    }
}
