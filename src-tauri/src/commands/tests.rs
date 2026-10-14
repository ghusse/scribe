use tauri::Manager;

use scribe_core::gesture::Mode;
use scribe_core::model::{NewDictation, Outcome};
use scribe_core::pipeline::ProviderError;
use scribe_platform::RawKey;

use super::*;
use crate::locale::LanguagePref;
use crate::overlay::{Notice, OverlayEvent, ToastLevel};
use crate::settings::SettingsError;
use crate::testing::{Fixture, MuteCall, UiCall};

/// Runs `f` with the fixture's services as Tauri state, the way commands receive them.
fn with_state<T>(f: &Fixture, run: impl FnOnce(Svc<'_>) -> T) -> T {
    let app = tauri::test::mock_app();
    app.manage(f.svc.clone());
    run(app.state::<Arc<Services>>())
}

fn insert(f: &Fixture, final_text: Option<&str>, audio_path: Option<String>) -> i64 {
    let d = NewDictation {
        created_at: now_rfc3339(),
        mode: Mode::Hold,
        app_name: None,
        app_bundle_id: None,
        audio_path,
        duration_ms: 1_000,
        raw_text: final_text.map(|t| t.to_lowercase()),
        final_text: final_text.map(String::from),
        style: Some("default".into()),
        body_start: None,
        transcriber: None,
        corrector: None,
        stt_ms: None,
        llm_ms: None,
        outcome: Outcome::Pasted,
        error: None,
    };
    f.svc.db.lock().unwrap().insert_dictation(&d).unwrap()
}

#[test]
fn history_commands_read_edit_and_delete_rows() {
    let f = Fixture::new();
    let wav = f.svc.paths.audio_dir.join("a.wav");
    std::fs::write(&wav, b"x").unwrap();
    let a = insert(&f, Some("Bonjour Scribe, comment ça va ?"), Some(wav.to_string_lossy().into_owned()));
    let b = insert(&f, Some("Autre chose."), None);
    with_state(&f, |s| {
        let all = list_dictations(s.clone(), None, 10, 0).unwrap();
        assert_eq!(all.len(), 2);
        let found = list_dictations(s.clone(), Some("scribe".into()), 10, 0).unwrap();
        assert_eq!(found.iter().map(|d| d.id).collect::<Vec<_>>(), vec![a]);

        save_edited_text(s.clone(), b, Some("Corrigé.".into())).unwrap();
        copy_dictation(s.clone(), b).unwrap();
        assert_eq!(f.clipboard.text().as_deref(), Some("Corrigé."), "the edited text wins");

        delete_dictation(s.clone(), a).unwrap();
        assert!(!wav.exists(), "its recording is deleted too");
        delete_dictation(s.clone(), b).unwrap();
        assert!(list_dictations(s, None, 10, 0).unwrap().is_empty());
    });
}

#[test]
fn copy_reports_missing_rows_text_and_clipboard_errors() {
    let f = Fixture::new();
    let empty = insert(&f, None, None);
    let ok = insert(&f, Some("Texte."), None);
    with_state(&f, |s| {
        assert_eq!(copy_dictation(s.clone(), 999).unwrap_err(), AppError::DictationNotFound);
        assert_eq!(copy_dictation(s.clone(), empty).unwrap_err(), AppError::NoText);
        *f.clipboard.fail.lock().unwrap() = true;
        assert_eq!(copy_dictation(s, ok).unwrap_err(), AppError::System { detail: "presse-papier occupé".into() });
    });
}

#[test]
fn retranscribe_command_delegates_to_the_pipeline() {
    let f = Fixture::new();
    with_state(&f, |s| {
        let r = tauri::async_runtime::block_on(retranscribe(s, 42));
        assert_eq!(r.unwrap_err(), AppError::DictationNotFound);
    });
}

#[test]
fn glossary_commands_round_trip() {
    let f = Fixture::new();
    with_state(&f, |s| {
        let id = add_term(s.clone(), "Scribe".into(), vec!["scribe".into()], Some("l'app".into())).unwrap();
        update_term(s.clone(), id, "Scribe".into(), vec!["scribes".into()], None).unwrap();
        let terms = list_terms(s.clone()).unwrap();
        assert_eq!(terms.len(), 1);
        assert_eq!(
            (terms[0].variants.clone(), terms[0].note.clone(), terms[0].source),
            (vec!["scribes".to_string()], None, TermSource::Manual)
        );
        delete_term(s.clone(), id).unwrap();
        assert!(list_terms(s.clone()).unwrap().is_empty());
        assert!(add_term(s.clone(), "  ".into(), vec![], None).is_err(), "blank term rejected by the store");
        add_term(s.clone(), "Tauri".into(), vec![], None).unwrap();
        assert_eq!(
            add_term(s, "tauri".into(), vec![], None).unwrap_err(),
            AppError::DuplicateTerm { term: "tauri".into() }
        );
    });
}

#[test]
fn valid_settings_are_saved_applied_and_announced() {
    let f = Fixture::new();
    let new = Settings { correction_enabled: false, trigger_keys: vec![0xA2, 0x41], ..Default::default() };
    with_state(&f, |s| {
        // Saved and applied in canonical order.
        save_settings(s.clone(), Settings { trigger_keys: vec![0x41, 0xA3], ..new.clone() }).unwrap();
        assert_eq!(get_settings(s), new);
    });
    assert_eq!(Settings::load(&f.svc.paths.settings_path), new);
    assert!(matches!(f.ctrl_rx.try_recv(), Ok(ControllerMsg::SettingsChanged)));
}

#[test]
fn invalid_settings_are_neither_written_nor_applied() {
    let f = Fixture::new();
    let before = f.svc.settings.read().unwrap().clone();
    with_state(&f, |s| {
        let err = save_settings(s.clone(), Settings { trigger_keys: vec![], ..Default::default() }).unwrap_err();
        assert_eq!(err, AppError::InvalidSettings { error: SettingsError::InvalidHotkey });
        assert_eq!(get_settings(s.clone()), before);
        // A valid value that cannot be written is not applied either.
        std::fs::create_dir(f.svc.paths.settings_path.with_extension("json.tmp")).unwrap();
        assert!(save_settings(s.clone(), Settings::default()).is_err());
        assert_eq!(get_settings(s), before);
    });
    assert!(!f.svc.paths.settings_path.exists());
    assert!(f.ctrl_rx.try_recv().is_err());
}

#[test]
fn a_new_ui_language_is_announced_once() {
    let f = Fixture::new();
    with_state(&f, |s| {
        assert_eq!(language(s.clone()), Language::Fr, "system: fr-FR first");
        save_settings(s.clone(), Settings { language: LanguagePref::Fr, ..Default::default() }).unwrap();
        assert!(f.ui.calls().is_empty(), "the system language was already French");
        save_settings(s.clone(), Settings { language: LanguagePref::En, ..Default::default() }).unwrap();
        assert_eq!(language(s.clone()), Language::En);
        save_settings(
            s.clone(),
            Settings { language: LanguagePref::En, correction_enabled: false, ..Default::default() },
        )
        .unwrap();
        assert_eq!(f.ui.calls(), vec![UiCall::LanguageChanged(Language::En)], "only on a change");
        save_settings(s.clone(), Settings::default()).unwrap();
        assert_eq!(language(s), Language::Fr);
    });
    assert_eq!(f.ui.calls(), vec![UiCall::LanguageChanged(Language::En), UiCall::LanguageChanged(Language::Fr)]);
}

#[test]
fn an_invalid_language_change_is_not_announced() {
    let f = Fixture::new();
    with_state(&f, |s| {
        let bad = Settings { language: LanguagePref::En, trigger_keys: vec![], ..Default::default() };
        assert!(save_settings(s.clone(), bad).is_err());
        assert_eq!(language(s), Language::Fr);
    });
    assert!(f.ui.calls().is_empty());
}

#[test]
fn api_keys_are_stored_and_reported_per_provider() {
    let f = Fixture::new();
    with_state(&f, |s| {
        let status = key_status(s.clone());
        assert_eq!(status.len(), catalog::PROVIDERS.len());
        assert!(status.values().all(|v| !v));
        set_api_key(s.clone(), "openai".into(), " sk-1 ".into()).unwrap();
        let status = key_status(s.clone());
        assert_eq!((status["openai"], status["anthropic"]), (true, false));
        assert_eq!(
            set_api_key(s.clone(), "nope".into(), "k".into()).unwrap_err(),
            AppError::System { detail: "unknown provider: nope".into() }
        );
        set_api_key(s.clone(), "openai".into(), "".into()).unwrap();
        assert!(!key_status(s)["openai"]);
    });
    assert!(f.secrets.keys.lock().unwrap().is_empty());
}

fn test_providers_with(f: &Fixture) -> Result<ProviderTest, AppError> {
    with_state(f, |s| tauri::async_runtime::block_on(test_providers(s)))
}

#[test]
fn provider_test_reports_each_call() {
    let f = Fixture::new();
    let r = test_providers_with(&f).unwrap();
    assert!(r.stt.is_ok() && matches!(r.llm, Some(Ok(_))), "{r:?}");

    f.plan(|p| {
        p.stt = Err(ProviderError::Auth);
        p.llm = Err(ProviderError::Http { status: 404, body: "modèle inconnu".into() });
    });
    let r = test_providers_with(&f).unwrap();
    assert_eq!(r.stt, Err(ProviderError::Auth));
    assert_eq!(r.llm, Some(Err(ProviderError::Http { status: 404, body: "modèle inconnu".into() })));
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        serde_json::json!({
            "stt": {"Err": {"code": "auth"}},
            "llm": {"Err": {"code": "http", "status": 404, "body": "modèle inconnu"}}
        })
    );

    let missing = ProviderError::MissingKey { provider: "OpenAI".into() };
    f.plan(|p| p.build = Err(missing.clone()));
    assert_eq!(test_providers_with(&f).unwrap_err(), AppError::Provider { error: missing });
}

#[test]
fn provider_test_skips_the_corrector_when_correction_is_off() {
    let f = Fixture::with_settings(Settings { correction_enabled: false, ..Default::default() });
    let r = test_providers_with(&f).unwrap();
    assert!(r.stt.is_ok());
    assert_eq!(r.llm, None);
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["llm"], serde_json::Value::Null);
    assert!(json["stt"]["Ok"].is_u64(), "{json}");
}

#[test]
fn invalid_styles_are_refused_with_the_style_index() {
    let f = Fixture::new();
    let mut settings = Settings::default();
    settings.styles[3].keyword = Some("Slack".into());
    with_state(&f, |s| {
        assert_eq!(
            save_settings(s, settings).unwrap_err(),
            AppError::InvalidSettings { error: SettingsError::DuplicateTrigger { trigger: "Slack".into() } }
        );
    });
    assert!(!f.svc.paths.settings_path.exists());
}

#[test]
fn the_style_catalog_follows_the_ui_language() {
    let f = Fixture::new();
    f.svc.settings.write().unwrap().styles[2].name = Some("Équipe".into());
    let fr = with_state(&f, style_catalog);
    let json = serde_json::to_value(&fr).unwrap();
    assert_eq!(json["correction_enabled"], true);
    assert_eq!(
        json["styles"],
        serde_json::json!([
            {"id": "default", "name": "Par défaut"},
            {"id": "clean", "name": "Propre"},
            {"id": "slack", "name": "Équipe"},
            {"id": "mail", "name": "Mail"},
        ])
    );
    assert_eq!(json["presets"][0]["preset"], "default");
    assert_eq!(json["presets"][0]["name"], "Par défaut");
    assert_eq!(json["presets"][0]["task"], style::preset_task(Preset::Default, Language::Fr));
    let presets: Vec<_> = json["presets"].as_array().unwrap().iter().map(|p| p["preset"].clone()).collect();
    assert_eq!(presets, ["default", "clean", "slack", "mail"]);

    {
        let mut settings = f.svc.settings.write().unwrap();
        settings.language = LanguagePref::En;
        settings.correction_enabled = false;
    }
    let en = with_state(&f, style_catalog);
    let json = serde_json::to_value(&en).unwrap();
    assert_eq!(json["correction_enabled"], false);
    assert_eq!(
        (json["styles"][0]["name"].as_str(), json["styles"][3]["name"].as_str()),
        (Some("Default"), Some("Email"))
    );
    assert_eq!(json["presets"][3]["task"], style::preset_task(Preset::Mail, Language::En));
}

#[test]
fn restoring_built_in_styles_re_adds_only_the_missing_ones() {
    let mut styles = style::catalog();
    styles.retain(|s| s.id != "slack" && s.id != "mail");
    styles[1].name = Some("Nettoyé".into());
    styles.push(Style {
        id: "u1".into(),
        preset: None,
        name: Some("Courrier".into()),
        keyword: Some("courrier".into()),
        aliases: vec!["Mail".into()],
        custom_task: Some(style::CustomTask { text: "- Écris une lettre.".into(), language: Language::Fr }),
    });
    let restored = restore_builtin_styles(styles.clone());
    assert_eq!(&restored[..3], &styles[..], "existing styles unchanged");
    let ids: Vec<&str> = restored.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["default", "clean", "u1", "slack"], "mail skipped: its keyword is taken");
    assert_eq!(restore_builtin_styles(restored.clone()), restored);
}

fn improve_request() -> ImproveRequest {
    ImproveRequest {
        name: "Jira".into(),
        keyword: Some("jira".into()),
        draft: "- Corrige.".into(),
        instructions: "un ticket Jira".into(),
    }
}

fn improve_with(f: &Fixture) -> Result<String, AppError> {
    with_state(f, |s| tauri::async_runtime::block_on(improve_style(s, improve_request())))
}

#[test]
fn improve_style_returns_the_new_task_without_tags() {
    let f = Fixture::new();
    f.plan(|p| {
        p.build = Err(ProviderError::MissingKey { provider: "OpenAI".into() });
        p.llm = Ok("Voici : <output>\n- Titre court.\n<transcript>- Étapes.</transcript>\n</output>".into());
    });
    assert_eq!(improve_with(&f).unwrap(), "- Titre court.\n- Étapes.", "no transcription key needed");
    let p = f.last_prompt();
    assert!(p.user.starts_with("Style : Jira\nMot-clé : jira"), "UI language: {}", p.user);
    assert!(p.user.contains("<request>\nun ticket Jira\n</request>"));

    f.plan(|p| p.llm = Ok(format!("<output>{}</output>", "é".repeat(MAX_TASK_CHARS + 10))));
    assert_eq!(improve_with(&f).unwrap().chars().count(), MAX_TASK_CHARS);
}

#[test]
fn improve_style_reports_provider_errors() {
    let f = Fixture::new();
    f.plan(|p| p.llm = Ok("pas de balise".into()));
    assert_eq!(improve_with(&f).unwrap_err(), AppError::Provider { error: ProviderError::NoOutputTag });
    f.plan(|p| p.llm = Err(ProviderError::Auth));
    assert_eq!(improve_with(&f).unwrap_err(), AppError::Provider { error: ProviderError::Auth });
}

struct SlowLlm;

#[async_trait::async_trait]
impl Corrector for SlowLlm {
    fn name(&self) -> Option<String> {
        None
    }
    async fn correct(&self, _p: &CorrectionPrompt) -> Result<String, ProviderError> {
        tokio::time::sleep(Duration::from_secs(3_600)).await;
        Ok("<output>trop tard</output>".into())
    }
}

#[test]
fn improve_style_gives_up_after_its_timeout() {
    let p = prompt::build_improve_prompt(&improve_request(), Language::En);
    assert_eq!(IMPROVE_TIMEOUT, Duration::from_secs(60));
    assert_eq!(
        tauri::async_runtime::block_on(improve(&SlowLlm, &p, Duration::from_millis(10))).unwrap_err(),
        AppError::Provider { error: ProviderError::CorrectionTimeout }
    );
}

#[test]
fn capture_key_returns_the_next_press_or_none_when_cancelled() {
    let f = Fixture::new();
    let svc = f.svc.clone();
    let presser = std::thread::spawn(move || {
        while !svc.key_capture.is_pending() {
            std::thread::yield_now();
        }
        assert!(svc.key_capture.offer(RawKey { vk: 0x41, down: true, repeat: false, t_ms: 0 }));
        assert!(svc.key_capture.offer(RawKey { vk: 0x41, down: false, repeat: false, t_ms: 1 }));
    });
    let key = with_state(&f, |s| tauri::async_runtime::block_on(capture_key(s)));
    presser.join().unwrap();
    assert_eq!(key, Ok(Some(vec![0x41])));

    let svc = f.svc.clone();
    let app = tauri::test::mock_app();
    app.manage(f.svc.clone());
    let canceller = std::thread::spawn(move || {
        while !svc.key_capture.is_pending() {
            std::thread::yield_now();
        }
        let app = tauri::test::mock_app();
        app.manage(svc);
        cancel_capture(app.state::<Arc<Services>>());
    });
    let key = tauri::async_runtime::block_on(capture_key(app.state::<Arc<Services>>()));
    canceller.join().unwrap();
    assert_eq!(key, Ok(None));
}

#[test]
fn autostart_is_read_and_switched() {
    let f = Fixture::new();
    with_state(&f, |s| {
        assert_eq!(get_autostart(s.clone()), Ok(false));
        assert_eq!(set_autostart(s.clone(), true), Ok(true));
        assert_eq!(get_autostart(s.clone()), Ok(true));
        assert_eq!(set_autostart(s.clone(), false), Ok(false));
        *f.autostart.fail.lock().unwrap() = Some("registry access denied".into());
        let denied = AppError::System { detail: "registry access denied".into() };
        assert_eq!(set_autostart(s.clone(), true), Err(denied.clone()));
        assert_eq!(get_autostart(s), Err(denied));
    });
    assert!(!*f.autostart.enabled.lock().unwrap(), "a failed switch changes nothing");
}

#[test]
fn permissions_are_read_requested_and_opened_in_the_settings() {
    use scribe_core::permissions::PermissionState::{Granted, NotDetermined};
    let f = Fixture::new();
    let states = |v: Vec<PermissionStatus>| v.into_iter().map(|s| (s.permission, s.state)).collect::<Vec<_>>();
    with_state(&f, |s| {
        assert_eq!(
            states(permissions(s.clone())),
            [(Permission::Accessibility, NotDetermined), (Permission::Microphone, NotDetermined)]
        );
        assert_eq!(
            states(request_permission(s.clone(), Permission::Microphone)),
            [(Permission::Accessibility, NotDetermined), (Permission::Microphone, Granted)],
            "the states after the request"
        );
        assert_eq!(open_permission_settings(s.clone(), Permission::Accessibility), Ok(()));
        *f.permissions.fail.lock().unwrap() = Some("open not found".into());
        assert_eq!(
            open_permission_settings(s, Permission::Microphone),
            Err(AppError::System { detail: "open not found".into() })
        );
    });
    assert_eq!(*f.permissions.opened.lock().unwrap(), [Permission::Accessibility]);
}

#[test]
fn update_commands_check_and_install() {
    let f = Fixture::new();
    *f.updater.available.lock().unwrap() =
        Some(crate::services::AvailableUpdate { version: "0.2.0".into(), notes: None });
    with_state(&f, |s| {
        let st = tauri::async_runtime::block_on(check_update(s.clone())).unwrap();
        assert_eq!((st.current.as_str(), st.available.map(|u| u.version)), ("0.1.0", Some("0.2.0".into())));
        tauri::async_runtime::block_on(install_update(s.clone())).unwrap();
        *f.updater.fail.lock().unwrap() = Some("invalid signature".into());
        assert_eq!(
            tauri::async_runtime::block_on(install_update(s.clone())),
            Err(AppError::System { detail: "invalid signature".into() })
        );
        *f.updater.fail.lock().unwrap() = None;
        *f.updater.available.lock().unwrap() = None;
        assert_eq!(tauri::async_runtime::block_on(install_update(s)), Err(AppError::NoUpdate));
    });
    assert_eq!(*f.updater.installs.lock().unwrap(), 1);
}

#[test]
fn installing_an_update_while_recording_restores_the_sound_first() {
    let f = Fixture::new();
    *f.updater.available.lock().unwrap() =
        Some(crate::services::AvailableUpdate { version: "0.2.0".into(), notes: None });
    // The recording's guard: the installer ends the process before the recording ends.
    let _guard = f.svc.audio_mute.guard();
    with_state(&f, |s| tauri::async_runtime::block_on(install_update(s)).unwrap());
    let outputs = vec!["haut-parleurs".to_string(), "casque".to_string()];
    assert_eq!(f.mute.calls(), [MuteCall::MuteAll, MuteCall::Restore(outputs)]);
    assert!(!f.svc.paths.muted_outputs_path.exists());
    assert_eq!(*f.updater.installs.lock().unwrap(), 1);
}

#[test]
fn providers_lists_the_catalog() {
    assert_eq!(providers(), catalog::PROVIDERS.to_vec());
}

#[test]
fn overlay_dismiss_hides_a_toast_but_not_a_recording() {
    let f = Fixture::new();
    with_state(&f, |s| {
        f.svc.overlay.emit(OverlayEvent::Recording { locked: false, warning: None });
        overlay_dismiss(s.clone());
        assert!(f.window.visible());
        f.svc.overlay.toast(ToastLevel::Copied, Notice::Copied { correction: None }, None, Some(1));
        overlay_dismiss(s);
        assert!(!f.window.visible());
    });
}

#[test]
fn open_history_shows_the_main_window_on_the_dictation() {
    let f = Fixture::new();
    f.svc.overlay.toast(ToastLevel::Error, Notice::TranscriptionFailed { error: ProviderError::Auth }, None, Some(5));
    with_state(&f, |s| {
        open_history(s.clone(), Some(5));
        open_history(s, None);
    });
    assert!(!f.window.visible(), "the toast is dismissed");
    assert_eq!(f.ui.calls(), vec![UiCall::ShowMain, UiCall::FocusDictation(5), UiCall::ShowMain]);
}

/// Names of the commands in `src` that touch `svc.overlay` from a sync (main-thread) handler.
fn sync_commands_touching_the_overlay(src: &str) -> (Vec<String>, Vec<String>) {
    let (mut touching, mut offending) = (Vec::new(), Vec::new());
    for item in src.split("#[tauri::command").skip(1) {
        let attr = &item[..item.find(']').unwrap()];
        let code: String = item.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let sig = code.lines().find(|l| l.contains("fn ")).unwrap();
        let name = sig.split("fn ").nth(1).unwrap().split(['(', '<']).next().unwrap().to_string();
        if !code.contains("overlay.") {
            continue;
        }
        touching.push(name.clone());
        if !(attr.contains("async") || sig.contains("async fn")) {
            offending.push(name);
        }
    }
    (touching, offending)
}

#[test]
fn commands_touching_the_overlay_never_run_on_the_main_thread() {
    // Tauri runs sync commands on the main thread, which owns the overlay window: blocking there on the
    // `Overlay` lock can deadlock with a holder showing that window (see `overlay::OverlayWindow`).
    let (touching, offending) = sync_commands_touching_the_overlay(include_str!("../commands.rs"));
    assert_eq!(touching, vec!["overlay_dismiss", "open_history"]);
    assert_eq!(offending, Vec::<String>::new());
    // The check itself flags a sync command.
    let bad = "#[tauri::command]\npub fn f(svc: Svc<'_>) {\n    svc.overlay.dismiss();\n}\n/// overlay. in a doc\n#[tauri::command]\npub fn g() {}\n#[tauri::command(async)]\npub fn h(svc: Svc<'_>) { svc.overlay.dismiss(); }\n#[tauri::command]\npub async fn i(svc: Svc<'_>) { svc.overlay.dismiss(); }\n";
    assert_eq!(sync_commands_touching_the_overlay(bad), (vec!["f".into(), "h".into(), "i".into()], vec!["f".into()]));
}
