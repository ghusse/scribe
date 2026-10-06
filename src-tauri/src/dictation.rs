use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use scribe_core::audio::{self, AudioClip};
use scribe_core::focus::{self, FocusSnapshot};
use scribe_core::gesture::Mode;
use scribe_core::insert::{self, InsertResult};
use scribe_core::model::{Level, NewDictation, Outcome, TranscriptionUpdate};
use scribe_core::pipeline::{self, PipelineError, PipelineOutput, ProviderError};
use scribe_core::prompt;
use scribe_core::storage::Db;

use crate::overlay::{OverlayEvent, ToastLevel};
use crate::services::{now_rfc3339, Services};
use crate::settings::Settings;

pub const FOCUS_TIMEOUT_MS: u64 = 300;

pub struct Captured {
    pub clip: AudioClip,
    pub mode: Mode,
    pub focus_start: FocusSnapshot,
}

/// Spec §7: a correction failure is always mentioned (« non corrigé »), whatever the insertion result.
fn with_correction_note(msg: &str, correction_error: Option<&str>) -> String {
    match correction_error {
        Some(err) => format!("{msg} (non corrigé : {err})"),
        None => msg.to_string(),
    }
}

fn preview(text: &str) -> String {
    let p: String = text.chars().take(140).collect();
    if p.len() < text.len() { format!("{p}…") } else { p }
}

/// What the overlay shows once a transcribed dictation was delivered (or not).
pub fn feedback(inserted: InsertResult, correction_error: Option<&str>, final_text: &str, id: Option<i64>) -> OverlayEvent {
    let toast = |level, message: String, preview: Option<String>| OverlayEvent::Toast { level, message, preview, dictation_id: id };
    let p = Some(preview(final_text));
    match inserted {
        InsertResult::Pasted => match correction_error {
            Some(err) => toast(ToastLevel::Info, format!("Inséré sans correction ({err})"), None),
            None => OverlayEvent::Idle,
        },
        InsertResult::PastedUncertain => toast(ToastLevel::Uncertain, with_correction_note("Texte inséré ?", correction_error), p),
        InsertResult::ClipboardOnly | InsertResult::PasteFailed => {
            toast(ToastLevel::Copied, with_correction_note("Texte copié dans le presse-papier", correction_error), p)
        }
        InsertResult::NotPasted => toast(
            ToastLevel::Copied,
            with_correction_note("Non inséré : texte copié, collez-le avec Ctrl+V", correction_error),
            p,
        ),
        InsertResult::ClipboardFailed => toast(
            ToastLevel::Error,
            with_correction_note("Presse-papier indisponible : texte dans l'historique", correction_error),
            p,
        ),
    }
}

/// What the overlay shows when the dictation could not be transcribed (saved for a retry).
pub fn failure_feedback(e: &ProviderError, id: Option<i64>) -> OverlayEvent {
    OverlayEvent::Toast {
        level: ToastLevel::Error,
        message: format!("Échec de la transcription : {e}. Réessayez depuis l'historique."),
        preview: None,
        dictation_id: id,
    }
}

/// Facts about the recording itself, known before any network call.
pub struct RecordMeta {
    pub created_at: String,
    pub mode: Mode,
    pub app_name: Option<String>,
    pub audio_path: Option<String>,
    pub duration_ms: u64,
}

/// How the pipeline ended, for the history row.
pub enum Delivery<'a> {
    Failed(&'a ProviderError),
    Done { out: &'a PipelineOutput, transcriber: String, corrector: String, inserted: InsertResult },
}

/// The history row of a dictation.
pub fn build_record(meta: &RecordMeta, settings: &Settings, delivery: Delivery<'_>) -> NewDictation {
    let mut record = NewDictation {
        created_at: meta.created_at.clone(),
        mode: meta.mode,
        app_name: meta.app_name.clone(),
        app_bundle_id: None,
        audio_path: meta.audio_path.clone(),
        duration_ms: meta.duration_ms as i64,
        raw_text: None,
        final_text: None,
        level: settings.level,
        transcriber: Some(settings.stt_model.clone()),
        corrector: None,
        stt_ms: None,
        llm_ms: None,
        outcome: Outcome::Error,
        error: None,
    };
    match delivery {
        Delivery::Failed(e) => record.error = Some(e.to_string()),
        Delivery::Done { out, transcriber, corrector, inserted } => {
            record.raw_text = Some(out.raw.clone());
            record.final_text = Some(out.final_text.clone());
            record.transcriber = Some(transcriber);
            record.corrector = (settings.level != Level::Raw).then_some(corrector);
            record.stt_ms = Some(out.stt_ms as i64);
            record.llm_ms = out.llm_ms.map(|v| v as i64);
            record.outcome = inserted.outcome();
            record.error = out.correction_error.clone();
        }
    }
    record
}

/// Pastes (or copies) the text, checking that the focus did not move since the recording started.
async fn deliver(svc: &Arc<Services>, start: FocusSnapshot, text: String, restore_delay_ms: u64) -> InsertResult {
    let svc = svc.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let end = focus::snapshot_with_timeout(svc.focus.clone(), FOCUS_TIMEOUT_MS);
        let plan = insert::decide(&start, &end);
        insert::perform(plan, &text, svc.clipboard.as_ref(), svc.keys.as_ref(), svc.field.as_ref(), restore_delay_ms, &|ms| {
            std::thread::sleep(Duration::from_millis(ms))
        })
    })
    .await
    .unwrap_or(InsertResult::ClipboardFailed)
}

pub async fn process(svc: Arc<Services>, cap: Captured) {
    let settings = svc.settings.read().unwrap().clone();
    let duration = audio::duration_ms(&cap.clip);
    if duration < settings.min_recording_ms || audio::is_silent(&cap.clip, settings.silence_threshold_dbfs) {
        svc.overlay.emit(OverlayEvent::Idle);
        return;
    }
    let wav = match audio::encode_wav(&cap.clip) {
        Ok(w) => w,
        Err(e) => return svc.overlay.toast(ToastLevel::Error, format!("Encodage audio impossible : {e}"), None, None),
    };
    let created_at = now_rfc3339();
    // Written before any network call: a dictation is never lost.
    let audio_file = svc.paths.audio_dir.join(format!("{}.wav", created_at.replace([':', '.'], "-")));
    let audio_path = match std::fs::write(&audio_file, &wav) {
        Ok(()) => Some(audio_file.to_string_lossy().into_owned()),
        Err(e) => {
            tracing::error!("écriture audio impossible : {e}");
            None
        }
    };
    let terms = svc.db.lock().unwrap().list_terms().unwrap_or_default();
    let meta = RecordMeta { created_at, mode: cap.mode, app_name: cap.focus_start.app_name.clone(), audio_path, duration_ms: duration };
    let result = match svc.build_providers(&settings) {
        Ok((stt, llm)) => pipeline::run(&settings.pipeline_config(), &wav, &terms, meta.app_name.as_deref(), stt.as_ref(), llm.as_ref())
            .await
            .map(|out| (out, stt.name(), llm.name())),
        Err(e) => Err(PipelineError::Transcription(e)),
    };

    match result {
        Err(PipelineError::Empty) => {
            if let Some(p) = &meta.audio_path {
                let _ = std::fs::remove_file(p);
            }
            svc.overlay.emit(OverlayEvent::Idle);
        }
        Err(PipelineError::Transcription(e)) => {
            let record = build_record(&meta, &settings, Delivery::Failed(&e));
            let id = svc.db.lock().unwrap().insert_dictation(&record).ok();
            svc.overlay.emit(failure_feedback(&e, id));
        }
        Ok((out, transcriber, corrector)) => {
            let inserted = deliver(&svc, cap.focus_start, out.final_text.clone(), settings.restore_delay_ms).await;
            let record = build_record(&meta, &settings, Delivery::Done { out: &out, transcriber, corrector, inserted });
            let id = {
                let db = svc.db.lock().unwrap();
                let id = db.insert_dictation(&record).ok();
                let used = prompt::terms_used(&out.final_text, &terms);
                let _ = db.bump_term_usage(&used, &meta.created_at);
                id
            };
            svc.overlay.emit(feedback(inserted, out.correction_error.as_deref(), &out.final_text, id));
        }
    }
    svc.ui.history_changed();
}

/// Re-runs the pipeline on a stored recording (after a failure, or with new settings/glossary).
pub async fn retranscribe(svc: Arc<Services>, id: i64) -> Result<(), String> {
    let settings = svc.settings.read().unwrap().clone();
    let (dictation, terms) = {
        let db = svc.db.lock().unwrap();
        (db.get_dictation(id).map_err(|e| e.to_string())?, db.list_terms().unwrap_or_default())
    };
    let dictation = dictation.ok_or("dictée introuvable")?;
    let path = dictation.audio_path.ok_or("l'audio de cette dictée a été purgé")?;
    let wav = std::fs::read(&path).map_err(|e| format!("lecture audio impossible : {e}"))?;
    let (stt, llm) = svc.build_providers(&settings).map_err(|e| e.to_string())?;
    let out = pipeline::run(&settings.pipeline_config(), &wav, &terms, dictation.app_name.as_deref(), stt.as_ref(), llm.as_ref())
        .await
        .map_err(|e| e.to_string())?;
    // A failed dictation was never delivered: hand the new text over through the clipboard, and
    // only record « Clipboard » when that copy actually happened.
    let outcome = if dictation.outcome == Outcome::Error {
        match svc.clipboard.write_text(&out.final_text) {
            Ok(()) => Outcome::Clipboard,
            Err(e) => {
                tracing::warn!("copie après retranscription impossible : {e}");
                Outcome::Error
            }
        }
    } else {
        dictation.outcome
    };
    let update = TranscriptionUpdate {
        raw_text: Some(out.raw),
        final_text: Some(out.final_text),
        transcriber: Some(stt.name()),
        corrector: (settings.level != Level::Raw).then(|| llm.name()),
        stt_ms: Some(out.stt_ms as i64),
        llm_ms: out.llm_ms.map(|v| v as i64),
        outcome,
        error: out.correction_error,
    };
    svc.db.lock().unwrap().update_transcription(id, &update).map_err(|e| e.to_string())?;
    svc.ui.history_changed();
    Ok(())
}

pub fn purge_audio(svc: &Services) {
    let days = svc.settings.read().unwrap().audio_retention_days;
    purge_audio_in(&svc.db.lock().unwrap(), days, chrono::Utc::now());
}

/// Deletes the recordings older than the retention and forgets their path. A file that is already
/// gone is forgotten too; a file that cannot be removed (locked, access denied) keeps its path, so
/// it is retried at the next purge instead of being orphaned on disk. Returns the ids purged.
pub fn purge_audio_in(db: &Db, days: u32, now: chrono::DateTime<chrono::Utc>) -> Vec<i64> {
    let Some(cutoff) = purge_cutoff(now, days) else {
        return Vec::new();
    };
    let mut purged = Vec::new();
    for (id, path) in db.audio_to_purge(&cutoff).unwrap_or_default() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != ErrorKind::NotFound => tracing::warn!("purge audio impossible ({path}) : {e}"),
            _ => {
                if db.clear_audio_path(id).is_ok() {
                    purged.push(id);
                }
            }
        }
    }
    purged
}

/// RFC 3339 cutoff for the audio purge, or None when nothing must be purged (0 = keep forever).
/// Never panics: a retention too large for date arithmetic means « keep everything ».
fn purge_cutoff(now: chrono::DateTime<chrono::Utc>, days: u32) -> Option<String> {
    if days == 0 {
        return None;
    }
    let cutoff = now.checked_sub_signed(chrono::TimeDelta::try_days(days as i64)?)?;
    Some(cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

#[cfg(test)]
mod tests;
