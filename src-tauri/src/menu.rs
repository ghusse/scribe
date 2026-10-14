//! macOS menu bar: wiring only (the labels are `locale::app_menu_labels`).
use tauri::menu::{Menu, PredefinedMenuItem, Submenu};
use tauri::AppHandle;

use crate::locale::{self, Language};

/// Replaces Tauri's default menu, whose labels are English only. The Edit menu also carries the ⌘C / ⌘V
/// shortcuts of the webview's text fields.
pub fn set(app: &AppHandle, language: Language) -> tauri::Result<()> {
    let l = locale::app_menu_labels(language);
    let app_menu = Submenu::with_items(
        app,
        "Scribe",
        true,
        &[
            &PredefinedMenuItem::about(app, Some(l.about), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(l.hide))?,
            &PredefinedMenuItem::hide_others(app, Some(l.hide_others))?,
            &PredefinedMenuItem::show_all(app, Some(l.show_all))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(l.quit))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        l.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(l.undo))?,
            &PredefinedMenuItem::redo(app, Some(l.redo))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(l.cut))?,
            &PredefinedMenuItem::copy(app, Some(l.copy))?,
            &PredefinedMenuItem::paste(app, Some(l.paste))?,
            &PredefinedMenuItem::select_all(app, Some(l.select_all))?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        l.window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(l.minimize))?,
            &PredefinedMenuItem::maximize(app, Some(l.zoom))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, Some(l.close))?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &edit, &window])?)?;
    Ok(())
}
