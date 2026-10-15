use std::io::ErrorKind;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use scribe_core::audio::{self, AudioClip};
use scribe_core::focus::{self, FocusSnapshot};
use scribe_core::gesture::Mode;
use scribe_core::insert::{
    self, FieldReader, InsertPlan, InsertResult, PastedText, ReplacePlan, ReplaceRefusal, MAX_SELECT_GRAPHEMES,
};
use scribe_core::model::{CorrectionUpdate, NewDictation, Outcome, TranscriptionUpdate};
use scribe_core::pipeline::{self, Correction, PipelineError, PipelineOutput, ProviderError, StyleInputs};
use scribe_core::prompt;
use scribe_core::storage::Db;
use scribe_core::style::{self, Style};

use crate::error::AppError;
use crate::overlay::{Notice, OverlayEvent, StyleBadge, ToastLevel};
use crate::services::{now_rfc3339, Services};
use crate::settings::Settings;

pub const FOCUS_TIMEOUT_MS: u64 = 300;
/// Same bound for each read of the focused field when checking a paste (a hung app then counts as unreadable).
pub const FIELD_READ_TIMEOUT_MS: u64 = 300;

pub struct Captured {
    pub clip: AudioClip,
    pub mode: Mode,
    pub focus_start: FocusSnapshot,
}

fn preview(text: &str) -> String {
    let p: String = text.chars().take(140).collect();
    if p.len() < text.len() {
        format!("{p}…")
    } else {
        p
    }
}

/// What the overlay shows once a transcribed dictation was delivered (or not). Spec §7: a correction failure is
/// always mentioned, whatever the insertion result. `style`: the badge of the keyword style that ran, whose clean
/// paste also gets a toast.
pub fn feedback(
    inserted: InsertResult,
    correction_error: Option<&ProviderError>,
    final_text: &str,
    id: Option<i64>,
    style: Option<StyleBadge>,
) -> OverlayEvent {
    let toast = |level, message, preview: Option<String>| OverlayEvent::Toast {
        level,
        message,
        preview,
        dictation_id: id,
        style: style.clone(),
    };
    let p = Some(preview(final_text));
    let correction = correction_error.cloned();
    match inserted {
        InsertResult::Pasted => match correction {
            Some(correction) => toast(ToastLevel::Info, Notice::PastedWithoutCorrection { correction }, None),
            None if style.is_some() => toast(ToastLevel::Info, Notice::Pasted, None),
            None => OverlayEvent::Idle,
        },
        InsertResult::PastedUncertain => toast(ToastLevel::Uncertain, Notice::PastedUncertain { correction }, p),
        InsertResult::ClipboardOnly | InsertResult::PasteFailed => {
            toast(ToastLevel::Copied, Notice::Copied { correction }, p)
        }
        InsertResult::NotPasted => toast(ToastLevel::Copied, Notice::NotPasted { correction }, p),
        InsertResult::ClipboardFailed => toast(ToastLevel::Error, Notice::ClipboardUnavailable { correction }, p),
    }
}

/// What the overlay shows when the dictation could not be transcribed (saved for a retry).
pub fn failure_feedback(e: &ProviderError, id: Option<i64>) -> OverlayEvent {
    OverlayEvent::Toast {
        level: ToastLevel::Error,
        message: Notice::TranscriptionFailed { error: e.clone() },
        preview: None,
        dictation_id: id,
        style: None,
    }
}

/// `dictations.error`: the JSON of the error, which the history translates.
pub fn stored_error(e: &ProviderError) -> String {
    serde_json::to_string(e).expect("serialize ProviderError")
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
    Done { out: &'a PipelineOutput, transcriber: String, corrector: Option<String>, inserted: InsertResult },
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
        style: None,
        body_start: None,
        transcriber: Some(settings.stt_model.clone()),
        corrector: None,
        stt_ms: None,
        llm_ms: None,
        outcome: Outcome::Error,
        error: None,
    };
    match delivery {
        Delivery::Failed(e) => record.error = Some(stored_error(e)),
        Delivery::Done { out, transcriber, corrector, inserted } => {
            record.raw_text = Some(out.raw.clone());
            record.final_text = Some(out.final_text.clone());
            record.style = out.style.clone();
            record.body_start = out.body_start.map(|v| v as i64);
            record.transcriber = Some(transcriber);
            // Correction off and short dictations never reach the corrector (`pipeline::run`).
            record.corrector = out.llm_ms.and(corrector);
            record.stt_ms = Some(out.stt_ms as i64);
            record.llm_ms = out.llm_ms.map(|v| v as i64);
            record.outcome = inserted.outcome();
            record.error = out.correction_error.as_ref().map(stored_error);
        }
    }
    record
}

/// Pastes (or copies) the text, checking that the focus did not move since the recording started. Also returns the
/// focus it pasted into.
async fn deliver(
    svc: &Arc<Services>,
    start: FocusSnapshot,
    text: String,
    restore_delay_ms: u64,
) -> (InsertResult, FocusSnapshot) {
    let svc = svc.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _insert = svc.insert_lock.lock().unwrap();
        let end = focus::snapshot_with_timeout(svc.focus.clone(), FOCUS_TIMEOUT_MS);
        let plan = insert::decide(&start, &end);
        let field = insert::TimedFieldReader::new(svc.field.clone(), FIELD_READ_TIMEOUT_MS);
        let inserted =
            insert::perform(plan, &text, svc.clipboard.as_ref(), svc.keys.as_ref(), &field, restore_delay_ms, &sleep);
        (inserted, end)
    })
    .await
    .unwrap_or((InsertResult::ClipboardFailed, FocusSnapshot::unknown()))
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms))
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
        Err(detail) => return svc.overlay.toast(ToastLevel::Error, Notice::AudioEncodingFailed { detail }, None, None),
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
    let meta = RecordMeta {
        created_at,
        mode: cap.mode,
        app_name: cap.focus_start.app_name.clone(),
        audio_path,
        duration_ms: duration,
    };
    let language = svc.language();
    let inputs = StyleInputs { styles: &settings.styles, language, terms: &terms, app_name: meta.app_name.as_deref() };
    // Authoritative: replaces the badge peeked at while recording.
    let on_detected = |s: Option<&Style>| {
        svc.overlay.emit(OverlayEvent::Processing { style: s.map(|s| StyleBadge::of(s, language)) });
    };
    let result = match svc.build_providers(&settings) {
        Ok((stt, llm)) => {
            pipeline::run(&settings.pipeline_config(), &wav, &inputs, &on_detected, stt.as_ref(), llm.as_ref())
                .await
                .map(|out| (out, stt.name(), llm.name()))
        }
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
            let (inserted, end) =
                deliver(&svc, cap.focus_start, out.final_text.clone(), settings.restore_delay_ms).await;
            let record = build_record(&meta, &settings, Delivery::Done { out: &out, transcriber, corrector, inserted });
            let id = {
                let db = svc.db.lock().unwrap();
                let id = db.insert_dictation(&record).ok();
                let used = prompt::terms_used(&out.final_text, &terms);
                let _ = db.bump_term_usage(&used, &meta.created_at);
                id
            };
            if let (Some(dictation_id), InsertResult::Pasted | InsertResult::PastedUncertain) = (id, inserted) {
                let pasted = PastedText { dictation_id, focus: end, text: out.final_text.clone() };
                *svc.last_paste.lock().unwrap() = Some(pasted);
            }
            let badge = StyleBadge::keyword(&settings.styles, out.style.as_deref(), language);
            svc.overlay.emit(feedback(inserted, out.correction_error.as_ref(), &out.final_text, id, badge));
        }
    }
    svc.ui.history_changed();
}

/// Transcribes the start of a recording to peek at its style, with the style keywords as hints. None on any
/// failure: the final transcription decides anyway.
pub async fn transcribe_prefix(svc: &Services, clip: AudioClip) -> Option<String> {
    let settings = svc.settings.read().unwrap().clone();
    let wav = audio::encode_wav(&clip).ok()?;
    let cfg = settings.pipeline_config();
    let hints = prompt::transcriber_hints(&style::keywords(&settings.styles), &[], cfg.hint_budget_chars);
    let transcribed = match svc.build_providers(&settings) {
        Ok((stt, _)) => pipeline::transcribe(&cfg, &wav, &hints, stt.as_ref()).await,
        Err(e) => Err(PipelineError::Transcription(e)),
    };
    transcribed.map(|(text, _)| text).inspect_err(|e| tracing::debug!("peek transcription failed: {e}")).ok()
}

/// Re-runs the pipeline on a stored recording (after a failure, or with new settings/glossary).
pub async fn retranscribe(svc: Arc<Services>, id: i64) -> Result<(), AppError> {
    let settings = svc.settings.read().unwrap().clone();
    let (dictation, terms) = {
        let db = svc.db.lock().unwrap();
        (db.get_dictation(id)?, db.list_terms().unwrap_or_default())
    };
    let dictation = dictation.ok_or(AppError::DictationNotFound)?;
    let path = dictation.audio_path.ok_or(AppError::AudioPurged)?;
    let wav = std::fs::read(&path).map_err(|e| AppError::system(format!("cannot read the audio: {e}")))?;
    let (stt, llm) = svc.build_providers(&settings)?;
    let inputs = StyleInputs {
        styles: &settings.styles,
        language: svc.language(),
        terms: &terms,
        app_name: dictation.app_name.as_deref(),
    };
    let out = pipeline::run(&settings.pipeline_config(), &wav, &inputs, &|_| {}, stt.as_ref(), llm.as_ref()).await?;
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
        style: out.style,
        body_start: out.body_start.map(|v| v as i64),
        transcriber: Some(stt.name()),
        corrector: out.llm_ms.and_then(|_| llm.name()),
        stt_ms: Some(out.stt_ms as i64),
        llm_ms: out.llm_ms.map(|v| v as i64),
        outcome,
        error: out.correction_error.as_ref().map(stored_error),
    };
    svc.db.lock().unwrap().update_transcription(id, &update)?;
    svc.ui.history_changed();
    Ok(())
}

/// What a style switch rewrites: the whole transcript for the default style, so a keyword said by mistake comes
/// back; otherwise the body after the keyword detected when the dictation was made (even if renamed since).
pub fn restyle_source<'a>(raw: &'a str, body_start: Option<i64>, style: &Style) -> &'a str {
    if style.id == style::DEFAULT_STYLE_ID {
        return raw;
    }
    body_start.and_then(|b| usize::try_from(b).ok()).and_then(|b| raw.get(b..)).map_or(raw, str::trim_end)
}

/// A dictation rewritten with another style, waiting to be applied.
struct Rewrite {
    id: i64,
    /// Its number in `Services::restyle_seq`.
    seq: u64,
    style: Style,
    correction: Correction,
    corrector: Option<String>,
    /// The row's outcome before the switch, kept when the text already reads as the new one.
    outcome: Outcome,
}

/// Rewrites a dictation with another style, then replaces the pasted text in place when that is safe, else copies
/// the new text. When several switches overlap, only the last one asked is applied.
pub async fn restyle(svc: Arc<Services>, id: i64, style_id: &str) -> Result<(), AppError> {
    let rewrite = rewrite(&svc, id, style_id).await?;
    apply_rewrite(&svc, rewrite).await
}

async fn rewrite(svc: &Services, id: i64, style_id: &str) -> Result<Rewrite, AppError> {
    let settings = svc.settings.read().unwrap().clone();
    let style = style::find(&settings.styles, style_id).ok_or(AppError::StyleNotFound)?.clone();
    let (dictation, terms) = {
        let db = svc.db.lock().unwrap();
        (db.get_dictation(id)?, db.list_terms().unwrap_or_default())
    };
    let dictation = dictation.ok_or(AppError::DictationNotFound)?;
    let raw = dictation.raw_text.as_deref().ok_or(AppError::NothingToRestyle)?;
    let seq = svc.restyle_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let inputs = StyleInputs {
        styles: &settings.styles,
        language: svc.language(),
        terms: &terms,
        app_name: dictation.app_name.as_deref(),
    };
    let corrector = svc.build_corrector(&settings);
    let text = restyle_source(raw, dictation.body_start, &style);
    let correction = pipeline::correct(&settings.pipeline_config(), text, &style, &inputs, corrector.as_ref()).await;
    Ok(Rewrite { id, seq, style, correction, corrector: corrector.name(), outcome: dictation.outcome })
}

async fn apply_rewrite(svc: &Arc<Services>, rw: Rewrite) -> Result<(), AppError> {
    let _turn = svc.restyle_lock.lock().await;
    if svc.restyle_seq.load(Ordering::SeqCst) != rw.seq {
        return Ok(());
    }
    let id = rw.id;
    // Any style, the default one included: the toast keeps offering the switch.
    let badge = Some(StyleBadge::of(&rw.style, svc.language()));
    if let Some(error) = rw.correction.error {
        let toast = OverlayEvent::Toast {
            level: ToastLevel::Error,
            message: Notice::RestyleFailed { error },
            preview: None,
            dictation_id: Some(id),
            style: badge,
        };
        svc.overlay.emit_for_dictation(id, toast);
        return Ok(());
    }
    let text = rw.correction.final_text;
    let restore_delay_ms = svc.settings.read().unwrap().restore_delay_ms;
    let delivered = {
        let (svc, text) = (svc.clone(), text.clone());
        tauri::async_runtime::spawn_blocking(move || deliver_rewrite(&svc, id, &text, restore_delay_ms))
            .await
            .unwrap_or(Delivered::Inserted(InsertResult::ClipboardFailed))
    };
    let outcome = match delivered {
        Delivered::Inserted(r) => r.outcome(),
        Delivered::Unchanged | Delivered::HistoryOnly => rw.outcome,
    };
    let update = CorrectionUpdate {
        final_text: Some(text.clone()),
        style: Some(rw.style.id.clone()),
        corrector: rw.corrector,
        llm_ms: rw.correction.llm_ms.map(|v| v as i64),
        outcome,
        error: None,
    };
    svc.db.lock().unwrap().update_correction(id, &update)?;
    if let Some(toast) = rewrite_feedback(delivered, &text, id, badge) {
        svc.overlay.emit_for_dictation(id, toast);
    }
    svc.ui.history_changed();
    Ok(())
}

/// What became of a rewritten text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivered {
    /// The pasted text already reads as the new one.
    Unchanged,
    /// No toast could say it was copied (a newer dictation runs or shows its toast): the clipboard is left alone.
    HistoryOnly,
    Inserted(InsertResult),
}

/// Puts a rewritten text where its dictation went: over the pasted text when `insert::plan_replace` allows it, else
/// in the clipboard.
fn deliver_rewrite(svc: &Services, id: i64, text: &str, restore_delay_ms: u64) -> Delivered {
    let _insert = svc.insert_lock.lock().unwrap();
    let field = insert::TimedFieldReader::new(svc.field.clone(), FIELD_READ_TIMEOUT_MS);
    let last = svc.last_paste.lock().unwrap().clone().filter(|l| l.dictation_id == id);
    let plan = match &last {
        None => ReplacePlan::CopyOnly(ReplaceRefusal::NotLastPaste),
        Some(last) if last.text == text => return Delivered::Unchanged,
        Some(last) => {
            let now = focus::snapshot_with_timeout(svc.focus.clone(), FOCUS_TIMEOUT_MS);
            let idle = !svc.session_busy.load(Ordering::Relaxed);
            insert::plan_replace(last, &now, field.before_caret().as_deref(), idle, MAX_SELECT_GRAPHEMES)
        }
    };
    let (clipboard, keys) = (svc.clipboard.as_ref(), svc.keys.as_ref());
    let inserted = match plan {
        ReplacePlan::Replace { suffix, graphemes } => {
            insert::replace(&suffix, graphemes, text, clipboard, keys, &field, restore_delay_ms, &sleep)
        }
        ReplacePlan::CopyOnly(reason) => {
            if reason == ReplaceRefusal::Busy || !svc.overlay.free_for_dictation(id) {
                tracing::info!("style switch kept in the history only: {reason:?}");
                return Delivered::HistoryOnly;
            }
            tracing::info!("style switch copied instead of replaced: {reason:?}");
            insert::perform(InsertPlan::ClipboardOnly, text, clipboard, keys, &field, restore_delay_ms, &sleep)
        }
    };
    if inserted == InsertResult::Pasted {
        if let Some(last) = svc.last_paste.lock().unwrap().as_mut().filter(|l| l.dictation_id == id) {
            last.text = text.to_string();
        }
    }
    Delivered::Inserted(inserted)
}

/// The toast of an applied style switch, if any.
fn rewrite_feedback(delivered: Delivered, text: &str, id: i64, style: Option<StyleBadge>) -> Option<OverlayEvent> {
    let (level, message, preview) = match delivered {
        Delivered::HistoryOnly => return None,
        Delivered::Unchanged | Delivered::Inserted(InsertResult::Pasted | InsertResult::PastedUncertain) => {
            (ToastLevel::Info, Notice::Restyled, None)
        }
        Delivered::Inserted(InsertResult::ClipboardFailed) => {
            (ToastLevel::Error, Notice::ClipboardUnavailable { correction: None }, Some(preview(text)))
        }
        Delivered::Inserted(_) => (ToastLevel::Copied, Notice::RestyleCopied, Some(preview(text))),
    };
    Some(OverlayEvent::Toast { level, message, preview, dictation_id: Some(id), style })
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
            Err(e) if e.kind() != ErrorKind::NotFound => {
                tracing::warn!("purge audio impossible ({path}) : {e}")
            }
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
