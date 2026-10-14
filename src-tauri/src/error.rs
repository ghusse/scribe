//! The error of every command: a code the UI translates (`AppError` in src/lib/api.ts).
use serde::Serialize;

use scribe_core::pipeline::{PipelineError, ProviderError};
use scribe_core::storage::StorageError;

use crate::settings::SettingsError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AppError {
    /// A technical failure (database, file, keyring, clipboard, OS…): `detail` is shown untranslated.
    System {
        detail: String,
    },
    DictationNotFound,
    NoText,
    AudioPurged,
    NoSpeech,
    DuplicateTerm {
        term: String,
    },
    InvalidSettings {
        error: SettingsError,
    },
    Provider {
        error: ProviderError,
    },
    NoUpdate,
}

impl AppError {
    pub fn system(e: impl std::fmt::Display) -> Self {
        AppError::System { detail: e.to_string() }
    }
}

impl From<String> for AppError {
    fn from(detail: String) -> Self {
        AppError::System { detail }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::system(e)
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::system(e)
    }
}

impl From<StorageError> for AppError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::DuplicateTerm(term) => AppError::DuplicateTerm { term },
            e => AppError::system(e),
        }
    }
}

impl From<ProviderError> for AppError {
    fn from(error: ProviderError) -> Self {
        AppError::Provider { error }
    }
}

impl From<PipelineError> for AppError {
    fn from(e: PipelineError) -> Self {
        match e {
            PipelineError::Transcription(error) => AppError::Provider { error },
            PipelineError::Empty => AppError::NoSpeech,
        }
    }
}

impl From<SettingsError> for AppError {
    fn from(error: SettingsError) -> Self {
        AppError::InvalidSettings { error }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn errors_map_onto_codes() {
        assert_eq!(AppError::from("disk full".to_string()), AppError::System { detail: "disk full".into() });
        assert_eq!(AppError::from(std::io::Error::other("denied")), AppError::System { detail: "denied".into() });
        assert_eq!(
            AppError::from(tauri::Error::FailedToReceiveMessage),
            AppError::system(tauri::Error::FailedToReceiveMessage)
        );
        assert_eq!(
            AppError::from(StorageError::DuplicateTerm("Tauri".into())),
            AppError::DuplicateTerm { term: "Tauri".into() }
        );
        assert_eq!(
            AppError::from(StorageError::Invalid("empty term".into())),
            AppError::System { detail: "invalid data: empty term".into() }
        );
        assert_eq!(AppError::from(PipelineError::Empty), AppError::NoSpeech);
        assert_eq!(
            AppError::from(PipelineError::Transcription(ProviderError::Auth)),
            AppError::Provider { error: ProviderError::Auth }
        );
        assert_eq!(AppError::from(ProviderError::Timeout), AppError::Provider { error: ProviderError::Timeout });
        assert_eq!(
            AppError::from(SettingsError::ModelRequired),
            AppError::InvalidSettings { error: SettingsError::ModelRequired }
        );
    }

    #[test]
    fn app_errors_serialize_as_codes_for_the_ui() {
        let cases = [
            (AppError::System { detail: "x".into() }, json!({"code": "system", "detail": "x"})),
            (AppError::DictationNotFound, json!({"code": "dictation_not_found"})),
            (AppError::NoText, json!({"code": "no_text"})),
            (AppError::AudioPurged, json!({"code": "audio_purged"})),
            (AppError::NoSpeech, json!({"code": "no_speech"})),
            (AppError::DuplicateTerm { term: "Tauri".into() }, json!({"code": "duplicate_term", "term": "Tauri"})),
            (
                AppError::InvalidSettings { error: SettingsError::RestoreDelayTooLong { max_ms: 2_000 } },
                json!({"code": "invalid_settings", "error": {"code": "restore_delay_too_long", "max_ms": 2_000}}),
            ),
            (
                AppError::Provider { error: ProviderError::Http { status: 404, body: "b".into() } },
                json!({"code": "provider", "error": {"code": "http", "status": 404, "body": "b"}}),
            ),
            (AppError::NoUpdate, json!({"code": "no_update"})),
        ];
        for (e, expected) in cases {
            assert_eq!(serde_json::to_value(&e).unwrap(), expected);
        }
    }
}
