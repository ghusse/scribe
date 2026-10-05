use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, State};

use scribe_core::audio::{self, AudioClip, TARGET_RATE};
use scribe_core::model::{Dictation, Term, TermSource};
use scribe_core::prompt::CorrectionPrompt;
use scribe_providers::catalog::{self, Provider};

use crate::controller::ControllerMsg;
use crate::dictation;
use crate::overlay;
use crate::secrets;
use crate::services::{now_rfc3339, Services};
use crate::settings::Settings;
use crate::tray;

type Svc<'a> = State<'a, Arc<Services>>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub fn list_dictations(svc: Svc<'_>, query: Option<String>, limit: u32, offset: u32) -> Result<Vec<Dictation>, String> {
    svc.db.lock().unwrap().list_dictations(query.as_deref(), limit, offset).map_err(err)
}

#[tauri::command]
pub fn save_edited_text(svc: Svc<'_>, id: i64, text: Option<String>) -> Result<(), String> {
    svc.db.lock().unwrap().set_edited_text(id, text.as_deref()).map_err(err)
}

#[tauri::command]
pub fn delete_dictation(svc: Svc<'_>, id: i64) -> Result<(), String> {
    if let Some(path) = svc.db.lock().unwrap().delete_dictation(id).map_err(err)? {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[tauri::command]
pub fn copy_dictation(svc: Svc<'_>, id: i64) -> Result<(), String> {
    let d = svc.db.lock().unwrap().get_dictation(id).map_err(err)?.ok_or("dictée introuvable")?;
    let text = d.best_text().ok_or("cette dictée n'a pas de texte")?.to_string();
    svc.clipboard.write_text(&text)
}

#[tauri::command]
pub async fn retranscribe(svc: Svc<'_>, id: i64) -> Result<(), String> {
    dictation::retranscribe(svc.inner().clone(), id).await
}

#[tauri::command]
pub fn list_terms(svc: Svc<'_>) -> Result<Vec<Term>, String> {
    svc.db.lock().unwrap().list_terms().map_err(err)
}

#[tauri::command]
pub fn add_term(svc: Svc<'_>, term: String, variants: Vec<String>, note: Option<String>) -> Result<i64, String> {
    svc.db.lock().unwrap().add_term(&term, &variants, note.as_deref(), TermSource::Manual, &now_rfc3339()).map_err(err)
}

#[tauri::command]
pub fn update_term(svc: Svc<'_>, id: i64, term: String, variants: Vec<String>, note: Option<String>) -> Result<(), String> {
    svc.db.lock().unwrap().update_term(id, &term, &variants, note.as_deref()).map_err(err)
}

#[tauri::command]
pub fn delete_term(svc: Svc<'_>, id: i64) -> Result<(), String> {
    svc.db.lock().unwrap().delete_term(id).map_err(err)
}

#[tauri::command]
pub fn get_settings(svc: Svc<'_>) -> Settings {
    svc.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(svc: Svc<'_>, settings: Settings) -> Result<(), String> {
    settings.validate()?;
    settings.save(&svc.paths.settings_path).map_err(err)?;
    *svc.settings.write().unwrap() = settings;
    let _ = svc.ctrl_tx.lock().unwrap().send(ControllerMsg::SettingsChanged);
    Ok(())
}

#[tauri::command]
pub fn key_status() -> HashMap<String, bool> {
    catalog::PROVIDERS.iter().map(|p| (p.id.to_string(), secrets::get_key(p.id).is_some())).collect()
}

#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Result<(), String> {
    secrets::set_key(&provider, &key)
}

#[derive(Serialize)]
pub struct ProviderTest {
    /// Duration of the call in ms, or the error.
    stt: Result<u64, String>,
    /// None when the level is raw: no correction is made, so none is tested.
    llm: Option<Result<u64, String>>,
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// Validates the keys with two tiny real calls (half a second of silence, a one-word correction).
#[tauri::command]
pub async fn test_providers(svc: Svc<'_>) -> Result<ProviderTest, String> {
    let settings = svc.settings.read().unwrap().clone();
    let (stt, llm) = dictation::build_providers(&settings).map_err(err)?;
    let silence = AudioClip { samples: vec![0; (TARGET_RATE / 2) as usize], sample_rate: TARGET_RATE };
    let wav = audio::encode_wav(&silence)?;
    let start = Instant::now();
    let stt_result = stt.transcribe(&wav, &[]).await.map(|_| elapsed_ms(start)).map_err(err);
    let llm_result = if settings.level == scribe_core::model::Level::Raw {
        None
    } else {
        let p = CorrectionPrompt { system: "Réponds <output>ok</output>.".into(), user: "<transcript>test</transcript>".into() };
        let start = Instant::now();
        Some(llm.correct(&p).await.map(|_| elapsed_ms(start)).map_err(err))
    };
    Ok(ProviderTest { stt: stt_result, llm: llm_result })
}

/// Waits (max 10 s) for the next key press and returns its virtual-key code.
#[tauri::command]
pub async fn capture_key(svc: Svc<'_>) -> Result<Option<u32>, String> {
    let (tx, rx) = mpsc::channel();
    *svc.key_capture.lock().unwrap() = Some(tx);
    let svc2 = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = rx.recv_timeout(Duration::from_secs(10)).ok();
        svc2.key_capture.lock().unwrap().take();
        r
    })
    .await
    .map_err(err)
}

/// Ends a pending capture_key early: dropping its sender makes it return None at once.
#[tauri::command]
pub fn cancel_capture(svc: Svc<'_>) {
    svc.key_capture.lock().unwrap().take();
}

#[tauri::command]
pub fn providers() -> Vec<Provider> {
    catalog::PROVIDERS.to_vec()
}

#[tauri::command]
pub fn overlay_dismiss(svc: Svc<'_>) {
    overlay::dismiss(&svc.app);
}

#[tauri::command]
pub fn open_history(svc: Svc<'_>, id: Option<i64>) {
    overlay::dismiss(&svc.app);
    tray::show_main(&svc.app);
    if let Some(id) = id {
        let _ = svc.app.emit_to("main", "focus-dictation", id);
    }
}
