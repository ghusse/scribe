use std::path::Path;

use serde::{Deserialize, Serialize};

use scribe_core::gesture::GestureConfig;
use scribe_core::model::Level;
use scribe_core::pipeline::PipelineConfig;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub trigger_vk: u32,
    /// 0 = no lock key.
    pub lock_vk: u32,
    pub gesture: GestureConfig,
    pub level: Level,
    pub stt_preset: String,
    pub stt_base_url: String,
    pub stt_model: String,
    pub llm_model: String,
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
            stt_preset: "openai".into(),
            stt_base_url: "https://api.openai.com/v1".into(),
            stt_model: "gpt-4o-transcribe".into(),
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

/// About 100 years: anything above is a typo, and would overflow date arithmetic.
pub const MAX_AUDIO_RETENTION_DAYS: u32 = 36_500;

/// The API key travels with every request: refuse plain HTTP except to the local machine.
fn is_secure_base_url(url: &str) -> bool {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        return !rest.is_empty();
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = match host.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or(""),
            None => host.split(':').next().unwrap_or(""),
        };
        return matches!(host, "localhost" | "127.0.0.1" | "::1");
    }
    false
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("réglages illisibles ({e}), valeurs par défaut utilisées");
                Settings::default()
            }),
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
        if !is_secure_base_url(&self.stt_base_url) {
            return Err("l'adresse du service de transcription doit commencer par https:// (http:// accepté seulement pour localhost)".into());
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
        assert_eq!((s.stt_preset.as_str(), s.stt_model.as_str()), ("openai", "gpt-4o-transcribe"));
        assert_eq!((s.llm_model.as_str(), s.llm_effort.as_str()), ("claude-opus-5-5", "low"));
        assert_eq!((s.min_recording_ms, s.max_recording_ms, s.restore_delay_ms), (300, 600_000, 150));
        assert_eq!(s.level, Level::Formatted);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = Settings { stt_preset: "groq".into(), level: Level::Clean, ..Default::default() };
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
            Settings { stt_base_url: String::new(), ..Default::default() },
            Settings { stt_base_url: "http://api.example.com/v1".into(), ..Default::default() },
            Settings { stt_base_url: "http://localhost.evil.com/v1".into(), ..Default::default() },
            Settings { stt_base_url: "https://".into(), ..Default::default() },
        ];
        for s in bad {
            assert!(s.validate().is_err(), "should be rejected: {s:?}");
        }
        let good = [
            Settings { audio_retention_days: 0, ..Default::default() },
            Settings { audio_retention_days: MAX_AUDIO_RETENTION_DAYS, ..Default::default() },
            Settings { stt_base_url: "http://localhost:8080/v1".into(), ..Default::default() },
            Settings { stt_base_url: "http://127.0.0.1/v1".into(), ..Default::default() },
            Settings { stt_base_url: "https://api.groq.com/openai/v1".into(), ..Default::default() },
        ];
        for s in good {
            assert!(s.validate().is_ok(), "should be accepted: {s:?}");
        }
    }
}
