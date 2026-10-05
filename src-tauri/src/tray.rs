use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::controller::ControllerMsg;
use crate::services::Services;

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Ouvrir Scribe", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Mettre en pause", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("icône par défaut").clone())
        .tooltip("Scribe")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "pause" => {
                let svc = app.state::<Arc<Services>>();
                let paused = !svc.hook_cfg.paused.load(Ordering::Relaxed);
                svc.hook_cfg.paused.store(paused, Ordering::Relaxed);
                let _ = pause.set_checked(paused);
                // The controller drops an in-flight recording on pause.
                let _ = svc.ctrl_tx.lock().unwrap().send(ControllerMsg::PauseChanged);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
