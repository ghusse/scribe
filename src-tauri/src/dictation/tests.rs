use scribe_core::audio::TARGET_RATE;
use scribe_core::focus::FocusState;
use scribe_core::model::{Dictation, TermSource};

use super::*;
use crate::testing::{editable, Fixture, UiCall};

fn speech(ms: u32) -> AudioClip {
    let n = (TARGET_RATE as u64 * ms as u64 / 1000) as usize;
    let samples: Vec<f32> = (0..n).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / TARGET_RATE as f32).sin()).collect();
    AudioClip { samples: audio::to_i16(&samples), sample_rate: TARGET_RATE }
}

fn captured(clip: AudioClip) -> Captured {
    Captured { clip, mode: Mode::Locked, focus_start: editable("Notepad") }
}

fn run(f: &Fixture, clip: AudioClip) {
    tauri::async_runtime::block_on(process(f.svc.clone(), captured(clip)));
}

fn rows(f: &Fixture) -> Vec<Dictation> {
    f.svc.db.lock().unwrap().list_dictations(None, 100, 0).unwrap()
}

fn only_row(f: &Fixture) -> Dictation {
    let rows = rows(f);
    assert_eq!(rows.len(), 1, "{rows:?}");
    rows.into_iter().next().unwrap()
}

fn toast(level: ToastLevel, message: &str, preview: Option<&str>, id: Option<i64>) -> OverlayEvent {
    OverlayEvent::Toast { level, message: message.into(), preview: preview.map(String::from), dictation_id: id }
}

fn set_end_focus(f: &Fixture, state: FocusState) {
    f.focus.0.lock().unwrap().state = state;
}

// --- pure helpers -------------------------------------------------------------------------------

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
fn correction_note_is_appended_to_every_toast() {
    assert_eq!(with_correction_note("Texte inséré ?", None), "Texte inséré ?");
    assert_eq!(with_correction_note("Texte inséré ?", Some("délai")), "Texte inséré ? (non corrigé : délai)");
}

#[test]
fn preview_is_cut_at_140_characters() {
    assert_eq!(preview("court"), "court");
    let exact = "é".repeat(140);
    assert_eq!(preview(&exact), exact);
    let long = "é".repeat(141);
    assert_eq!(preview(&long), format!("{}…", "é".repeat(140)));
}

#[test]
fn feedback_for_each_insertion_result() {
    let id = Some(3);
    assert_eq!(feedback(InsertResult::Pasted, None, "t", id), OverlayEvent::Idle);
    assert_eq!(
        feedback(InsertResult::Pasted, Some("délai"), "t", id),
        toast(ToastLevel::Info, "Inséré sans correction (délai)", None, id)
    );
    assert_eq!(feedback(InsertResult::PastedUncertain, None, "t", id), toast(ToastLevel::Uncertain, "Texte inséré ?", Some("t"), id));
    assert_eq!(
        feedback(InsertResult::PastedUncertain, Some("délai"), "t", id),
        toast(ToastLevel::Uncertain, "Texte inséré ? (non corrigé : délai)", Some("t"), id)
    );
    for r in [InsertResult::ClipboardOnly, InsertResult::PasteFailed] {
        assert_eq!(feedback(r, None, "t", None), toast(ToastLevel::Copied, "Texte copié dans le presse-papier", Some("t"), None));
    }
    assert_eq!(
        feedback(InsertResult::ClipboardFailed, Some("x"), "t", id),
        toast(ToastLevel::Error, "Presse-papier indisponible : texte dans l'historique (non corrigé : x)", Some("t"), id)
    );
}

#[test]
fn failure_feedback_points_to_the_history() {
    assert_eq!(
        failure_feedback(&ProviderError::Auth, Some(9)),
        toast(ToastLevel::Error, "Échec de la transcription : clé API refusée. Réessayez depuis l'historique.", None, Some(9))
    );
}

fn meta() -> RecordMeta {
    RecordMeta { created_at: "2026-10-05T10:00:00.000Z".into(), mode: Mode::Hold, app_name: Some("Word".into()), audio_path: Some("a.wav".into()), duration_ms: 1_234 }
}

#[test]
fn build_record_for_a_failed_transcription() {
    let s = Settings::default();
    let r = build_record(&meta(), &s, Delivery::Failed(&ProviderError::Timeout));
    assert_eq!(r.outcome, Outcome::Error);
    assert_eq!(r.error.as_deref(), Some("délai dépassé"));
    assert_eq!((r.raw_text, r.final_text, r.corrector), (None, None, None));
    assert_eq!(r.transcriber.as_deref(), Some("gpt-transcribe"), "the configured model");
    assert_eq!((r.created_at.as_str(), r.mode, r.app_name.as_deref(), r.audio_path.as_deref(), r.duration_ms), ("2026-10-05T10:00:00.000Z", Mode::Hold, Some("Word"), Some("a.wav"), 1_234));
    assert_eq!(r.level, Level::Formatted);
}

#[test]
fn build_record_for_a_delivered_dictation() {
    let out = PipelineOutput { raw: "r".into(), final_text: "F".into(), stt_ms: 10, llm_ms: Some(20), correction_error: Some("e".into()) };
    let s = Settings::default();
    let done = |inserted| Delivery::Done { out: &out, transcriber: "stt".into(), corrector: "llm".into(), inserted };
    let r = build_record(&meta(), &s, done(InsertResult::PastedUncertain));
    assert_eq!((r.raw_text.as_deref(), r.final_text.as_deref()), (Some("r"), Some("F")));
    assert_eq!((r.transcriber.as_deref(), r.corrector.as_deref()), (Some("stt"), Some("llm")));
    assert_eq!((r.stt_ms, r.llm_ms), (Some(10), Some(20)));
    assert_eq!((r.outcome, r.error.as_deref()), (Outcome::PastedUncertain, Some("e")));
    let raw = Settings { level: Level::Raw, ..Default::default() };
    let r = build_record(&meta(), &raw, done(InsertResult::ClipboardFailed));
    assert_eq!(r.corrector, None, "no corrector at the raw level");
    assert_eq!(r.outcome, Outcome::Error);
}

// --- process ------------------------------------------------------------------------------------

#[test]
fn a_too_short_or_silent_recording_is_dropped() {
    let f = Fixture::new();
    run(&f, speech(100));
    let silent = AudioClip { samples: vec![0; TARGET_RATE as usize], sample_rate: TARGET_RATE };
    run(&f, silent);
    assert!(rows(&f).is_empty());
    assert!(f.wav_files().is_empty());
    assert_eq!(f.window.events(), vec![OverlayEvent::Idle, OverlayEvent::Idle]);
    assert!(f.ui.calls().is_empty(), "nothing new in the history");
}

#[test]
fn no_speech_deletes_the_recording_without_a_row() {
    let f = Fixture::new();
    f.plan(|p| p.stt = Ok("  Merci d'avoir regardé. ".into()));
    run(&f, speech(1_000));
    assert!(rows(&f).is_empty());
    assert!(f.wav_files().is_empty(), "the .wav is removed");
    assert_eq!(f.window.last_event(), Some(OverlayEvent::Idle));
    assert_eq!(f.ui.calls(), vec![UiCall::HistoryChanged]);
}

#[test]
fn provider_setup_failure_keeps_the_audio_for_a_retry() {
    let f = Fixture::new();
    f.plan(|p| p.build = Err(ProviderError::Config("clé API manquante pour « OpenAI »".into())));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::Error);
    assert_eq!(row.error.as_deref(), Some("configuration : clé API manquante pour « OpenAI »"));
    assert_eq!(row.raw_text, None);
    let audio = row.audio_path.clone().expect("audio kept");
    assert!(std::path::Path::new(&audio).exists());
    assert_eq!(f.wav_files().len(), 1);
    assert_eq!(f.window.last_event(), Some(failure_feedback(&ProviderError::Config("clé API manquante pour « OpenAI »".into()), Some(row.id))));
    assert_eq!(f.ui.calls(), vec![UiCall::HistoryChanged]);
}

#[test]
fn transcription_failure_is_saved_without_text() {
    let f = Fixture::new();
    f.plan(|p| p.stt = Err(ProviderError::Auth));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.outcome, row.error.as_deref(), row.raw_text.as_deref()), (Outcome::Error, Some("clé API refusée"), None));
    assert!(row.audio_path.is_some());
    assert_eq!(row.mode, "locked");
    assert_eq!(row.app_name.as_deref(), Some("Notepad"));
    assert_eq!(f.window.last_event(), Some(failure_feedback(&ProviderError::Auth, Some(row.id))));
    assert_eq!(f.ui.count(&UiCall::HistoryChanged), 1);
}

#[test]
fn pasted_dictation_goes_idle_and_is_recorded() {
    let f = Fixture::new();
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::Pasted);
    assert_eq!((row.raw_text.as_deref(), row.final_text.as_deref()), (Some("bonjour scribe"), Some("Bonjour Scribe.")));
    assert_eq!((row.transcriber.as_deref(), row.corrector.as_deref()), (Some("fake-stt"), Some("fake-llm")));
    assert!(row.llm_ms.is_some() && row.stt_ms.is_some());
    assert_eq!(row.error, None);
    assert!((900..=1_100).contains(&row.duration_ms), "{}", row.duration_ms);
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert_eq!(f.window.last_event(), Some(OverlayEvent::Idle));
    assert!(!f.window.visible());
    assert_eq!(f.ui.calls(), vec![UiCall::HistoryChanged]);
}

#[test]
fn paste_restores_the_previous_clipboard() {
    let f = Fixture::new();
    *f.clipboard.content.lock().unwrap() = Some("ancien contenu".into());
    run(&f, speech(1_000));
    assert_eq!(only_row(&f).outcome, Outcome::Pasted);
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert_eq!(f.clipboard.text().as_deref(), Some("ancien contenu"));
}

#[test]
fn pasted_without_correction_says_so() {
    let f = Fixture::new();
    f.plan(|p| p.llm = Err(ProviderError::Auth));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Pasted, Some("bonjour scribe")));
    assert_eq!(row.error.as_deref(), Some("clé API refusée"));
    assert_eq!(f.window.last_event(), Some(toast(ToastLevel::Info, "Inséré sans correction (clé API refusée)", None, Some(row.id))));
}

#[test]
fn unknown_focus_pastes_with_a_doubt() {
    let f = Fixture::new();
    set_end_focus(&f, FocusState::Unknown);
    f.plan(|p| p.llm = Err(ProviderError::Refusal));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::PastedUncertain);
    assert_eq!(
        f.window.last_event(),
        Some(toast(ToastLevel::Uncertain, "Texte inséré ? (non corrigé : le modèle a refusé la requête)", Some("bonjour scribe"), Some(row.id)))
    );
}

#[test]
fn not_editable_focus_or_failed_paste_copies_the_text() {
    for setup in [0, 1] {
        let f = Fixture::new();
        if setup == 0 {
            set_end_focus(&f, FocusState::NotEditable);
        } else {
            *f.keys.fail.lock().unwrap() = true;
        }
        run(&f, speech(1_000));
        let row = only_row(&f);
        assert_eq!(row.outcome, Outcome::Clipboard);
        assert_eq!(f.clipboard.text().as_deref(), Some("Bonjour Scribe."));
        assert_eq!(
            f.window.last_event(),
            Some(toast(ToastLevel::Copied, "Texte copié dans le presse-papier", Some("Bonjour Scribe."), Some(row.id)))
        );
    }
}

#[test]
fn unavailable_clipboard_keeps_the_text_in_the_history() {
    let f = Fixture::new();
    *f.clipboard.fail.lock().unwrap() = true;
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Error, Some("Bonjour Scribe.")));
    assert_eq!(
        f.window.last_event(),
        Some(toast(ToastLevel::Error, "Presse-papier indisponible : texte dans l'historique", Some("Bonjour Scribe."), Some(row.id)))
    );
}

#[test]
fn raw_level_records_no_corrector() {
    let f = Fixture::with_settings(Settings { level: Level::Raw, restore_delay_ms: 0, ..Default::default() });
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.final_text.as_deref(), row.corrector, row.llm_ms), (Some("bonjour scribe"), None, None));
    assert_eq!(row.level, Level::Raw);
}

#[test]
fn glossary_terms_found_in_the_final_text_are_counted() {
    let f = Fixture::new();
    {
        let db = f.svc.db.lock().unwrap();
        db.add_term("Scribe", &[], None, TermSource::Manual, "2026-01-01T00:00:00.000Z").unwrap();
        db.add_term("Tauri", &[], None, TermSource::Manual, "2026-01-01T00:00:00.000Z").unwrap();
    }
    run(&f, speech(1_000));
    let row = only_row(&f);
    let terms = f.svc.db.lock().unwrap().list_terms().unwrap();
    let scribe = terms.iter().find(|t| t.term == "Scribe").unwrap();
    let tauri = terms.iter().find(|t| t.term == "Tauri").unwrap();
    assert_eq!((scribe.use_count, scribe.last_used_at.as_deref()), (1, Some(row.created_at.as_str())));
    assert_eq!((tauri.use_count, tauri.last_used_at.as_deref()), (0, None));
}

#[test]
fn unwritable_audio_does_not_stop_the_dictation() {
    let f = Fixture::new();
    std::fs::remove_dir_all(&f.svc.paths.audio_dir).unwrap();
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.audio_path, None);
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Pasted, Some("Bonjour Scribe.")));
}

// --- retranscribe -------------------------------------------------------------------------------

/// A stored dictation with a real .wav, as process() leaves it.
fn stored(f: &Fixture, outcome: Outcome, with_audio: bool) -> i64 {
    let path = f.svc.paths.audio_dir.join("old.wav");
    std::fs::write(&path, audio::encode_wav(&speech(500)).unwrap()).unwrap();
    let mut r = build_record(&meta(), &Settings::default(), Delivery::Failed(&ProviderError::Auth));
    r.outcome = outcome;
    r.audio_path = with_audio.then(|| path.to_string_lossy().into_owned());
    f.svc.db.lock().unwrap().insert_dictation(&r).unwrap()
}

fn retry(f: &Fixture, id: i64) -> Result<(), String> {
    tauri::async_runtime::block_on(retranscribe(f.svc.clone(), id))
}

fn get(f: &Fixture, id: i64) -> Dictation {
    f.svc.db.lock().unwrap().get_dictation(id).unwrap().unwrap()
}

#[test]
fn retranscribing_a_failed_dictation_copies_the_new_text() {
    let f = Fixture::new();
    let id = stored(&f, Outcome::Error, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!((d.raw_text.as_deref(), d.final_text.as_deref()), (Some("bonjour scribe"), Some("Bonjour Scribe.")));
    assert_eq!((d.transcriber.as_deref(), d.corrector.as_deref()), (Some("fake-stt"), Some("fake-llm")));
    assert_eq!((d.outcome, d.error), (Outcome::Clipboard, None));
    assert_eq!(f.clipboard.text().as_deref(), Some("Bonjour Scribe."));
    assert_eq!(f.ui.calls(), vec![UiCall::HistoryChanged]);
}

#[test]
fn retranscribe_keeps_the_error_outcome_when_the_copy_fails() {
    let f = Fixture::new();
    *f.clipboard.fail.lock().unwrap() = true;
    let id = stored(&f, Outcome::Error, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!((d.outcome, d.final_text.as_deref()), (Outcome::Error, Some("Bonjour Scribe.")));
}

#[test]
fn retranscribing_a_delivered_dictation_keeps_its_outcome_and_clipboard() {
    let f = Fixture::with_settings(Settings { level: Level::Raw, ..Default::default() });
    f.plan(|p| p.stt = Ok("nouveau texte".into()));
    let id = stored(&f, Outcome::PastedUncertain, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!((d.outcome, d.final_text.as_deref(), d.corrector), (Outcome::PastedUncertain, Some("nouveau texte"), None));
    assert_eq!(f.clipboard.text(), None);
}

#[test]
fn retranscribe_records_a_correction_failure() {
    let f = Fixture::new();
    f.plan(|p| p.llm = Ok("pas de balise".into()));
    let id = stored(&f, Outcome::Pasted, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!(d.final_text.as_deref(), Some("bonjour scribe"));
    assert_eq!(d.error.as_deref(), Some("réponse du correcteur sans balise <output>"));
}

#[test]
fn retranscribe_errors_leave_the_row_untouched() {
    let f = Fixture::new();
    assert_eq!(retry(&f, 404).unwrap_err(), "dictée introuvable");

    let purged = stored(&f, Outcome::Error, false);
    assert_eq!(retry(&f, purged).unwrap_err(), "l'audio de cette dictée a été purgé");

    let id = stored(&f, Outcome::Error, true);
    let before = get(&f, id);
    f.plan(|p| p.build = Err(ProviderError::Config("clé API manquante".into())));
    assert_eq!(retry(&f, id).unwrap_err(), "configuration : clé API manquante");
    f.plan(|p| p.build = Ok(()));
    f.plan(|p| p.stt = Err(ProviderError::Auth));
    assert_eq!(retry(&f, id).unwrap_err(), "transcription : clé API refusée");
    f.plan(|p| p.stt = Ok("".into()));
    assert_eq!(retry(&f, id).unwrap_err(), "aucune parole détectée");
    std::fs::remove_file(before.audio_path.as_deref().unwrap()).unwrap();
    assert!(retry(&f, id).unwrap_err().starts_with("lecture audio impossible : "));

    assert_eq!(get(&f, id), before);
    assert!(f.ui.calls().is_empty());
}

// --- purge --------------------------------------------------------------------------------------

fn row_at(db: &Db, created_at: &str, audio_path: &std::path::Path) -> i64 {
    let mut r = build_record(&meta(), &Settings::default(), Delivery::Failed(&ProviderError::Auth));
    r.created_at = created_at.into();
    r.audio_path = Some(audio_path.to_string_lossy().into_owned());
    db.insert_dictation(&r).unwrap()
}

#[test]
fn purge_removes_old_recordings_and_forgets_their_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    let now: chrono::DateTime<chrono::Utc> = "2026-10-05T12:00:00Z".parse().unwrap();
    let old = dir.path().join("old.wav");
    let recent = dir.path().join("recent.wav");
    std::fs::write(&old, b"x").unwrap();
    std::fs::write(&recent, b"x").unwrap();
    let old_id = row_at(&db, "2026-08-01T00:00:00.000Z", &old);
    let gone_id = row_at(&db, "2026-08-02T00:00:00.000Z", &dir.path().join("gone.wav"));
    let recent_id = row_at(&db, "2026-10-01T00:00:00.000Z", &recent);

    assert_eq!(purge_audio_in(&db, 0, now), Vec::<i64>::new(), "0 = keep forever");
    assert!(old.exists());

    assert_eq!(purge_audio_in(&db, 30, now), vec![old_id, gone_id]);
    assert!(!old.exists());
    assert!(recent.exists());
    assert_eq!(db.get_dictation(old_id).unwrap().unwrap().audio_path, None);
    assert_eq!(db.get_dictation(gone_id).unwrap().unwrap().audio_path, None, "already deleted: forgotten too");
    assert!(db.get_dictation(recent_id).unwrap().unwrap().audio_path.is_some());
}

#[test]
fn purge_keeps_the_path_of_a_file_it_could_not_remove() {
    // A directory cannot be removed with remove_file (the error is not NotFound): the row keeps its
    // path so the next purge retries, instead of orphaning the file on disk.
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    let locked = dir.path().join("locked.wav");
    std::fs::create_dir(&locked).unwrap();
    let id = row_at(&db, "2026-01-01T00:00:00.000Z", &locked);
    let now: chrono::DateTime<chrono::Utc> = "2026-10-05T12:00:00Z".parse().unwrap();
    assert!(purge_audio_in(&db, 30, now).is_empty());
    assert_eq!(db.get_dictation(id).unwrap().unwrap().audio_path, Some(locked.to_string_lossy().into_owned()));
}

#[test]
fn purge_audio_uses_the_configured_retention() {
    let f = Fixture::with_settings(Settings { audio_retention_days: 1, ..Default::default() });
    let wav = f.svc.paths.audio_dir.join("a.wav");
    std::fs::write(&wav, b"x").unwrap();
    let id = row_at(&f.svc.db.lock().unwrap(), "2000-01-01T00:00:00.000Z", &wav);
    purge_audio(&f.svc);
    assert!(!wav.exists());
    assert_eq!(get(&f, id).audio_path, None);
}
