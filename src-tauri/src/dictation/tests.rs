use scribe_core::audio::TARGET_RATE;
use scribe_core::focus::FocusState;
use scribe_core::lang::Language;
use scribe_core::model::{Dictation, TermSource};
use scribe_core::style::{self, Preset};

use super::*;
use crate::locale::LanguagePref;
use crate::testing::{editable, Fixture, UiCall};

fn speech(ms: u32) -> AudioClip {
    let n = (TARGET_RATE as u64 * ms as u64 / 1000) as usize;
    let samples: Vec<f32> =
        (0..n).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / TARGET_RATE as f32).sin()).collect();
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

fn toast(level: ToastLevel, message: Notice, preview: Option<&str>, id: Option<i64>) -> OverlayEvent {
    badged_toast(level, message, preview, id, None)
}

fn badged_toast(
    level: ToastLevel,
    message: Notice,
    preview: Option<&str>,
    id: Option<i64>,
    style: Option<StyleBadge>,
) -> OverlayEvent {
    OverlayEvent::Toast { level, message, preview: preview.map(String::from), dictation_id: id, style }
}

fn stored(e: &ProviderError) -> Option<String> {
    Some(stored_error(e))
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
    let timeout = ProviderError::Timeout;
    let note = || Some(timeout.clone());
    assert_eq!(feedback(InsertResult::Pasted, None, "t", id, None), OverlayEvent::Idle);
    assert_eq!(
        feedback(InsertResult::Pasted, Some(&timeout), "t", id, None),
        toast(ToastLevel::Info, Notice::PastedWithoutCorrection { correction: timeout.clone() }, None, id)
    );
    assert_eq!(
        feedback(InsertResult::PastedUncertain, None, "t", id, None),
        toast(ToastLevel::Uncertain, Notice::PastedUncertain { correction: None }, Some("t"), id)
    );
    assert_eq!(
        feedback(InsertResult::PastedUncertain, Some(&timeout), "t", id, None),
        toast(ToastLevel::Uncertain, Notice::PastedUncertain { correction: note() }, Some("t"), id)
    );
    for r in [InsertResult::ClipboardOnly, InsertResult::PasteFailed] {
        assert_eq!(
            feedback(r, None, "t", None, None),
            toast(ToastLevel::Copied, Notice::Copied { correction: None }, Some("t"), None)
        );
    }
    assert_eq!(
        feedback(InsertResult::NotPasted, Some(&timeout), "t", id, None),
        toast(ToastLevel::Copied, Notice::NotPasted { correction: note() }, Some("t"), id)
    );
    assert_eq!(
        feedback(InsertResult::ClipboardFailed, Some(&timeout), "t", id, None),
        toast(ToastLevel::Error, Notice::ClipboardUnavailable { correction: note() }, Some("t"), id)
    );
}

#[test]
fn failure_feedback_points_to_the_history() {
    assert_eq!(
        failure_feedback(&ProviderError::Auth, Some(9)),
        toast(ToastLevel::Error, Notice::TranscriptionFailed { error: ProviderError::Auth }, None, Some(9))
    );
}

#[test]
fn errors_are_stored_as_json_for_the_history() {
    assert_eq!(stored_error(&ProviderError::Timeout), r#"{"code":"timeout"}"#);
    assert_eq!(
        stored_error(&ProviderError::MissingKey { provider: "OpenAI".into() }),
        r#"{"code":"missing_key","provider":"OpenAI"}"#
    );
}

fn meta() -> RecordMeta {
    RecordMeta {
        created_at: "2026-10-05T10:00:00.000Z".into(),
        mode: Mode::Hold,
        app_name: Some("Word".into()),
        audio_path: Some("a.wav".into()),
        duration_ms: 1_234,
    }
}

#[test]
fn build_record_for_a_failed_transcription() {
    let s = Settings::default();
    let r = build_record(&meta(), &s, Delivery::Failed(&ProviderError::Timeout));
    assert_eq!(r.outcome, Outcome::Error);
    assert_eq!(r.error, stored(&ProviderError::Timeout));
    assert_eq!((r.raw_text, r.final_text, r.corrector), (None, None, None));
    assert_eq!(r.transcriber.as_deref(), Some("gpt-transcribe"), "the configured model");
    assert_eq!(
        (r.created_at.as_str(), r.mode, r.app_name.as_deref(), r.audio_path.as_deref(), r.duration_ms),
        ("2026-10-05T10:00:00.000Z", Mode::Hold, Some("Word"), Some("a.wav"), 1_234)
    );
    assert_eq!((r.style, r.body_start), (None, None));
}

#[test]
fn build_record_for_a_delivered_dictation() {
    let out = PipelineOutput {
        raw: "r".into(),
        final_text: "F".into(),
        stt_ms: 10,
        llm_ms: Some(20),
        correction_error: Some(ProviderError::NoOutputTag),
        style: Some("slack".into()),
        body_start: Some(7),
    };
    let s = Settings::default();
    let done =
        |inserted| Delivery::Done { out: &out, transcriber: "stt".into(), corrector: Some("llm".into()), inserted };
    let r = build_record(&meta(), &s, done(InsertResult::PastedUncertain));
    assert_eq!((r.raw_text.as_deref(), r.final_text.as_deref()), (Some("r"), Some("F")));
    assert_eq!((r.transcriber.as_deref(), r.corrector.as_deref()), (Some("stt"), Some("llm")));
    assert_eq!((r.stt_ms, r.llm_ms), (Some(10), Some(20)));
    assert_eq!((r.outcome, r.error), (Outcome::PastedUncertain, stored(&ProviderError::NoOutputTag)));
    assert_eq!((r.style.as_deref(), r.body_start), (Some("slack"), Some(7)));
    // What the pipeline returns with correction off or for a short dictation: the corrector never ran.
    let uncorrected = PipelineOutput { llm_ms: None, correction_error: None, ..out.clone() };
    let raw = Settings { correction_enabled: false, ..Default::default() };
    let done = |inserted| Delivery::Done {
        out: &uncorrected,
        transcriber: "stt".into(),
        corrector: Some("llm".into()),
        inserted,
    };
    let r = build_record(&meta(), &raw, done(InsertResult::ClipboardFailed));
    assert_eq!(r.corrector, None, "no corrector with correction off");
    assert_eq!(r.outcome, Outcome::Error);
    // No model reached (missing correction key): the history shows no corrector, not a placeholder name.
    let unreachable = PipelineOutput { correction_error: Some(ProviderError::CorrectionDisabled), ..out.clone() };
    let r = build_record(
        &meta(),
        &s,
        Delivery::Done {
            out: &unreachable,
            transcriber: "stt".into(),
            corrector: None,
            inserted: InsertResult::Pasted,
        },
    );
    assert_eq!(r.corrector, None);
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
fn the_minimum_duration_is_inclusive() {
    let min = Settings::default().min_recording_ms;
    // One millisecond short of the minimum: dropped, nothing written.
    let f = Fixture::new();
    run(&f, speech(min as u32 - 1));
    assert_eq!(audio::duration_ms(&speech(min as u32 - 1)), min - 1);
    assert!(rows(&f).is_empty());
    assert!(f.wav_files().is_empty());
    assert_eq!(f.window.events(), vec![OverlayEvent::Idle]);
    // Exactly the minimum: processed like any dictation.
    let f = Fixture::new();
    run(&f, speech(min as u32));
    assert_eq!(audio::duration_ms(&speech(min as u32)), min);
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::Pasted);
    assert_eq!(row.duration_ms as u64, min);
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
    let missing = ProviderError::MissingKey { provider: "OpenAI".into() };
    f.plan(|p| p.build = Err(missing.clone()));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::Error);
    assert_eq!(row.error, stored(&missing));
    assert_eq!(row.raw_text, None);
    let audio = row.audio_path.clone().expect("audio kept");
    assert!(std::path::Path::new(&audio).exists());
    assert_eq!(f.wav_files().len(), 1);
    assert_eq!(f.window.last_event(), Some(failure_feedback(&missing, Some(row.id))));
    assert_eq!(f.ui.calls(), vec![UiCall::HistoryChanged]);
}

#[test]
fn transcription_failure_is_saved_without_text() {
    let f = Fixture::new();
    f.plan(|p| p.stt = Err(ProviderError::Auth));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.outcome, row.error, row.raw_text.as_deref()), (Outcome::Error, stored(&ProviderError::Auth), None));
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
    assert_eq!(
        (row.raw_text.as_deref(), row.final_text.as_deref()),
        (Some("bonjour scribe comment ça va"), Some("Bonjour Scribe, comment ça va ?"))
    );
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
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Pasted, Some("bonjour scribe comment ça va")));
    assert_eq!(row.error, stored(&ProviderError::Auth));
    assert_eq!(
        f.window.last_event(),
        Some(toast(
            ToastLevel::Info,
            Notice::PastedWithoutCorrection { correction: ProviderError::Auth },
            None,
            Some(row.id)
        ))
    );
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
        Some(toast(
            ToastLevel::Uncertain,
            Notice::PastedUncertain { correction: Some(ProviderError::Refusal) },
            Some("bonjour scribe comment ça va"),
            Some(row.id)
        ))
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
        assert_eq!(f.clipboard.text().as_deref(), Some("Bonjour Scribe, comment ça va ?"));
        assert_eq!(
            f.window.last_event(),
            Some(toast(
                ToastLevel::Copied,
                Notice::Copied { correction: None },
                Some("Bonjour Scribe, comment ça va ?"),
                Some(row.id)
            ))
        );
    }
}

#[test]
fn a_paste_seen_in_the_field_is_recorded_as_inserted_even_with_an_uncertain_focus() {
    let f = Fixture::new();
    set_end_focus(&f, FocusState::Unknown);
    f.field.script(&[Some("Objet : "), Some("Objet : Bonjour Scribe, comment ça va ?")]);
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::Pasted);
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert_eq!(f.window.last_event(), Some(OverlayEvent::Idle), "no « Texte inséré ? » toast");
}

#[test]
fn a_paste_the_field_did_not_receive_leaves_the_text_to_paste_by_hand() {
    let f = Fixture::new();
    f.field.script(&[Some("Objet : ")]);
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(row.outcome, Outcome::Clipboard);
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1, "Ctrl+V was sent");
    assert_eq!(
        f.clipboard.text().as_deref(),
        Some("Bonjour Scribe, comment ça va ?"),
        "not restored: ready for Ctrl+V"
    );
    assert_eq!(
        f.window.last_event(),
        Some(toast(
            ToastLevel::Copied,
            Notice::NotPasted { correction: None },
            Some("Bonjour Scribe, comment ça va ?"),
            Some(row.id)
        ))
    );
}

#[test]
fn unavailable_clipboard_keeps_the_text_in_the_history() {
    let f = Fixture::new();
    *f.clipboard.fail.lock().unwrap() = true;
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Error, Some("Bonjour Scribe, comment ça va ?")));
    assert_eq!(
        f.window.last_event(),
        Some(toast(
            ToastLevel::Error,
            Notice::ClipboardUnavailable { correction: None },
            Some("Bonjour Scribe, comment ça va ?"),
            Some(row.id)
        ))
    );
}

#[test]
fn correction_off_records_no_corrector() {
    let f = Fixture::with_settings(Settings { correction_enabled: false, restore_delay_ms: 0, ..Default::default() });
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(
        (row.final_text.as_deref(), row.corrector, row.llm_ms),
        (Some("bonjour scribe comment ça va"), None, None)
    );
    assert_eq!((row.style, row.body_start), (None, None));
}

#[test]
fn correction_off_still_strips_the_style_keyword() {
    let f = Fixture::with_settings(Settings { correction_enabled: false, restore_delay_ms: 0, ..Default::default() });
    f.plan(|p| p.stt = Ok("Slack, on décale la réunion".into()));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.final_text.as_deref(), row.style, row.body_start), (Some("on décale la réunion"), None, Some(7)));
    assert!(f.prompts.lock().unwrap().is_empty());
}

#[test]
fn a_spoken_keyword_applies_its_style_in_the_ui_language() {
    let f = Fixture::with_settings(Settings { language: LanguagePref::En, restore_delay_ms: 0, ..Default::default() });
    f.plan(|p| {
        p.stt = Ok("Slack, on décale la réunion".into());
        p.llm = Ok("<output>On décale la réunion.</output>".into());
    });
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(
        (row.final_text.as_deref(), row.style.as_deref(), row.body_start),
        (Some("On décale la réunion."), Some("slack"), Some(7))
    );
    let p = f.last_prompt();
    assert!(p.system.contains(style::preset_task(Preset::Slack, Language::En)), "{}", p.system);
    assert!(p.user.contains("<transcript>\non décale la réunion\n</transcript>"), "{}", p.user);
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
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Pasted, Some("Bonjour Scribe, comment ça va ?")));
}

// --- retranscribe -------------------------------------------------------------------------------

/// A stored dictation with a real .wav, as process() leaves it.
fn stored_row(f: &Fixture, outcome: Outcome, with_audio: bool) -> i64 {
    let path = f.svc.paths.audio_dir.join("old.wav");
    std::fs::write(&path, audio::encode_wav(&speech(500)).unwrap()).unwrap();
    let mut r = build_record(&meta(), &Settings::default(), Delivery::Failed(&ProviderError::Auth));
    r.outcome = outcome;
    r.audio_path = with_audio.then(|| path.to_string_lossy().into_owned());
    f.svc.db.lock().unwrap().insert_dictation(&r).unwrap()
}

fn retry(f: &Fixture, id: i64) -> Result<(), AppError> {
    tauri::async_runtime::block_on(retranscribe(f.svc.clone(), id))
}

fn get(f: &Fixture, id: i64) -> Dictation {
    f.svc.db.lock().unwrap().get_dictation(id).unwrap().unwrap()
}

#[test]
fn retranscribing_a_failed_dictation_copies_the_new_text() {
    let f = Fixture::new();
    let id = stored_row(&f, Outcome::Error, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!(
        (d.raw_text.as_deref(), d.final_text.as_deref()),
        (Some("bonjour scribe comment ça va"), Some("Bonjour Scribe, comment ça va ?"))
    );
    assert_eq!((d.transcriber.as_deref(), d.corrector.as_deref()), (Some("fake-stt"), Some("fake-llm")));
    assert_eq!((d.outcome, d.error), (Outcome::Clipboard, None));
    assert_eq!(f.clipboard.text().as_deref(), Some("Bonjour Scribe, comment ça va ?"));
    assert_eq!(f.ui.calls(), vec![UiCall::HistoryChanged]);
}

#[test]
fn retranscribe_keeps_the_error_outcome_when_the_copy_fails() {
    let f = Fixture::new();
    *f.clipboard.fail.lock().unwrap() = true;
    let id = stored_row(&f, Outcome::Error, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!((d.outcome, d.final_text.as_deref()), (Outcome::Error, Some("Bonjour Scribe, comment ça va ?")));
}

#[test]
fn retranscribing_a_delivered_dictation_keeps_its_outcome_and_clipboard() {
    let f = Fixture::with_settings(Settings { correction_enabled: false, ..Default::default() });
    f.plan(|p| p.stt = Ok("nouveau texte".into()));
    let id = stored_row(&f, Outcome::PastedUncertain, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!(
        (d.outcome, d.final_text.as_deref(), d.corrector),
        (Outcome::PastedUncertain, Some("nouveau texte"), None)
    );
    assert_eq!(f.clipboard.text(), None);
}

#[test]
fn retranscribing_detects_the_style_again() {
    let f = Fixture::new();
    f.plan(|p| p.stt = Ok("Mail : bonjour Paul, à demain".into()));
    let id = stored_row(&f, Outcome::Error, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!((d.style.as_deref(), d.body_start), (Some("mail"), Some(7)));
    assert!(f.last_prompt().user.contains("<transcript>\nbonjour Paul, à demain\n</transcript>"));
}

#[test]
fn a_short_dictation_is_pasted_uncorrected_and_records_no_corrector() {
    let f = Fixture::new();
    f.plan(|p| p.stt = Ok("OK, merci beaucoup.".into()));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!((row.outcome, row.final_text.as_deref()), (Outcome::Pasted, Some("OK, merci beaucoup.")));
    assert_eq!((row.corrector, row.llm_ms, row.error), (None, None, None));

    let id = stored_row(&f, Outcome::Pasted, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!((d.final_text.as_deref(), d.corrector), (Some("OK, merci beaucoup."), None));
}

#[test]
fn retranscribe_records_a_correction_failure() {
    let f = Fixture::new();
    f.plan(|p| p.llm = Ok("pas de balise".into()));
    let id = stored_row(&f, Outcome::Pasted, true);
    retry(&f, id).unwrap();
    let d = get(&f, id);
    assert_eq!(d.final_text.as_deref(), Some("bonjour scribe comment ça va"));
    assert_eq!(d.error, stored(&ProviderError::NoOutputTag));
}

#[test]
fn retranscribe_errors_leave_the_row_untouched() {
    let f = Fixture::new();
    assert_eq!(retry(&f, 404).unwrap_err(), AppError::DictationNotFound);

    let purged = stored_row(&f, Outcome::Error, false);
    assert_eq!(retry(&f, purged).unwrap_err(), AppError::AudioPurged);

    let id = stored_row(&f, Outcome::Error, true);
    let before = get(&f, id);
    let missing = ProviderError::MissingKey { provider: "OpenAI".into() };
    f.plan(|p| p.build = Err(missing.clone()));
    assert_eq!(retry(&f, id).unwrap_err(), AppError::Provider { error: missing });
    f.plan(|p| p.build = Ok(()));
    f.plan(|p| p.stt = Err(ProviderError::Auth));
    assert_eq!(retry(&f, id).unwrap_err(), AppError::Provider { error: ProviderError::Auth });
    f.plan(|p| p.stt = Ok("".into()));
    assert_eq!(retry(&f, id).unwrap_err(), AppError::NoSpeech);
    std::fs::remove_file(before.audio_path.as_deref().unwrap()).unwrap();
    assert!(matches!(
        retry(&f, id).unwrap_err(),
        AppError::System { detail } if detail.starts_with("cannot read the audio: ")
    ));

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

// --- style badge ---------------------------------------------------------------------------------

fn slack() -> Option<StyleBadge> {
    Some(StyleBadge { id: "slack".into(), name: "Slack".into() })
}

fn processing(f: &Fixture) -> Vec<OverlayEvent> {
    f.window.events().into_iter().filter(|e| matches!(e, OverlayEvent::Processing { .. })).collect()
}

#[test]
fn feedback_carries_the_badge_and_toasts_a_clean_paste_only_with_a_style() {
    let id = Some(3);
    assert_eq!(
        feedback(InsertResult::Pasted, None, "t", id, slack()),
        badged_toast(ToastLevel::Info, Notice::Pasted, None, id, slack())
    );
    assert_eq!(
        feedback(InsertResult::Pasted, Some(&ProviderError::Auth), "t", id, slack()),
        badged_toast(
            ToastLevel::Info,
            Notice::PastedWithoutCorrection { correction: ProviderError::Auth },
            None,
            id,
            slack()
        )
    );
    assert_eq!(
        feedback(InsertResult::NotPasted, None, "t", id, slack()),
        badged_toast(ToastLevel::Copied, Notice::NotPasted { correction: None }, Some("t"), id, slack())
    );
}

#[test]
fn a_keyword_style_shows_its_badge_from_processing_to_the_final_toast() {
    let f = Fixture::new();
    f.plan(|p| p.stt = Ok("Slack, on décale la réunion à demain".into()));
    run(&f, speech(1_000));
    let row = only_row(&f);
    assert_eq!(processing(&f), vec![OverlayEvent::Processing { style: slack() }]);
    assert_eq!(
        f.window.last_event(),
        Some(badged_toast(ToastLevel::Info, Notice::Pasted, None, Some(row.id), slack()))
    );
}

#[test]
fn without_a_keyword_processing_clears_the_badge_and_the_paste_stays_silent() {
    let f = Fixture::new();
    run(&f, speech(1_000));
    assert_eq!(processing(&f), vec![OverlayEvent::Processing { style: None }]);
    assert_eq!(f.window.last_event(), Some(OverlayEvent::Idle));
}

#[test]
fn with_correction_off_a_keyword_shows_no_badge() {
    let f = Fixture::with_settings(Settings { correction_enabled: false, restore_delay_ms: 0, ..Default::default() });
    f.plan(|p| p.stt = Ok("Slack, on décale la réunion à demain".into()));
    run(&f, speech(1_000));
    assert_eq!(only_row(&f).final_text.as_deref(), Some("on décale la réunion à demain"));
    assert!(processing(&f).is_empty());
    assert_eq!(f.window.last_event(), Some(OverlayEvent::Idle));
}

#[test]
fn the_badge_follows_the_ui_language() {
    let f = Fixture::with_settings(Settings { language: LanguagePref::En, restore_delay_ms: 0, ..Default::default() });
    f.plan(|p| p.stt = Ok("Mail, bonjour Paul, on se voit demain".into()));
    set_end_focus(&f, FocusState::NotEditable);
    run(&f, speech(1_000));
    let email = Some(StyleBadge { id: "mail".into(), name: "Email".into() });
    assert_eq!(processing(&f), vec![OverlayEvent::Processing { style: email.clone() }]);
    let Some(OverlayEvent::Toast { message, style, .. }) = f.window.last_event() else { panic!("no toast") };
    assert_eq!((message, style), (Notice::Copied { correction: None }, email));
}

// --- transcribe_prefix --------------------------------------------------------------------------

#[test]
fn transcribe_prefix_returns_the_text_or_none() {
    let f = Fixture::new();
    f.svc.db.lock().unwrap().add_term("Tauri", &[], None, TermSource::Manual, "2026-01-01T00:00:00.000Z").unwrap();
    let prefix = || tauri::async_runtime::block_on(transcribe_prefix(&f.svc, speech(500)));
    assert_eq!(prefix().as_deref(), Some("bonjour scribe comment ça va"));
    let keywords = style::keywords(&f.svc.settings.read().unwrap().styles);
    assert!(!keywords.is_empty());
    assert_eq!(f.hints.lock().unwrap().as_slice(), [keywords], "keyword hints only, no glossary term");
    f.plan(|p| p.stt = Err(ProviderError::Auth));
    assert_eq!(prefix(), None);
    f.plan(|p| p.build = Err(ProviderError::MissingKey { provider: "OpenAI".into() }));
    assert_eq!(prefix(), None);
    assert!(rows(&f).is_empty(), "a peek is never recorded");
}

// --- last paste ---------------------------------------------------------------------------------

#[test]
fn only_a_pasted_dictation_becomes_the_last_paste() {
    let f = Fixture::new();
    run(&f, speech(1_000));
    let pasted = PastedText {
        dictation_id: only_row(&f).id,
        focus: editable("Notepad"),
        text: "Bonjour Scribe, comment ça va ?".into(),
    };
    assert_eq!(*f.svc.last_paste.lock().unwrap(), Some(pasted.clone()));
    set_end_focus(&f, FocusState::NotEditable);
    run(&f, speech(1_000));
    assert_eq!(*f.svc.last_paste.lock().unwrap(), Some(pasted), "a copied dictation is not pasted");

    let f = Fixture::new();
    set_end_focus(&f, FocusState::Unknown);
    run(&f, speech(1_000));
    assert_eq!(f.svc.last_paste.lock().unwrap().as_ref().map(|p| p.focus.state), Some(FocusState::Unknown));
}

// --- restyle ------------------------------------------------------------------------------------

const PASTED: &str = "On se voit demain.";
const NEW: &str = "Bonjour,\n\nOn se voit demain.\n\nBonne journée";

fn newest_id(f: &Fixture) -> i64 {
    rows(f).iter().map(|d| d.id).max().unwrap()
}

/// A « Slack, … » dictation pasted in Notepad, then the corrector answering `NEW` and a field holding the pasted
/// text before its caret, which pastes as asked.
fn pasted_slack(f: &Fixture) -> i64 {
    f.plan(|p| {
        p.stt = Ok("Slack, on se voit demain".into());
        p.llm = Ok(format!("<output>{PASTED}</output>"));
    });
    run(f, speech(1_000));
    f.plan(|p| p.llm = Ok(format!("<output>{NEW}</output>")));
    *f.field.before_caret.lock().unwrap() = Some(format!("Objet : {PASTED}"));
    *f.field.selectable.lock().unwrap() = true;
    f.field.script(&[Some(&format!("Objet : {PASTED}")), Some(&format!("Objet : {NEW}"))]);
    *f.clipboard.content.lock().unwrap() = Some("ancien contenu".into());
    newest_id(f)
}

fn restyle_to(f: &Fixture, id: i64, style: &str) -> Result<(), AppError> {
    tauri::async_runtime::block_on(restyle(f.svc.clone(), id, style))
}

fn badge(id: &str, name: &str) -> Option<StyleBadge> {
    Some(StyleBadge { id: id.into(), name: name.into() })
}

fn mail() -> Option<StyleBadge> {
    badge("mail", "Mail")
}

#[test]
fn the_text_to_restyle_is_the_body_except_for_the_default_style() {
    let styles = style::catalog();
    let (default, slack) = (&styles[0], style::find(&styles, "slack").unwrap());
    let raw = "Slack, on se voit demain  ";
    assert_eq!(restyle_source(raw, Some(7), slack), "on se voit demain");
    assert_eq!(restyle_source(raw, Some(7), default), raw, "the default style gets the keyword back");
    assert_eq!(restyle_source(raw, None, slack), raw, "no keyword was detected");
    for wrong in [-1, 99] {
        assert_eq!(restyle_source(raw, Some(wrong), slack), raw);
    }
    assert_eq!(restyle_source("é, x", Some(1), slack), "é, x", "not a char boundary");
}

#[test]
fn a_style_switch_replaces_the_pasted_text_in_place() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    restyle_to(&f, id, "mail").unwrap();
    assert_eq!(*f.keys.pastes.lock().unwrap(), 2);
    assert!(f.keys.presses.lock().unwrap().is_empty(), "selected through accessibility");
    assert_eq!(f.field.selected.lock().unwrap().as_deref(), Some(PASTED));
    assert_eq!(f.clipboard.text().as_deref(), Some("ancien contenu"), "clipboard restored");
    assert_eq!(f.window.last_event(), Some(badged_toast(ToastLevel::Info, Notice::Restyled, None, Some(id), mail())));
    assert_eq!(f.svc.last_paste.lock().unwrap().as_ref().map(|p| p.text.as_str()), Some(NEW));
    let row = get(&f, id);
    assert_eq!(
        (row.final_text.as_deref(), row.style.as_deref(), row.outcome, row.edited_text),
        (Some(NEW), Some("mail"), Outcome::Pasted, None)
    );
    assert_eq!((row.raw_text.as_deref(), row.body_start), (Some("Slack, on se voit demain"), Some(7)));
    assert_eq!(row.corrector.as_deref(), Some("fake-llm"));
    assert!(f.last_prompt().user.contains("<transcript>\non se voit demain\n</transcript>"));
    assert!(f.last_prompt().system.contains(style::preset_task(Preset::Mail, Language::Fr)));
    assert_eq!(f.ui.count(&UiCall::HistoryChanged), 2);
}

#[test]
fn a_style_switch_copies_the_text_when_replacing_is_not_safe() {
    type Setup = fn(&Fixture);
    let cases: [(&str, Setup); 6] = [
        ("other window", |f| f.focus.0.lock().unwrap().window_id = Some(8)),
        ("terminal", |f| set_end_focus(f, FocusState::Terminal)),
        ("caret unreadable", |f| *f.field.before_caret.lock().unwrap() = None),
        ("text moved", |f| *f.field.before_caret.lock().unwrap() = Some("autre chose".into())),
        ("not the last paste", |f| *f.svc.last_paste.lock().unwrap() = None),
        ("too long", |f| {
            let long = "a".repeat(MAX_SELECT_GRAPHEMES + 1);
            f.svc.last_paste.lock().unwrap().as_mut().unwrap().text = long.clone();
            *f.field.before_caret.lock().unwrap() = Some(long);
        }),
    ];
    for (name, setup) in cases {
        let f = Fixture::new();
        let id = pasted_slack(&f);
        setup(&f);
        restyle_to(&f, id, "mail").unwrap();
        assert_eq!(*f.keys.pastes.lock().unwrap(), 1, "{name}: no keys sent");
        assert!(f.keys.presses.lock().unwrap().is_empty(), "{name}");
        assert_eq!(f.clipboard.text().as_deref(), Some(NEW), "{name}");
        assert_eq!(
            f.window.last_event(),
            Some(badged_toast(ToastLevel::Copied, Notice::RestyleCopied, Some(NEW), Some(id), mail())),
            "{name}"
        );
        let row = get(&f, id);
        assert_eq!((row.final_text.as_deref(), row.outcome), (Some(NEW), Outcome::Clipboard), "{name}");
        assert_ne!(f.svc.last_paste.lock().unwrap().as_ref().map(|p| p.text.as_str()), Some(NEW), "{name}");
    }
}

/// The rewrite is saved, but the user's clipboard is never overwritten by a text no toast can announce.
fn assert_kept_in_history_only(f: &Fixture, id: i64, shown: Option<OverlayEvent>) {
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert!(f.keys.presses.lock().unwrap().is_empty());
    assert_eq!(f.clipboard.text().as_deref(), Some("ancien contenu"));
    assert_eq!(f.window.last_event(), shown);
    let row = get(f, id);
    assert_eq!(
        (row.final_text.as_deref(), row.style.as_deref(), row.outcome),
        (Some(NEW), Some("mail"), Outcome::Pasted)
    );
    assert_eq!(f.ui.count(&UiCall::HistoryChanged), 2);
}

#[test]
fn a_switch_landing_during_a_new_dictation_leaves_the_clipboard_alone() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    f.svc.session_busy.store(true, Ordering::Relaxed);
    let shown = f.window.last_event();
    restyle_to(&f, id, "mail").unwrap();
    assert_kept_in_history_only(&f, id, shown);
    assert_ne!(f.svc.last_paste.lock().unwrap().as_ref().map(|p| p.text.as_str()), Some(NEW));
}

#[test]
fn a_switch_landing_on_a_newer_dictations_toast_leaves_the_clipboard_alone() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    *f.svc.last_paste.lock().unwrap() = None;
    f.svc.overlay.toast(ToastLevel::Copied, Notice::Copied { correction: None }, None, Some(id + 1));
    let shown = f.window.last_event();
    restyle_to(&f, id, "mail").unwrap();
    assert_kept_in_history_only(&f, id, shown);
}

#[test]
fn a_selection_the_field_never_shows_leaves_the_new_text_in_the_clipboard() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    *f.field.selectable.lock().unwrap() = false;
    *f.field.selected.lock().unwrap() = Some("partiel".into());
    restyle_to(&f, id, "mail").unwrap();
    let steps = insert::caret_units(PASTED).graphemes;
    assert_eq!(
        *f.keys.presses.lock().unwrap(),
        vec![(insert::NavKey::Left, true, steps), (insert::NavKey::Right, false, 1)],
        "arrow fallback, then the selection is collapsed"
    );
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert_eq!(f.clipboard.text().as_deref(), Some(NEW));
    assert_eq!(
        f.window.last_event(),
        Some(badged_toast(ToastLevel::Copied, Notice::RestyleCopied, Some(NEW), Some(id), mail()))
    );
}

#[test]
fn an_unavailable_clipboard_keeps_the_new_text_in_the_history() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    *f.svc.last_paste.lock().unwrap() = None;
    *f.clipboard.fail.lock().unwrap() = true;
    restyle_to(&f, id, "mail").unwrap();
    assert_eq!(
        f.window.last_event(),
        Some(badged_toast(
            ToastLevel::Error,
            Notice::ClipboardUnavailable { correction: None },
            Some(NEW),
            Some(id),
            mail()
        ))
    );
    assert_eq!((get(&f, id).final_text.as_deref(), get(&f, id).outcome), (Some(NEW), Outcome::Error));
}

#[test]
fn a_rewrite_identical_to_the_pasted_text_sends_no_keys() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    f.plan(|p| p.llm = Ok(format!("<output>{PASTED}</output>")));
    restyle_to(&f, id, "clean").unwrap();
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert!(f.keys.presses.lock().unwrap().is_empty());
    assert_eq!(f.clipboard.text().as_deref(), Some("ancien contenu"));
    assert_eq!(
        f.window.last_event(),
        Some(badged_toast(ToastLevel::Info, Notice::Restyled, None, Some(id), badge("clean", "Propre")))
    );
    assert_eq!((get(&f, id).style.as_deref(), get(&f, id).outcome), (Some("clean"), Outcome::Pasted));
}

#[test]
fn a_failed_rewrite_says_so_and_changes_nothing() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    let before = get(&f, id);
    f.plan(|p| p.llm = Err(ProviderError::Auth));
    restyle_to(&f, id, "mail").unwrap();
    assert_eq!(
        f.window.last_event(),
        Some(badged_toast(
            ToastLevel::Error,
            Notice::RestyleFailed { error: ProviderError::Auth },
            None,
            Some(id),
            mail()
        ))
    );
    assert_eq!(get(&f, id), before);
    assert_eq!(*f.keys.pastes.lock().unwrap(), 1);
    assert_eq!(f.ui.count(&UiCall::HistoryChanged), 1);
}

#[test]
fn switching_to_the_default_style_corrects_the_whole_transcript() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    restyle_to(&f, id, style::DEFAULT_STYLE_ID).unwrap();
    assert!(f.last_prompt().user.contains("<transcript>\nSlack, on se voit demain\n</transcript>"));
    assert_eq!(
        f.window.last_event(),
        Some(badged_toast(ToastLevel::Info, Notice::Restyled, None, Some(id), badge("default", "Par défaut")))
    );
    assert_eq!(get(&f, id).style.as_deref(), Some(style::DEFAULT_STYLE_ID));
}

#[test]
fn the_body_is_sliced_where_it_was_detected_even_after_the_keyword_changed() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    for s in f.svc.settings.write().unwrap().styles.iter_mut().filter(|s| s.id == "slack") {
        s.keyword = Some("messagerie".into());
    }
    restyle_to(&f, id, "slack").unwrap();
    assert!(f.last_prompt().user.contains("<transcript>\non se voit demain\n</transcript>"));
}

#[test]
fn a_style_switch_needs_no_transcription_key() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    f.plan(|p| p.build = Err(ProviderError::MissingKey { provider: "OpenAI".into() }));
    restyle_to(&f, id, "mail").unwrap();
    assert_eq!(get(&f, id).final_text.as_deref(), Some(NEW));
}

#[test]
fn a_style_switch_refuses_unknown_styles_and_dictations_without_transcript() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    assert_eq!(restyle_to(&f, id, "gone").unwrap_err(), AppError::StyleNotFound);
    assert_eq!(restyle_to(&f, 999, "mail").unwrap_err(), AppError::DictationNotFound);
    f.plan(|p| p.stt = Err(ProviderError::Auth));
    run(&f, speech(1_000));
    assert_eq!(restyle_to(&f, newest_id(&f), "mail").unwrap_err(), AppError::NothingToRestyle);
    assert_eq!(f.svc.restyle_seq.load(Ordering::SeqCst), 0, "nothing was asked of the corrector");
}

#[test]
fn a_late_switch_result_never_covers_a_new_recording() {
    let f = Fixture::new();
    let id = pasted_slack(&f);
    *f.svc.last_paste.lock().unwrap() = None;
    let recording = OverlayEvent::Recording { locked: false, warning: None, style: None };
    f.svc.overlay.emit(recording.clone());
    restyle_to(&f, id, "mail").unwrap();
    assert_kept_in_history_only(&f, id, Some(recording));
}

#[test]
fn only_the_last_switch_asked_is_applied_whatever_order_they_finish_in() {
    for newer_first in [false, true] {
        let f = Fixture::new();
        let id = pasted_slack(&f);
        *f.svc.last_paste.lock().unwrap() = None;
        let block = |rw: Rewrite| tauri::async_runtime::block_on(apply_rewrite(&f.svc, rw)).unwrap();
        f.plan(|p| p.llm = Ok("<output>Propre.</output>".into()));
        let older = tauri::async_runtime::block_on(rewrite(&f.svc, id, "clean")).unwrap();
        f.plan(|p| p.llm = Ok(format!("<output>{NEW}</output>")));
        let newer = tauri::async_runtime::block_on(rewrite(&f.svc, id, "mail")).unwrap();
        let toasts_before = f.window.events().len();
        if newer_first {
            block(newer);
            block(older);
        } else {
            block(older);
            block(newer);
        }
        assert_eq!(f.window.events().len(), toasts_before + 1, "newer first: {newer_first}");
        assert_eq!(f.clipboard.text().as_deref(), Some(NEW));
        let row = get(&f, id);
        assert_eq!((row.final_text.as_deref(), row.style.as_deref()), (Some(NEW), Some("mail")));
        assert_eq!(f.ui.count(&UiCall::HistoryChanged), 2);
    }
}
