use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use scribe_core::chord::{self, ChordError};
use scribe_core::gesture::GestureConfig;
use scribe_core::pipeline::{PipelineConfig, ProviderRole};
use scribe_core::style::{self, Style, StyleError};
use scribe_providers::catalog;

use crate::locale::LanguagePref;

/// Ctrl droit on Windows; Cmd droit on macOS, where most keyboards have no right Control.
pub const DEFAULT_TRIGGER: u32 = if cfg!(target_os = "macos") { 0x5C } else { 0xA3 };

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: LanguagePref,
    /// The trigger combination (`scribe_core::chord`), in canonical order. Files from before combinations
    /// hold a single `trigger_vk` number, read as a one-key combination.
    #[serde(alias = "trigger_vk", deserialize_with = "one_or_many")]
    pub trigger_keys: Vec<u32>,
    /// 0 = no lock key.
    pub lock_vk: u32,
    pub gesture: GestureConfig,
    /// Off: the raw transcript is pasted (a style keyword is still stripped).
    pub correction_enabled: bool,
    /// Default style first (`style::catalog`), then the keyword styles in detection tie-break order.
    pub styles: Vec<Style>,
    /// Catalog style ids already offered to this user: a built-in they deleted is not seeded again.
    pub offered_builtins: Vec<String>,
    /// Catalog provider id (`scribe_providers::catalog`); also the keyring entry of its API key.
    #[serde(alias = "stt_preset")]
    pub stt_provider: String,
    pub stt_model: String,
    pub llm_provider: String,
    pub llm_model: String,
    /// Ignored when the model takes no effort parameter.
    pub llm_effort: String,
    pub restore_delay_ms: u64,
    pub min_recording_ms: u64,
    pub max_recording_ms: u64,
    pub silence_threshold_dbfs: f32,
    pub llm_timeout_base_ms: u64,
    pub llm_timeout_per_char_ms: u64,
    pub hint_budget_chars: usize,
    pub audio_retention_days: u32,
    /// Mute every audio output while recording (`audio_mute`). Files from before this setting turn it on.
    pub mute_audio_during_dictation: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: LanguagePref::System,
            trigger_keys: vec![DEFAULT_TRIGGER],
            lock_vk: 0x20,
            gesture: GestureConfig::default(),
            correction_enabled: true,
            styles: style::catalog(),
            offered_builtins: style::catalog_ids(),
            stt_provider: "openai".into(),
            stt_model: "gpt-transcribe".into(),
            llm_provider: "anthropic".into(),
            llm_model: "claude-opus-5-5".into(),
            llm_effort: "low".into(),
            restore_delay_ms: 150,
            min_recording_ms: 300,
            max_recording_ms: 600_000,
            silence_threshold_dbfs: -45.0,
            llm_timeout_base_ms: 5_000,
            llm_timeout_per_char_ms: 5,
            hint_budget_chars: 800,
            audio_retention_days: 30,
            mute_audio_during_dictation: true,
        }
    }
}

/// Upload cap: 16 kHz mono 16-bit WAV is 1.92 MB/min, and the transcription APIs
/// (OpenAI, Groq) reject files above 25 MB (~13 min). 10 min keeps a safe margin, so a
/// long dictation (or its retranscription from the history) is never refused for size.
pub const MAX_RECORDING_MS_CAP: u64 = 600_000;
/// The paste path sleeps this long before ProcessingDone; the session ignores hotkeys meanwhile.
pub const MAX_RESTORE_DELAY_MS: u64 = 2_000;
/// LLM timeout bounds: keep `base + per_char * len` reasonable and far from u64 overflow.
pub const MAX_LLM_TIMEOUT_BASE_MS: u64 = 60_000;
pub const MAX_LLM_TIMEOUT_PER_CHAR_MS: u64 = 1_000;

/// About 100 years: anything above is a typo, and would overflow date arithmetic.
pub const MAX_AUDIO_RETENTION_DAYS: u32 = 36_500;
pub const MIN_MAX_RECORDING_MS: u64 = 10_000;
pub const SILENCE_THRESHOLD_RANGE_DBFS: (i32, i32) = (-90, -10);

/// Why `validate` refuses the settings; the UI translates the code (`src/lib/api.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SettingsError {
    InvalidHotkey,
    EscapeInHotkey,
    LockKeyInHotkey,
    MaxRecordingTooShort { min_seconds: u64 },
    MaxRecordingTooLong { max_minutes: u64 },
    MinRecordingNotBelowMax,
    SilenceThresholdOutOfRange { min_dbfs: i32, max_dbfs: i32 },
    HotkeyDelayZero,
    AudioRetentionTooLong { max_days: u32 },
    RestoreDelayTooLong { max_ms: u64 },
    CorrectionTimeoutTooLong,
    UnknownProvider { role: ProviderRole, id: String },
    ModelRequired,
    DefaultStyleMissing,
    DuplicateStyleId,
    TooManyStyles { max: usize },
    TooManyAliases { index: usize, max: usize },
    StyleNameRequired { index: usize },
    TriggerEmpty { index: usize },
    DuplicateTrigger { trigger: String },
    TaskEmpty { index: usize },
    TaskTooLong { index: usize, max_chars: usize },
}

impl From<StyleError> for SettingsError {
    fn from(e: StyleError) -> Self {
        match e {
            StyleError::DefaultStyleMissing => SettingsError::DefaultStyleMissing,
            StyleError::DuplicateStyleId => SettingsError::DuplicateStyleId,
            StyleError::TooManyStyles { max } => SettingsError::TooManyStyles { max },
            StyleError::TooManyAliases { index, max } => SettingsError::TooManyAliases { index, max },
            StyleError::StyleNameRequired { index } => SettingsError::StyleNameRequired { index },
            StyleError::TriggerEmpty { index } => SettingsError::TriggerEmpty { index },
            StyleError::DuplicateTrigger { trigger } => SettingsError::DuplicateTrigger { trigger },
            StyleError::TaskEmpty { index } => SettingsError::TaskEmpty { index },
            StyleError::TaskTooLong { index, max_chars } => SettingsError::TaskTooLong { index, max_chars },
        }
    }
}

/// The correction level of files from before styles (`"level"`), read once by `load`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LegacyLevel {
    Raw,
    Clean,
    Formatted,
}

impl From<ChordError> for SettingsError {
    fn from(e: ChordError) -> Self {
        match e {
            ChordError::Invalid => SettingsError::InvalidHotkey,
            ChordError::Escape => SettingsError::EscapeInHotkey,
            ChordError::LockKey => SettingsError::LockKeyInHotkey,
        }
    }
}

impl Settings {
    /// Reads the file, migrates a legacy `level`, seeds the catalog styles never offered, drops the styles that
    /// break a rule, then falls back to the defaults when another setting is invalid.
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Settings::default();
        };
        let parsed = serde_json::from_str::<serde_json::Value>(&text).and_then(|value| {
            let legacy = value.get("level").and_then(|l| LegacyLevel::deserialize(l).ok());
            let had_styles = value.get("styles").is_some();
            Ok(serde_json::from_value::<Settings>(value)?.migrated(legacy, had_styles))
        });
        let s = match parsed {
            Ok(s) => s.seeded().repaired(),
            Err(e) => {
                tracing::warn!("réglages illisibles ({e}), valeurs par défaut utilisées");
                return Settings::default();
            }
        };
        match s.validate() {
            Ok(()) => s.normalized(),
            Err(e) => {
                tracing::warn!("réglages invalides ({e:?}), valeurs par défaut utilisées");
                Settings::default()
            }
        }
    }

    /// `raw` turns the correction off; `clean` gives the default style the Clean task, unless the file already
    /// has styles (never overwritten).
    fn migrated(mut self, legacy: Option<LegacyLevel>, had_styles: bool) -> Self {
        match legacy {
            Some(LegacyLevel::Raw) => self.correction_enabled = false,
            Some(LegacyLevel::Clean) if !had_styles => {
                for s in self.styles.iter_mut().filter(|s| s.id == style::DEFAULT_STYLE_ID) {
                    s.preset = Some(style::Preset::Clean);
                }
            }
            _ => {}
        }
        self
    }

    /// Appends the catalog styles shipped since the last load, once.
    fn seeded(mut self) -> Self {
        let new: Vec<String> =
            style::catalog_ids().into_iter().filter(|id| !self.offered_builtins.contains(id)).collect();
        let ids: Vec<&str> = new.iter().map(String::as_str).collect();
        let (styles, skipped) = style::with_builtins(std::mem::take(&mut self.styles), &ids);
        if !skipped.is_empty() {
            tracing::warn!("styles intégrés non ajoutés (id ou mot-clé déjà pris) : {skipped:?}");
        }
        self.styles = styles;
        self.offered_builtins.extend(new);
        self
    }

    /// Drops each style that breaks a rule, so one bad style never costs the other settings.
    fn repaired(mut self) -> Self {
        let (styles, errors) = style::repaired(std::mem::take(&mut self.styles));
        for e in errors {
            tracing::warn!("style ignoré : {e}");
        }
        self.styles = styles;
        self
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).expect("serialize settings"))?;
        std::fs::rename(tmp, path)
    }

    /// Canonical form, applied after `validate`: the trigger keys in `chord::normalize` order.
    pub fn normalized(mut self) -> Self {
        self.trigger_keys = chord::normalize(&self.trigger_keys);
        self
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        chord::validate(&self.trigger_keys, self.lock_vk)?;
        if self.max_recording_ms < MIN_MAX_RECORDING_MS {
            return Err(SettingsError::MaxRecordingTooShort { min_seconds: MIN_MAX_RECORDING_MS / 1_000 });
        }
        if self.max_recording_ms > MAX_RECORDING_MS_CAP {
            return Err(SettingsError::MaxRecordingTooLong { max_minutes: MAX_RECORDING_MS_CAP / 60_000 });
        }
        if self.min_recording_ms >= self.max_recording_ms {
            return Err(SettingsError::MinRecordingNotBelowMax);
        }
        let (min_dbfs, max_dbfs) = SILENCE_THRESHOLD_RANGE_DBFS;
        if !(min_dbfs as f32..=max_dbfs as f32).contains(&self.silence_threshold_dbfs) {
            return Err(SettingsError::SilenceThresholdOutOfRange { min_dbfs, max_dbfs });
        }
        if self.gesture.hold_threshold_ms == 0 || self.gesture.double_tap_window_ms == 0 {
            return Err(SettingsError::HotkeyDelayZero);
        }
        if self.audio_retention_days > MAX_AUDIO_RETENTION_DAYS {
            return Err(SettingsError::AudioRetentionTooLong { max_days: MAX_AUDIO_RETENTION_DAYS });
        }
        if self.restore_delay_ms > MAX_RESTORE_DELAY_MS {
            return Err(SettingsError::RestoreDelayTooLong { max_ms: MAX_RESTORE_DELAY_MS });
        }
        if self.llm_timeout_base_ms > MAX_LLM_TIMEOUT_BASE_MS
            || self.llm_timeout_per_char_ms > MAX_LLM_TIMEOUT_PER_CHAR_MS
        {
            return Err(SettingsError::CorrectionTimeoutTooLong);
        }
        if catalog::stt_provider(&self.stt_provider).is_none() {
            return Err(SettingsError::UnknownProvider {
                role: ProviderRole::Transcription,
                id: self.stt_provider.clone(),
            });
        }
        if catalog::llm_provider(&self.llm_provider).is_none() {
            return Err(SettingsError::UnknownProvider {
                role: ProviderRole::Correction,
                id: self.llm_provider.clone(),
            });
        }
        if self.stt_model.trim().is_empty() || self.llm_model.trim().is_empty() {
            return Err(SettingsError::ModelRequired);
        }
        style::validate(&self.styles)?;
        Ok(())
    }

    pub fn pipeline_config(&self) -> PipelineConfig {
        PipelineConfig {
            correction_enabled: self.correction_enabled,
            hint_budget_chars: self.hint_budget_chars,
            llm_timeout_base_ms: self.llm_timeout_base_ms,
            llm_timeout_per_char_ms: self.llm_timeout_per_char_ms,
            ..PipelineConfig::default()
        }
    }
}

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(u32),
        Many(Vec<u32>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(vk) => vec![vk],
        OneOrMany::Many(keys) => keys,
    })
}

#[cfg(test)]
mod tests {
    use scribe_core::lang::Language;
    use scribe_core::style::{CustomTask, Preset};

    use super::*;

    #[test]
    fn defaults_match_global_constraints() {
        let s = Settings::default();
        assert_eq!((s.trigger_keys.clone(), s.lock_vk), (vec![DEFAULT_TRIGGER], 0x20));
        assert_eq!((s.stt_provider.as_str(), s.stt_model.as_str()), ("openai", "gpt-transcribe"));
        assert_eq!(s.llm_provider, "anthropic");
        assert_eq!((s.llm_model.as_str(), s.llm_effort.as_str()), ("claude-opus-5-5", "low"));
        assert_eq!((s.min_recording_ms, s.max_recording_ms, s.restore_delay_ms), (300, 600_000, 150));
        assert!(s.correction_enabled);
        assert_eq!(s.styles, style::catalog());
        assert_eq!(s.offered_builtins, ["default", "clean", "slack", "mail"]);
        assert!(s.mute_audio_during_dictation);
    }

    #[test]
    fn files_with_the_removed_exclusive_microphone_setting_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"llm_model":"claude-haiku-4-5","exclusive_microphone_during_dictation":true}"#)
            .unwrap();
        assert_eq!(Settings::load(&path).llm_model, "claude-haiku-4-5");
    }

    #[test]
    fn muting_during_dictation_is_on_for_files_from_before_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"llm_model":"claude-haiku-4-5"}"#).unwrap();
        assert!(Settings::load(&path).mute_audio_during_dictation);
        let off = Settings { mute_audio_during_dictation: false, ..Default::default() };
        off.save(&path).unwrap();
        assert_eq!(Settings::load(&path), off);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings {
            stt_provider: "groq".into(),
            llm_provider: "mistral".into(),
            correction_enabled: false,
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    fn load_json(json: &str) -> Settings {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, json).unwrap();
        Settings::load(&path)
    }

    fn user_style(id: &str, keyword: &str) -> Style {
        Style {
            id: id.into(),
            preset: None,
            name: Some(id.to_uppercase()),
            keyword: Some(keyword.into()),
            aliases: vec![],
            custom_task: Some(CustomTask { text: format!("- Style {id}."), language: Language::Fr }),
        }
    }

    fn ids(s: &Settings) -> Vec<&str> {
        s.styles.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn the_legacy_level_is_migrated() {
        let raw = load_json(r#"{"level":"raw"}"#);
        assert!(!raw.correction_enabled);
        assert_eq!(raw.styles, style::catalog());
        let clean = load_json(r#"{"level":"clean"}"#);
        assert!(clean.correction_enabled);
        assert_eq!(clean.styles[0].preset, Some(Preset::Clean), "the default style runs the Clean task");
        assert_eq!(clean.styles[1..], style::catalog()[1..]);
        let formatted = load_json(r#"{"level":"formatted"}"#);
        assert_eq!(formatted, Settings::default());
        assert_eq!(load_json(r#"{"level":"unknown"}"#), Settings::default(), "an unknown level is ignored");
    }

    #[test]
    fn a_migrated_file_reloads_identically_without_the_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for level in ["raw", "clean", "formatted"] {
            std::fs::write(&path, format!(r#"{{"level":"{level}","llm_model":"claude-haiku-4-5"}}"#)).unwrap();
            let first = Settings::load(&path);
            first.save(&path).unwrap();
            let json = std::fs::read_to_string(&path).unwrap();
            assert!(!json.contains("\"level\"") && json.contains("\"correction_enabled\""), "{json}");
            assert_eq!(Settings::load(&path), first, "{level}");
        }
    }

    #[test]
    fn a_legacy_clean_level_never_overwrites_saved_styles() {
        let mut styles = style::catalog();
        styles[0].custom_task = Some(CustomTask { text: "- Ma tâche.".into(), language: Language::Fr });
        let json = serde_json::json!({"level": "clean", "styles": styles}).to_string();
        let s = load_json(&json);
        assert_eq!(s.styles, styles);
        assert_eq!(s.styles[0].preset, Some(Preset::Default));
    }

    #[test]
    fn a_new_catalog_style_is_seeded_once() {
        let mut styles = style::catalog();
        styles.truncate(3);
        let json = serde_json::json!({"styles": styles, "offered_builtins": ["default", "clean", "slack"]});
        let s = load_json(&json.to_string());
        assert_eq!(ids(&s), ["default", "clean", "slack", "mail"], "shipped since the last load");
        assert_eq!(s.offered_builtins, ["default", "clean", "slack", "mail"]);

        let json = serde_json::json!({"styles": styles, "offered_builtins": ["default", "clean", "slack", "mail"]});
        assert_eq!(ids(&load_json(&json.to_string())), ["default", "clean", "slack"], "deleted: not re-added");
    }

    #[test]
    fn a_seeded_style_whose_keyword_is_taken_is_skipped() {
        let mut styles = style::catalog();
        styles.truncate(3);
        styles.push(user_style("u1", "Mail"));
        let json = serde_json::json!({"styles": styles, "offered_builtins": ["default", "clean", "slack"]});
        let s = load_json(&json.to_string());
        assert_eq!(ids(&s), ["default", "clean", "slack", "u1"]);
        assert_eq!(s.offered_builtins, ["default", "clean", "slack", "mail"], "offered once, even when skipped");
    }

    #[test]
    fn a_broken_style_costs_only_itself() {
        let mut styles = style::catalog();
        styles.push(user_style("u1", "slack"));
        styles.push(user_style("u2", "jira"));
        let json = serde_json::json!({"styles": styles, "llm_model": "claude-haiku-4-5", "trigger_keys": [162, 65]});
        let s = load_json(&json.to_string());
        assert_eq!(ids(&s), ["default", "clean", "slack", "mail", "u2"], "the later duplicate is dropped");
        assert_eq!((s.llm_model.as_str(), s.trigger_keys.clone()), ("claude-haiku-4-5", vec![0xA2, 0x41]));

        let s = load_json(r#"{"styles":[],"llm_model":"claude-haiku-4-5"}"#);
        assert_eq!(ids(&s), ["default"], "the default style is put back");
        assert_eq!(s.llm_model, "claude-haiku-4-5");

        let mut styles: Vec<Style> =
            (0..style::MAX_STYLES).map(|i| user_style(&format!("u{i}"), &format!("k{i}"))).collect();
        styles.push(style::catalog().remove(0));
        let json = serde_json::json!({"styles": styles, "llm_model": "claude-haiku-4-5"});
        let s = load_json(&json.to_string());
        assert_eq!((ids(&s).len(), ids(&s)[0]), (style::MAX_STYLES, "default"), "the last user style is dropped");
        assert_eq!(s.llm_model, "claude-haiku-4-5");
    }

    #[test]
    fn style_rules_are_part_of_the_validation() {
        let mut s = Settings::default();
        s.styles[2].keyword = Some("Mail".into());
        assert_eq!(s.validate(), Err(SettingsError::DuplicateTrigger { trigger: "mail".into() }), "the later trigger");
        s.styles = vec![];
        assert_eq!(s.validate(), Err(SettingsError::DefaultStyleMissing));
    }

    #[test]
    fn style_errors_keep_their_code_and_index() {
        use serde_json::json;
        let cases = [
            (StyleError::DefaultStyleMissing, json!({"code": "default_style_missing"})),
            (StyleError::DuplicateStyleId, json!({"code": "duplicate_style_id"})),
            (StyleError::TooManyStyles { max: 10 }, json!({"code": "too_many_styles", "max": 10})),
            (
                StyleError::TooManyAliases { index: 2, max: 8 },
                json!({"code": "too_many_aliases", "index": 2, "max": 8}),
            ),
            (StyleError::StyleNameRequired { index: 4 }, json!({"code": "style_name_required", "index": 4})),
            (StyleError::TriggerEmpty { index: 1 }, json!({"code": "trigger_empty", "index": 1})),
            (
                StyleError::DuplicateTrigger { trigger: "Slack".into() },
                json!({"code": "duplicate_trigger", "trigger": "Slack"}),
            ),
            (StyleError::TaskEmpty { index: 3 }, json!({"code": "task_empty", "index": 3})),
            (
                StyleError::TaskTooLong { index: 5, max_chars: 4000 },
                json!({"code": "task_too_long", "index": 5, "max_chars": 4000}),
            ),
        ];
        for (e, expected) in cases {
            assert_eq!(serde_json::to_value(SettingsError::from(e.clone())).unwrap(), expected, "{e:?}");
            assert_eq!(serde_json::to_value(&e).unwrap(), expected, "same code as scribe_core");
        }
    }

    #[test]
    fn partial_or_corrupt_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"llm_model":"claude-haiku-4-5"}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.llm_model, "claude-haiku-4-5");
        assert_eq!(s.trigger_keys, vec![DEFAULT_TRIGGER]);
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert_eq!(Settings::load(&dir.path().join("absent.json")), Settings::default());
    }

    #[test]
    fn legacy_stt_preset_field_is_still_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"stt_preset":"groq","stt_base_url":"https://api.groq.com/openai/v1","stt_model":"whisper-large-v3"}"#,
        )
        .unwrap();
        let s = Settings::load(&path);
        assert_eq!((s.stt_provider.as_str(), s.stt_model.as_str()), ("groq", "whisper-large-v3"));
    }

    #[test]
    fn legacy_single_trigger_key_is_read_as_a_one_key_combination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"trigger_vk":162,"lock_vk":0}"#).unwrap();
        assert_eq!(Settings::load(&path).trigger_keys, vec![0xA2]);
        let s = Settings { trigger_keys: vec![0xA2, 0xA0, 0x41], ..Default::default() };
        s.save(&path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"trigger_keys\"") && !json.contains("trigger_vk"), "{json}");
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn trigger_keys_are_put_in_canonical_order() {
        let s = Settings { trigger_keys: vec![0x41, 0xA3, 0xA0], ..Default::default() }.normalized();
        assert_eq!(s.trigger_keys, vec![0xA2, 0xA0, 0x41]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"trigger_keys":[65,163]}"#).unwrap();
        assert_eq!(Settings::load(&path).trigger_keys, vec![0xA2, 0x41]);
    }

    #[test]
    fn the_correction_gets_five_seconds_by_default() {
        // gpt-6-astra often answers in 2 to 3.3 s: a 3 s base timed out and pasted the raw text after the wait anyway.
        assert_eq!(Settings::default().pipeline_config().llm_timeout_base_ms, 5_000);
    }

    #[test]
    fn invalid_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for bad in [
            r#"{"stt_provider":"grok"}"#,
            r#"{"llm_provider":"groq","llm_model":""}"#,
            r#"{"trigger_vk":0}"#,
            r#"{"trigger_keys":[]}"#,
            r#"{"trigger_keys":"A"}"#,
            r#"{"trigger_keys":[162,160,164,91,65]}"#,
            r#"{"max_recording_ms":1800000}"#,
            r#"{"llm_timeout_per_char_ms":18446744073709551615}"#,
        ] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(Settings::load(&path), Settings::default(), "should fall back: {bad}");
        }
    }

    #[test]
    fn validation_rejects_inconsistent_keys() {
        assert!(Settings::default().validate().is_ok());
        assert!(Settings { trigger_keys: vec![], ..Default::default() }.validate().is_err());
        assert!(Settings { lock_vk: DEFAULT_TRIGGER, ..Default::default() }.validate().is_err());
        assert_eq!(
            Settings { trigger_keys: vec![0xA2, 0x20], ..Default::default() }.validate(),
            Err(SettingsError::LockKeyInHotkey)
        );
        assert_eq!(
            Settings { trigger_keys: vec![0xA2, 0x1B], ..Default::default() }.validate(),
            Err(SettingsError::EscapeInHotkey)
        );
        assert_eq!(
            Settings { trigger_keys: vec![], ..Default::default() }.validate(),
            Err(SettingsError::InvalidHotkey)
        );
        assert!(Settings { trigger_keys: vec![0xA2, 0x20], lock_vk: 0, ..Default::default() }.validate().is_ok());
        assert!(Settings { max_recording_ms: 100, ..Default::default() }.validate().is_err());
    }

    fn silence_range() -> SettingsError {
        SettingsError::SilenceThresholdOutOfRange { min_dbfs: -90, max_dbfs: -10 }
    }

    #[test]
    fn validation_rejects_values_that_break_dictation() {
        use SettingsError::*;
        let unknown = |role, id: &str| UnknownProvider { role, id: id.into() };
        let bad = [
            (Settings { min_recording_ms: 700_000, ..Default::default() }, MinRecordingNotBelowMax),
            (Settings { min_recording_ms: 600_000, ..Default::default() }, MinRecordingNotBelowMax),
            (Settings { max_recording_ms: 9_999, ..Default::default() }, MaxRecordingTooShort { min_seconds: 10 }),
            (Settings { silence_threshold_dbfs: -1.0, ..Default::default() }, silence_range()),
            (Settings { silence_threshold_dbfs: -200.0, ..Default::default() }, silence_range()),
            (
                Settings {
                    gesture: GestureConfig { hold_threshold_ms: 0, ..Default::default() },
                    ..Default::default()
                },
                HotkeyDelayZero,
            ),
            (
                Settings {
                    gesture: GestureConfig { double_tap_window_ms: 0, ..Default::default() },
                    ..Default::default()
                },
                HotkeyDelayZero,
            ),
            (
                Settings { audio_retention_days: 100_000_000, ..Default::default() },
                AudioRetentionTooLong { max_days: 36_500 },
            ),
            (
                Settings { stt_provider: "anthropic".into(), ..Default::default() },
                unknown(ProviderRole::Transcription, "anthropic"),
            ),
            (
                Settings { llm_provider: "inconnu".into(), ..Default::default() },
                unknown(ProviderRole::Correction, "inconnu"),
            ),
            (Settings { stt_model: " ".into(), ..Default::default() }, ModelRequired),
            (Settings { llm_model: "".into(), ..Default::default() }, ModelRequired),
            (
                Settings { max_recording_ms: MAX_RECORDING_MS_CAP + 1, ..Default::default() },
                MaxRecordingTooLong { max_minutes: 10 },
            ),
            (Settings { max_recording_ms: 1_800_000, ..Default::default() }, MaxRecordingTooLong { max_minutes: 10 }),
            (
                Settings { restore_delay_ms: MAX_RESTORE_DELAY_MS + 1, ..Default::default() },
                RestoreDelayTooLong { max_ms: 2_000 },
            ),
            (Settings { restore_delay_ms: u64::MAX, ..Default::default() }, RestoreDelayTooLong { max_ms: 2_000 }),
            (Settings { llm_timeout_base_ms: u64::MAX, ..Default::default() }, CorrectionTimeoutTooLong),
            (Settings { llm_timeout_per_char_ms: u64::MAX, ..Default::default() }, CorrectionTimeoutTooLong),
        ];
        for (s, expected) in bad {
            assert_eq!(s.validate(), Err(expected), "{s:?}");
        }
        let good = [
            Settings { audio_retention_days: 0, ..Default::default() },
            Settings { audio_retention_days: MAX_AUDIO_RETENTION_DAYS, ..Default::default() },
            Settings { stt_provider: "groq".into(), stt_model: "whisper-large-v3-turbo".into(), ..Default::default() },
            Settings {
                llm_provider: "openai".into(),
                llm_model: "modèle-saisi-à-la-main".into(),
                ..Default::default()
            },
            Settings { max_recording_ms: MAX_RECORDING_MS_CAP, ..Default::default() },
            Settings { restore_delay_ms: MAX_RESTORE_DELAY_MS, ..Default::default() },
            Settings { restore_delay_ms: 0, ..Default::default() },
        ];
        for s in good {
            assert!(s.validate().is_ok(), "should be accepted: {s:?}");
        }
    }

    #[test]
    fn settings_errors_serialize_as_codes_for_the_ui() {
        use serde_json::json;
        let cases = [
            (SettingsError::InvalidHotkey, json!({"code": "invalid_hotkey"})),
            (SettingsError::LockKeyInHotkey, json!({"code": "lock_key_in_hotkey"})),
            (
                SettingsError::MaxRecordingTooShort { min_seconds: 10 },
                json!({"code": "max_recording_too_short", "min_seconds": 10}),
            ),
            (silence_range(), json!({"code": "silence_threshold_out_of_range", "min_dbfs": -90, "max_dbfs": -10})),
            (
                SettingsError::UnknownProvider { role: ProviderRole::Correction, id: "x".into() },
                json!({"code": "unknown_provider", "role": "correction", "id": "x"}),
            ),
            (SettingsError::ModelRequired, json!({"code": "model_required"})),
        ];
        for (e, expected) in cases {
            assert_eq!(serde_json::to_value(&e).unwrap(), expected);
        }
    }

    #[test]
    fn the_language_follows_the_system_unless_chosen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"llm_model":"claude-haiku-4-5"}"#).unwrap();
        assert_eq!(Settings::load(&path).language, LanguagePref::System, "file from before the setting");
        let fr = Settings { language: LanguagePref::Fr, ..Default::default() };
        fr.save(&path).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains(r#""language": "fr""#));
        assert_eq!(Settings::load(&path), fr);
    }
}
