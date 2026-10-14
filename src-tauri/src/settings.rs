use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use scribe_core::chord::{self, ChordError};
use scribe_core::gesture::GestureConfig;
use scribe_core::model::Level;
use scribe_core::pipeline::{PipelineConfig, ProviderRole};
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
    pub level: Level,
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
    /// Other apps get no microphone input while recording (`scribe_platform::exclusive_mic`). Off by default:
    /// a call in progress would lose its microphone.
    pub exclusive_microphone_during_dictation: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: LanguagePref::System,
            trigger_keys: vec![DEFAULT_TRIGGER],
            lock_vk: 0x20,
            gesture: GestureConfig::default(),
            level: Level::Formatted,
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
            exclusive_microphone_during_dictation: false,
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
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => match s.validate() {
                    Ok(()) => s.normalized(),
                    Err(e) => {
                        tracing::warn!("réglages invalides ({e:?}), valeurs par défaut utilisées");
                        Settings::default()
                    }
                },
                Err(e) => {
                    tracing::warn!("réglages illisibles ({e}), valeurs par défaut utilisées");
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        }
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
        Ok(())
    }

    pub fn pipeline_config(&self) -> PipelineConfig {
        PipelineConfig {
            level: self.level,
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
    use super::*;

    #[test]
    fn defaults_match_global_constraints() {
        let s = Settings::default();
        assert_eq!((s.trigger_keys.clone(), s.lock_vk), (vec![DEFAULT_TRIGGER], 0x20));
        assert_eq!((s.stt_provider.as_str(), s.stt_model.as_str()), ("openai", "gpt-transcribe"));
        assert_eq!(s.llm_provider, "anthropic");
        assert_eq!((s.llm_model.as_str(), s.llm_effort.as_str()), ("claude-opus-5-5", "low"));
        assert_eq!((s.min_recording_ms, s.max_recording_ms, s.restore_delay_ms), (300, 600_000, 150));
        assert_eq!(s.level, Level::Formatted);
        assert!(s.mute_audio_during_dictation);
        assert!(!s.exclusive_microphone_during_dictation);
    }

    #[test]
    fn exclusive_microphone_is_off_for_files_from_before_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"llm_model":"claude-haiku-4-5","mute_audio_during_dictation":true}"#).unwrap();
        assert!(!Settings::load(&path).exclusive_microphone_during_dictation);
        let on = Settings { exclusive_microphone_during_dictation: true, ..Default::default() };
        on.save(&path).unwrap();
        assert_eq!(Settings::load(&path), on);
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
            level: Level::Clean,
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
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
