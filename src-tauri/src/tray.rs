//! Tray icon and menu: wiring only (the pause rule is `controller::toggle_pause`, the labels `locale::tray_labels`).
use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::adapters::show_main;
use crate::controller;
use crate::locale::{self, Language};
use crate::services::Services;

/// The menu entries, managed so a language change can relabel them.
pub struct TrayMenu {
    open: MenuItem<Wry>,
    pause: CheckMenuItem<Wry>,
    quit: MenuItem<Wry>,
}

impl TrayMenu {
    pub fn relabel(&self, language: Language) {
        let labels = locale::tray_labels(language);
        let _ = self.open.set_text(labels.open);
        let _ = self.pause.set_text(labels.pause);
        let _ = self.quit.set_text(labels.quit);
    }
}

pub fn setup(app: &AppHandle, language: Language) -> tauri::Result<()> {
    let labels = locale::tray_labels(language);
    let open = MenuItem::with_id(app, "open", labels.open, true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", labels.pause, true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &quit])?;
    let toggle = pause.clone();
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("default window icon").clone())
        .tooltip("Scribe")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "pause" => {
                let paused = controller::toggle_pause(&app.state::<Arc<Services>>());
                let _ = toggle.set_checked(paused);
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
    app.manage(TrayMenu { open, pause, quit });
    Ok(())
}
