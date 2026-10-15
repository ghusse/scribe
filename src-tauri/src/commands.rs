use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use scribe_core::audio::{self, AudioClip, TARGET_RATE};
use scribe_core::model::{Dictation, Term, TermSource};
use scribe_core::permissions::{Permission, PermissionStatus};
use scribe_core::pipeline::{Corrector, ProviderError};
use scribe_core::prompt::{self, CorrectionPrompt, ImproveRequest};
use scribe_core::style::{self, Preset, Style, MAX_TASK_CHARS};
use scribe_platform::overlay_pointer::{HitArea, OverlayAreas};
use scribe_providers::catalog::{self, Provider};

use crate::audio_mute;
use crate::controller::ControllerMsg;
use crate::dictation;
use crate::error::AppError;
use crate::locale::Language;
use crate::overlay::StyleBadge;
use crate::secrets;
use crate::services::{now_rfc3339, Services};
use crate::settings::Settings;
use crate::update::{self, UpdateStatus};

type Svc<'a> = State<'a, Arc<Services>>;

/// How long `capture_key` waits for the new hotkey.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long `improve_style` waits for the corrector: it writes a whole task, not a dictation.
const IMPROVE_TIMEOUT: Duration = Duration::from_secs(60);

#[tauri::command]
pub fn list_dictations(
    svc: Svc<'_>,
    query: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<Vec<Dictation>, AppError> {
    Ok(svc.db.lock().unwrap().list_dictations(query.as_deref(), limit, offset)?)
}

#[tauri::command]
pub fn save_edited_text(svc: Svc<'_>, id: i64, text: Option<String>) -> Result<(), AppError> {
    Ok(svc.db.lock().unwrap().set_edited_text(id, text.as_deref())?)
}

/// Deletes the row and its recording.
#[tauri::command]
pub fn delete_dictation(svc: Svc<'_>, id: i64) -> Result<(), AppError> {
    if let Some(path) = svc.db.lock().unwrap().delete_dictation(id)? {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[tauri::command]
pub fn copy_dictation(svc: Svc<'_>, id: i64) -> Result<(), AppError> {
    let d = svc.db.lock().unwrap().get_dictation(id)?.ok_or(AppError::DictationNotFound)?;
    let text = d.best_text().ok_or(AppError::NoText)?.to_string();
    Ok(svc.clipboard.write_text(&text)?)
}

#[tauri::command]
pub async fn retranscribe(svc: Svc<'_>, id: i64) -> Result<(), AppError> {
    dictation::retranscribe(svc.inner().clone(), id).await
}

#[tauri::command]
pub fn list_terms(svc: Svc<'_>) -> Result<Vec<Term>, AppError> {
    Ok(svc.db.lock().unwrap().list_terms()?)
}

#[tauri::command]
pub fn add_term(svc: Svc<'_>, term: String, variants: Vec<String>, note: Option<String>) -> Result<i64, AppError> {
    Ok(svc.db.lock().unwrap().add_term(&term, &variants, note.as_deref(), TermSource::Manual, &now_rfc3339())?)
}

#[tauri::command]
pub fn update_term(
    svc: Svc<'_>,
    id: i64,
    term: String,
    variants: Vec<String>,
    note: Option<String>,
) -> Result<(), AppError> {
    Ok(svc.db.lock().unwrap().update_term(id, &term, &variants, note.as_deref())?)
}

#[tauri::command]
pub fn delete_term(svc: Svc<'_>, id: i64) -> Result<(), AppError> {
    Ok(svc.db.lock().unwrap().delete_term(id)?)
}

#[tauri::command]
pub fn get_settings(svc: Svc<'_>) -> Settings {
    svc.settings.read().unwrap().clone()
}

/// Invalid settings are neither written nor applied. A new UI language is announced to every window and the tray.
#[tauri::command]
pub fn save_settings(svc: Svc<'_>, settings: Settings) -> Result<(), AppError> {
    settings.validate()?;
    let settings = settings.normalized();
    settings.save(&svc.paths.settings_path)?;
    let before = svc.language();
    *svc.settings.write().unwrap() = settings;
    svc.send_ctrl(ControllerMsg::SettingsChanged);
    let after = svc.language();
    if after != before {
        svc.ui.language_changed(after);
    }
    Ok(())
}

/// The language the UI must use: the saved choice, else the system's.
#[tauri::command]
pub fn language(svc: Svc<'_>) -> Language {
    svc.language()
}

#[tauri::command]
pub fn key_status(svc: Svc<'_>) -> HashMap<String, bool> {
    catalog::PROVIDERS
        .iter()
        .map(|p| (p.id.to_string(), secrets::get_key(svc.secrets.as_ref(), p.id).is_some()))
        .collect()
}

#[tauri::command]
pub fn set_api_key(svc: Svc<'_>, provider: String, key: String) -> Result<(), AppError> {
    Ok(secrets::set_key(svc.secrets.as_ref(), &provider, &key)?)
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ProviderTest {
    /// Duration of the call in ms, or the error.
    stt: Result<u64, ProviderError>,
    /// None when the correction is off: no correction is made, so none is tested.
    llm: Option<Result<u64, ProviderError>>,
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// Validates the keys with two tiny real calls (half a second of silence, a one-word correction).
#[tauri::command]
pub async fn test_providers(svc: Svc<'_>) -> Result<ProviderTest, AppError> {
    let settings = svc.settings.read().unwrap().clone();
    let (stt, llm) = svc.build_providers(&settings)?;
    let silence = AudioClip { samples: vec![0; (TARGET_RATE / 2) as usize], sample_rate: TARGET_RATE };
    let wav = audio::encode_wav(&silence)?;
    let start = Instant::now();
    let stt_result = stt.transcribe(&wav, &[]).await.map(|_| elapsed_ms(start));
    let llm_result = if !settings.correction_enabled {
        None
    } else {
        let p = CorrectionPrompt {
            system: "Réponds <output>ok</output>.".into(),
            user: "<transcript>test</transcript>".into(),
        };
        let start = Instant::now();
        Some(llm.correct(&p).await.map(|_| elapsed_ms(start)))
    };
    Ok(ProviderTest { stt: stt_result, llm: llm_result })
}

/// A built-in task in the UI language, for the settings to show before the user edits it.
#[derive(Debug, PartialEq, Serialize)]
pub struct PresetView {
    preset: Preset,
    name: &'static str,
    task: &'static str,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct StyleCatalog {
    presets: Vec<PresetView>,
    styles: Vec<StyleBadge>,
    correction_enabled: bool,
}

/// The built-in tasks and the user's styles, named in the current UI language.
#[tauri::command]
pub fn style_catalog(svc: Svc<'_>) -> StyleCatalog {
    let language = svc.language();
    let settings = svc.settings.read().unwrap();
    StyleCatalog {
        presets: style::catalog()
            .into_iter()
            .filter_map(|s| s.preset)
            .map(|preset| PresetView {
                preset,
                name: style::preset_name(preset, language),
                task: style::preset_task(preset, language),
            })
            .collect(),
        styles: settings.styles.iter().map(|s| StyleBadge::of(s, language)).collect(),
        correction_enabled: settings.correction_enabled,
    }
}

/// `styles` with the missing built-in styles appended (never changing the others); nothing is saved.
#[tauri::command]
pub fn restore_builtin_styles(styles: Vec<Style>) -> Vec<Style> {
    let ids = style::catalog_ids();
    style::with_builtins(styles, &ids.iter().map(String::as_str).collect::<Vec<_>>()).0
}

/// Asks the corrector for a new Task block for a style; works without a transcription key.
#[tauri::command]
pub async fn improve_style(svc: Svc<'_>, request: ImproveRequest) -> Result<String, AppError> {
    let settings = svc.settings.read().unwrap().clone();
    let corrector = svc.build_corrector(&settings);
    improve(corrector.as_ref(), &prompt::build_improve_prompt(&request, svc.language()), IMPROVE_TIMEOUT).await
}

async fn improve(corrector: &dyn Corrector, p: &CorrectionPrompt, timeout: Duration) -> Result<String, AppError> {
    let response =
        tokio::time::timeout(timeout, corrector.correct(p)).await.map_err(|_| ProviderError::CorrectionTimeout)??;
    let task = prompt::extract_output(&response).ok_or(ProviderError::NoOutputTag)?;
    Ok(prompt::strip_tags(&task, prompt::PROMPT_TAGS).trim().chars().take(MAX_TASK_CHARS).collect())
}

/// Waits (max 10 s) for a key combination, returned once all its keys are released (virtual-key codes,
/// canonical order). None on timeout or `cancel_capture`; empty when the user pressed Échap.
#[tauri::command]
pub async fn capture_key(svc: Svc<'_>) -> Result<Option<Vec<u32>>, AppError> {
    let svc = svc.inner().clone();
    Ok(tauri::async_runtime::spawn_blocking(move || svc.key_capture.wait(CAPTURE_TIMEOUT)).await?)
}

/// Ends a pending capture_key early: it returns None at once.
#[tauri::command]
pub fn cancel_capture(svc: Svc<'_>) {
    svc.key_capture.cancel();
}

#[tauri::command]
pub fn get_autostart(svc: Svc<'_>) -> Result<bool, AppError> {
    Ok(svc.autostart.is_enabled()?)
}

/// Turns launching at login on or off; returns the state actually in place afterwards.
#[tauri::command]
pub fn set_autostart(svc: Svc<'_>, enabled: bool) -> Result<bool, AppError> {
    svc.autostart.set_enabled(enabled)?;
    Ok(svc.autostart.is_enabled()?)
}

#[tauri::command]
pub fn permissions(svc: Svc<'_>) -> Vec<PermissionStatus> {
    svc.permissions.status()
}

/// Shows the OS prompt when it still can; returns the states afterwards (the prompt answers later: read again).
#[tauri::command]
pub fn request_permission(svc: Svc<'_>, permission: Permission) -> Vec<PermissionStatus> {
    svc.permissions.request(permission);
    svc.permissions.status()
}

#[tauri::command]
pub fn open_permission_settings(svc: Svc<'_>, permission: Permission) -> Result<(), AppError> {
    Ok(svc.permissions.open_settings(permission)?)
}

#[tauri::command]
pub async fn check_update(svc: Svc<'_>) -> Result<UpdateStatus, AppError> {
    update::status(&svc).await
}

/// Installs the latest version and restarts Scribe (on Windows the installer closes and relaunches it). The process
/// ends without a recording in progress ever ending: the sound it muted is restored first.
#[tauri::command]
pub async fn install_update(svc: Svc<'_>) -> Result<(), AppError> {
    let audio = svc.audio_mute.clone();
    let _ =
        tauri::async_runtime::spawn_blocking(move || audio.restore_before_exit(audio_mute::EXIT_RESTORE_TIMEOUT)).await;
    svc.updater.install().await
}

#[tauri::command]
pub fn providers() -> Vec<Provider> {
    catalog::PROVIDERS.to_vec()
}

/// `async`: commands that touch the overlay must stay off the main (event-loop) thread, which owns the
/// overlay window. Waiting there for the `Overlay` lock while another thread holds it and talks to that
/// window could freeze the app (see `overlay::Overlay`). Enforced by a test in `commands/tests.rs`.
#[tauri::command(async)]
pub fn overlay_dismiss(svc: Svc<'_>) {
    svc.overlay.dismiss();
}

/// The overlay's visible surface and buttons (`[x, y, width, height]`, CSS pixels) after each render: on macOS its web
/// view gets no mouse event, so the hover state and the pointing-hand cursor are tracked natively against them.
#[tauri::command]
pub fn overlay_hit_areas(surface: Option<HitArea>, buttons: Vec<HitArea>) {
    forward_hit_areas(surface, buttons, scribe_platform::set_overlay_hit_areas);
}

fn forward_hit_areas(surface: Option<HitArea>, buttons: Vec<HitArea>, platform: impl FnOnce(OverlayAreas)) {
    platform(OverlayAreas { surface, buttons });
}

/// From an overlay toast: open the history, scrolled to that dictation. `async`: see `overlay_dismiss`.
#[tauri::command(async)]
pub fn open_history(svc: Svc<'_>, id: Option<i64>) {
    svc.overlay.dismiss();
    svc.ui.show_main();
    if let Some(id) = id {
        svc.ui.focus_dictation(id);
    }
}

/// From an overlay toast: rewrites a dictation with another style; the result comes as a toast.
#[tauri::command]
pub async fn restyle_dictation(svc: Svc<'_>, id: i64, style_id: String) -> Result<(), AppError> {
    dictation::restyle(svc.inner().clone(), id, &style_id).await
}

#[cfg(test)]
mod tests;
