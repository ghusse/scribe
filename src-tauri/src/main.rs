#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapters;
mod bootstrap;
mod commands;
mod controller;
mod dictation;
mod overlay;
mod providers;
mod secrets;
mod services;
mod settings;
#[cfg(test)]
mod testing;
mod tray;

use std::sync::{mpsc, Arc, Mutex, RwLock};

use tauri::{Manager, WindowEvent};

use scribe_core::storage::Db;
use scribe_platform::HookConfig;

use crate::adapters::{CpalRecorder, TauriOverlayWindow, TauriUi};
use crate::controller::{ControllerDeps, ControllerMsg};
use crate::overlay::{Overlay, ToastLevel};
use crate::secrets::{KeyringStore, SecretStore};
use crate::services::{AppPaths, KeyCapture, Services};
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
            let secret_store: Arc<dyn SecretStore> = Arc::new(KeyringStore::system());
            let needs_setup = bootstrap::needs_setup(&settings, |id| secrets::get_key(secret_store.as_ref(), id).is_some());
            let svc = Arc::new(Services {
                db: Mutex::new(db),
                settings: RwLock::new(settings),
                paths,
                hook_cfg: hook_cfg.clone(),
                focus: scribe_platform::focus_detector(),
                clipboard: Arc::new(scribe_platform::clipboard::SystemClipboard::default()),
                keys: scribe_platform::key_sender(),
                secrets: secret_store,
                providers: Box::new(providers::build),
                overlay: Overlay::new(Arc::new(TauriOverlayWindow::new(app.handle().clone()))),
                ui: Arc::new(TauriUi(app.handle().clone())),
                key_capture: KeyCapture::default(),
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
            adapters::setup_overlay(app.handle())?;
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
                    svc.overlay.toast(ToastLevel::Error, bootstrap::hook_unavailable_message(&e), None, None);
                }
            }
            let deps = ControllerDeps {
                recorder: Arc::new(CpalRecorder),
                clock: Arc::new(scribe_core::clock::now_ms),
                spawn_processing: Box::new(controller::spawn_processing),
            };
            controller::spawn(svc, rx, tx, deps);
            if needs_setup {
                adapters::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if bootstrap::hides_on_close(window.label()) {
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
            commands::cancel_capture,
            commands::providers,
            commands::overlay_dismiss,
            commands::open_history,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Scribe");
}
