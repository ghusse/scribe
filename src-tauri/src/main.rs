#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod controller;
mod dictation;
mod overlay;
mod secrets;
mod services;
mod settings;
mod tray;

use std::sync::{mpsc, Arc, Mutex, RwLock};

use tauri::{Manager, WindowEvent};

use scribe_core::model::Level;
use scribe_core::storage::Db;
use scribe_platform::HookConfig;

use crate::controller::ControllerMsg;
use crate::services::{AppPaths, Services};
use crate::settings::Settings;

const AUDIO_PURGE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

struct HookGuard(#[allow(dead_code)] scribe_platform::HookHandle);

fn main() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .setup(|app| {
            let paths = AppPaths::new(app.path().app_data_dir()?);
            std::fs::create_dir_all(&paths.audio_dir)?;
            let settings = Settings::load(&paths.settings_path);
            let db = Db::open(&paths.db_path).map_err(|e| e.to_string())?;
            let hook_cfg = Arc::new(HookConfig::new(settings.trigger_vk, settings.lock_vk));
            let (tx, rx) = mpsc::channel::<ControllerMsg>();
            let needs_setup = secrets::get_key(&settings.stt_preset).is_none()
                || (settings.level != Level::Raw && secrets::get_key("anthropic").is_none());
            let svc = Arc::new(Services {
                app: app.handle().clone(),
                db: Mutex::new(db),
                settings: RwLock::new(settings),
                paths,
                hook_cfg: hook_cfg.clone(),
                focus: scribe_platform::focus_detector(),
                clipboard: Arc::new(scribe_platform::clipboard::SystemClipboard),
                keys: scribe_platform::key_sender(),
                key_capture: Mutex::new(None),
                ctrl_tx: Mutex::new(tx.clone()),
            });
            app.manage(svc.clone());
            dictation::purge_audio(&svc);
            // Scribe lives in the tray for weeks: enforce the retention daily, not only at launch.
            let purge_svc = svc.clone();
            std::thread::Builder::new()
                .name("scribe-audio-purge".into())
                .spawn(move || loop {
                    std::thread::sleep(AUDIO_PURGE_INTERVAL);
                    dictation::purge_audio(&purge_svc);
                })?;
            overlay::setup(app.handle())?;
            tray::setup(app.handle())?;

            let key_tx = tx.clone();
            match scribe_platform::start_keyboard_hook(hook_cfg, Box::new(move |k| {
                let _ = key_tx.send(ControllerMsg::Key(k));
            })) {
                Ok(handle) => {
                    app.manage(HookGuard(handle));
                }
                Err(e) => {
                    tracing::error!("{e}");
                    overlay::toast(app.handle(), overlay::ToastLevel::Error, format!("Raccourci indisponible : {e}"), None, None);
                }
            }
            controller::spawn(svc, rx, tx);
            if needs_setup {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_dictations,
            commands::save_edited_text,
            commands::delete_dictation,
            commands::copy_dictation,
            commands::retranscribe,
            commands::list_terms,
            commands::add_term,
            commands::update_term,
            commands::delete_term,
            commands::get_settings,
            commands::save_settings,
            commands::key_status,
            commands::set_api_key,
            commands::test_providers,
            commands::capture_key,
            commands::stt_presets,
            commands::overlay_dismiss,
            commands::open_history,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Scribe");
}
