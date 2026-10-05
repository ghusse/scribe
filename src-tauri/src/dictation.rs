use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;

use scribe_core::audio::{self, AudioClip};
use scribe_core::focus::{self, FocusSnapshot};
use scribe_core::gesture::Mode;
use scribe_core::insert::{self, InsertResult};
use scribe_core::model::{Level, NewDictation, Outcome, TranscriptionUpdate};
use scribe_core::pipeline::{self, Corrector, PipelineError, ProviderError, Transcriber};
use scribe_core::prompt::{self, CorrectionPrompt};
use scribe_providers::anthropic::AnthropicCorrector;
use scribe_providers::catalog::{self, LlmApi};
use scribe_providers::openai_chat::OpenAiChatCorrector;
use scribe_providers::openai_compat::OpenAiCompatTranscriber;

use crate::overlay::{self, ToastLevel};
use crate::secrets;
use crate::services::{now_rfc3339, Services};
use crate::settings::Settings;

pub const FOCUS_TIMEOUT_MS: u64 = 300;
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Captured {
    pub clip: AudioClip,
    pub mode: Mode,
    pub focus_start: FocusSnapshot,
}

/// Stands in for the LLM when there is none (Raw level) or it cannot be built (missing key):
/// the pipeline then falls back to the raw text instead of failing the dictation.
struct NoCorrector(String);

#[async_trait::async_trait]
impl Corrector for NoCorrector {
    fn name(&self) -> String {
        "aucun".into()
    }
    async fn correct(&self, _p: &CorrectionPrompt) -> Result<String, ProviderError> {
        Err(ProviderError::Config(self.0.clone()))
    }
}

fn build_corrector(s: &Settings, llm_key: Option<String>) -> Box<dyn Corrector> {
    if s.level == Level::Raw {
        return Box::new(NoCorrector("correcteur désactivé".into()));
    }
    let Some(provider) = catalog::llm_provider(&s.llm_provider) else {
        return Box::new(NoCorrector(format!("fournisseur de correction inconnu : {}", s.llm_provider)));
    };
    let Some(key) = llm_key else {
        return Box::new(NoCorrector(format!("clé API {} manquante", provider.label)));
    };
    let effort = catalog::effort_levels(provider, &s.llm_model)
        .contains(&s.llm_effort.as_str())
        .then(|| s.llm_effort.clone());
    let built: Result<Box<dyn Corrector>, ProviderError> = match provider.llm_api {
        Some(LlmApi::Anthropic) => AnthropicCorrector::new(provider.base_url, key, &s.llm_model, effort, HTTP_TIMEOUT)
            .map(|c| Box::new(c) as Box<dyn Corrector>),
        _ => OpenAiChatCorrector::new(provider.base_url, key, &s.llm_model, effort, HTTP_TIMEOUT)
            .map(|c| Box::new(c) as Box<dyn Corrector>),
    };
    built.unwrap_or_else(|e| Box::new(NoCorrector(e.to_string())))
}

pub fn build_providers(s: &Settings) -> Result<(Box<dyn Transcriber>, Box<dyn Corrector>), ProviderError> {
    let provider = catalog::stt_provider(&s.stt_provider)
        .ok_or_else(|| ProviderError::Config(format!("fournisseur de transcription inconnu : {}", s.stt_provider)))?;
    let stt_key = secrets::get_key(provider.id)
        .ok_or_else(|| ProviderError::Config(format!("clé API manquante pour « {} »", provider.label)))?;
    let stt = OpenAiCompatTranscriber::new(provider.base_url, stt_key, &s.stt_model, HTTP_TIMEOUT)?;
    let llm_key = if s.level == Level::Raw { None } else { secrets::get_key(&s.llm_provider) };
    Ok((Box::new(stt), build_corrector(s, llm_key)))
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

pub async fn process(svc: Arc<Services>, cap: Captured) {
    let settings = svc.settings.read().unwrap().clone();
    let duration = audio::duration_ms(&cap.clip);
    if duration < settings.min_recording_ms || audio::is_silent(&cap.clip, settings.silence_threshold_dbfs) {
        overlay::emit(&svc.app, overlay::OverlayEvent::Idle);
        return;
    }
    let wav = match audio::encode_wav(&cap.clip) {
        Ok(w) => w,
        Err(e) => {
            overlay::toast(&svc.app, ToastLevel::Error, format!("Encodage audio impossible : {e}"), None, None);
            return;
        }
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
    let app_name = cap.focus_start.app_name.clone();
    let result = match build_providers(&settings) {
        Ok((stt, llm)) => {
            let r = pipeline::run(&settings.pipeline_config(), &wav, &terms, app_name.as_deref(), stt.as_ref(), llm.as_ref()).await;
            r.map(|out| (out, stt.name(), llm.name()))
        }
        Err(e) => Err(PipelineError::Transcription(e)),
    };

    let mut record = NewDictation {
        created_at: created_at.clone(),
        mode: cap.mode,
        app_name,
        app_bundle_id: None,
        audio_path: audio_path.clone(),
        duration_ms: duration as i64,
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

    match result {
        Err(PipelineError::Empty) => {
            if let Some(p) = &audio_path {
                let _ = std::fs::remove_file(p);
            }
            overlay::emit(&svc.app, overlay::OverlayEvent::Idle);
        }
        Err(PipelineError::Transcription(e)) => {
            record.error = Some(e.to_string());
            let id = svc.db.lock().unwrap().insert_dictation(&record).ok();
            overlay::toast(
                &svc.app,
                ToastLevel::Error,
                format!("Échec de la transcription : {e}. Réessayez depuis l'historique."),
                None,
                id,
            );
        }
        Ok((out, stt_name, llm_name)) => {
            let text = out.final_text.clone();
            let svc2 = svc.clone();
            let start = cap.focus_start.clone();
            let delay = settings.restore_delay_ms;
            let inserted = tauri::async_runtime::spawn_blocking(move || {
                let end = focus::snapshot_with_timeout(svc2.focus.clone(), FOCUS_TIMEOUT_MS);
                let plan = insert::decide(&start, &end);
                insert::perform(plan, &text, svc2.clipboard.as_ref(), svc2.keys.as_ref(), delay, &|ms| {
                    std::thread::sleep(Duration::from_millis(ms))
                })
            })
            .await
            .unwrap_or(InsertResult::ClipboardFailed);

            record.raw_text = Some(out.raw.clone());
            record.final_text = Some(out.final_text.clone());
            record.transcriber = Some(stt_name);
            record.corrector = (settings.level != Level::Raw).then_some(llm_name);
            record.stt_ms = Some(out.stt_ms as i64);
            record.llm_ms = out.llm_ms.map(|v| v as i64);
            record.outcome = inserted.outcome();
            record.error = out.correction_error.clone();
            let id = {
                let db = svc.db.lock().unwrap();
                let id = db.insert_dictation(&record).ok();
                let used = prompt::terms_used(&out.final_text, &terms);
                let _ = db.bump_term_usage(&used, &created_at);
                id
            };
            let p = Some(preview(&out.final_text));
            let err = out.correction_error.as_deref();
            match inserted {
                InsertResult::Pasted => match err {
                    Some(err) => overlay::toast(&svc.app, ToastLevel::Info, format!("Inséré sans correction ({err})"), None, id),
                    None => overlay::emit(&svc.app, overlay::OverlayEvent::Idle),
                },
                InsertResult::PastedUncertain => {
                    overlay::toast(&svc.app, ToastLevel::Uncertain, with_correction_note("Texte inséré ?", err), p, id)
                }
                InsertResult::ClipboardOnly | InsertResult::PasteFailed => overlay::toast(
                    &svc.app,
                    ToastLevel::Copied,
                    with_correction_note("Texte copié dans le presse-papier", err),
                    p,
                    id,
                ),
                InsertResult::ClipboardFailed => overlay::toast(
                    &svc.app,
                    ToastLevel::Error,
                    with_correction_note("Presse-papier indisponible : texte dans l'historique", err),
                    p,
                    id,
                ),
            }
        }
    }
    let _ = svc.app.emit_to("main", "history-changed", ());
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
    let (stt, llm) = build_providers(&settings).map_err(|e| e.to_string())?;
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
    let _ = svc.app.emit_to("main", "history-changed", ());
    Ok(())
}

pub fn purge_audio(svc: &Services) {
    let days = svc.settings.read().unwrap().audio_retention_days;
    let Some(cutoff) = purge_cutoff(chrono::Utc::now(), days) else {
        return;
    };
    let db = svc.db.lock().unwrap();
    for (id, path) in db.audio_to_purge(&cutoff).unwrap_or_default() {
        let _ = std::fs::remove_file(&path);
        let _ = db.clear_audio_path(id);
    }
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
mod tests {
    use super::*;

    #[test]
    fn purge_cutoff_never_panics_on_huge_retention() {
        let now = chrono::Utc::now();
        assert_eq!(purge_cutoff(now, 0), None);
        assert_eq!(purge_cutoff(now, 100_000_000), None);
        assert_eq!(purge_cutoff(now, u32::MAX), None);
        let t: chrono::DateTime<chrono::Utc> = "2026-10-05T12:00:00Z".parse().unwrap();
        assert_eq!(purge_cutoff(t, 30).as_deref(), Some("2026-09-05T12:00:00.000Z"));
    }

    #[test]
    fn missing_llm_key_falls_back_to_raw_text_instead_of_failing() {
        let s = Settings { level: Level::Formatted, ..Default::default() };
        let c = build_corrector(&s, None);
        let p = prompt::build_correction_prompt("bonjour", &[], None, Level::Formatted);
        let err = tauri::async_runtime::block_on(c.correct(&p)).unwrap_err();
        assert!(err.to_string().contains("Anthropic"), "{err}");
    }

    #[test]
    fn correction_note_is_appended_to_every_toast() {
        assert_eq!(with_correction_note("Texte inséré ?", None), "Texte inséré ?");
        assert_eq!(with_correction_note("Texte inséré ?", Some("délai")), "Texte inséré ? (non corrigé : délai)");
    }
}
