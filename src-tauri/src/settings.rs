use std::path::Path;

use serde::{Deserialize, Serialize};

use scribe_core::gesture::GestureConfig;
use scribe_core::model::Level;
use scribe_core::pipeline::PipelineConfig;
use scribe_providers::catalog;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub trigger_vk: u32,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger_vk: 0xA3,
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
            llm_timeout_base_ms: 3_000,
            llm_timeout_per_char_ms: 5,
            hint_budget_chars: 800,
            audio_retention_days: 30,
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

impl Settings {
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => match s.validate() {
                    Ok(()) => s,
                    Err(e) => {
                        tracing::warn!("réglages invalides ({e}), valeurs par défaut utilisées");
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

    pub fn validate(&self) -> Result<(), String> {
        if self.trigger_vk == 0 {
            return Err("choisissez une touche de déclenchement".into());
        }
        if self.lock_vk == self.trigger_vk {
            return Err("la touche de verrouillage doit différer de la touche de déclenchement".into());
        }
        if self.max_recording_ms < 10_000 {
            return Err("la durée maximale doit être d'au moins 10 secondes".into());
        }
        if self.max_recording_ms > MAX_RECORDING_MS_CAP {
            return Err(format!("la durée maximale ne peut pas dépasser {} minutes", MAX_RECORDING_MS_CAP / 60_000));
        }
        if self.min_recording_ms >= self.max_recording_ms {
            return Err("la durée minimale doit être inférieure à la durée maximale".into());
        }
        if !(-90.0..=-10.0).contains(&self.silence_threshold_dbfs) {
            return Err("le seuil de silence doit être compris entre −90 et −10 dBFS".into());
        }
        if self.gesture.hold_threshold_ms == 0 || self.gesture.double_tap_window_ms == 0 {
            return Err("les délais du raccourci doivent être supérieurs à 0 ms".into());
        }
        if self.audio_retention_days > MAX_AUDIO_RETENTION_DAYS {
            return Err(format!("la durée de conservation de l'audio ne peut pas dépasser {MAX_AUDIO_RETENTION_DAYS} jours"));
        }
        if self.restore_delay_ms > MAX_RESTORE_DELAY_MS {
            return Err(format!("le délai de restauration du presse-papiers ne peut pas dépasser {MAX_RESTORE_DELAY_MS} ms"));
        }
        if self.llm_timeout_base_ms > MAX_LLM_TIMEOUT_BASE_MS || self.llm_timeout_per_char_ms > MAX_LLM_TIMEOUT_PER_CHAR_MS {
            return Err("le délai de correction est trop élevé".into());
        }
        if catalog::stt_provider(&self.stt_provider).is_none() {
            return Err(format!("fournisseur de transcription inconnu : {}", self.stt_provider));
        }
        if catalog::llm_provider(&self.llm_provider).is_none() {
            return Err(format!("fournisseur de correction inconnu : {}", self.llm_provider));
        }
        if self.stt_model.trim().is_empty() || self.llm_model.trim().is_empty() {
            return Err("choisissez un modèle de transcription et un modèle de correction".into());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_global_constraints() {
        let s = Settings::default();
        assert_eq!((s.trigger_vk, s.lock_vk), (0xA3, 0x20));
        assert_eq!((s.stt_provider.as_str(), s.stt_model.as_str()), ("openai", "gpt-transcribe"));
        assert_eq!(s.llm_provider, "anthropic");
        assert_eq!((s.llm_model.as_str(), s.llm_effort.as_str()), ("claude-opus-5-5", "low"));
        assert_eq!((s.min_recording_ms, s.max_recording_ms, s.restore_delay_ms), (300, 600_000, 150));
        assert_eq!(s.level, Level::Formatted);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings { stt_provider: "groq".into(), llm_provider: "mistral".into(), level: Level::Clean, ..Default::default() };
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
        assert_eq!(s.trigger_vk, 0xA3);
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert_eq!(Settings::load(&dir.path().join("absent.json")), Settings::default());
    }

    #[test]
    fn legacy_stt_preset_field_is_still_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"stt_preset":"groq","stt_base_url":"https://api.groq.com/openai/v1","stt_model":"whisper-large-v3"}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!((s.stt_provider.as_str(), s.stt_model.as_str()), ("groq", "whisper-large-v3"));
    }

    #[test]
    fn invalid_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for bad in [
            r#"{"stt_provider":"grok"}"#,
            r#"{"llm_provider":"groq","llm_model":""}"#,
            r#"{"trigger_vk":0}"#,
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
        assert!(Settings { trigger_vk: 0, ..Default::default() }.validate().is_err());
        assert!(Settings { lock_vk: 0xA3, ..Default::default() }.validate().is_err());
        assert!(Settings { max_recording_ms: 100, ..Default::default() }.validate().is_err());
    }

    #[test]
    fn validation_rejects_values_that_break_dictation() {
        let bad = [
            Settings { min_recording_ms: 700_000, ..Default::default() },
            Settings { min_recording_ms: 600_000, ..Default::default() },
            Settings { silence_threshold_dbfs: -1.0, ..Default::default() },
            Settings { silence_threshold_dbfs: -200.0, ..Default::default() },
            Settings { gesture: GestureConfig { hold_threshold_ms: 0, ..Default::default() }, ..Default::default() },
            Settings { gesture: GestureConfig { double_tap_window_ms: 0, ..Default::default() }, ..Default::default() },
            Settings { audio_retention_days: 100_000_000, ..Default::default() },
            Settings { stt_provider: "anthropic".into(), ..Default::default() },
            Settings { llm_provider: "inconnu".into(), ..Default::default() },
            Settings { stt_model: " ".into(), ..Default::default() },
            Settings { max_recording_ms: MAX_RECORDING_MS_CAP + 1, ..Default::default() },
            Settings { max_recording_ms: 1_800_000, ..Default::default() },
            Settings { restore_delay_ms: MAX_RESTORE_DELAY_MS + 1, ..Default::default() },
            Settings { restore_delay_ms: u64::MAX, ..Default::default() },
            Settings { llm_timeout_base_ms: u64::MAX, ..Default::default() },
            Settings { llm_timeout_per_char_ms: u64::MAX, ..Default::default() },
        ];
        for s in bad {
            assert!(s.validate().is_err(), "should be rejected: {s:?}");
        }
        let good = [
            Settings { audio_retention_days: 0, ..Default::default() },
            Settings { audio_retention_days: MAX_AUDIO_RETENTION_DAYS, ..Default::default() },
            Settings { stt_provider: "groq".into(), stt_model: "whisper-large-v3-turbo".into(), ..Default::default() },
            Settings { llm_provider: "openai".into(), llm_model: "modèle-saisi-à-la-main".into(), ..Default::default() },
            Settings { max_recording_ms: MAX_RECORDING_MS_CAP, ..Default::default() },
            Settings { restore_delay_ms: MAX_RESTORE_DELAY_MS, ..Default::default() },
            Settings { restore_delay_ms: 0, ..Default::default() },
        ];
        for s in good {
            assert!(s.validate().is_ok(), "should be accepted: {s:?}");
        }
    }
}
