use serde::{Deserialize, Serialize};

use crate::gesture::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Raw,
    Clean,
    #[default]
    Formatted,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Raw => "raw",
            Level::Clean => "clean",
            Level::Formatted => "formatted",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "raw" => Some(Level::Raw),
            "clean" => Some(Level::Clean),
            "formatted" => Some(Level::Formatted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pasted,
    PastedUncertain,
    Clipboard,
    Error,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Pasted => "pasted",
            Outcome::PastedUncertain => "pasted_uncertain",
            Outcome::Clipboard => "clipboard",
            Outcome::Error => "error",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pasted" => Some(Outcome::Pasted),
            "pasted_uncertain" => Some(Outcome::PastedUncertain),
            "clipboard" => Some(Outcome::Clipboard),
            "error" => Some(Outcome::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TermSource {
    Manual,
    Correction,
    Mined,
}

impl TermSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TermSource::Manual => "manual",
            TermSource::Correction => "correction",
            TermSource::Mined => "mined",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "manual" => Some(TermSource::Manual),
            "correction" => Some(TermSource::Correction),
            "mined" => Some(TermSource::Mined),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Term {
    pub id: i64,
    pub term: String,
    pub variants: Vec<String>,
    pub note: Option<String>,
    pub source: TermSource,
    pub use_count: i64,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dictation {
    pub id: i64,
    pub created_at: String,
    pub mode: String,
    pub app_name: Option<String>,
    pub audio_path: Option<String>,
    pub duration_ms: i64,
    pub raw_text: Option<String>,
    pub final_text: Option<String>,
    pub edited_text: Option<String>,
    pub level: Level,
    pub transcriber: Option<String>,
    pub corrector: Option<String>,
    pub stt_ms: Option<i64>,
    pub llm_ms: Option<i64>,
    pub outcome: Outcome,
    pub error: Option<String>,
}

impl Dictation {
    /// The text the user would want back: their correction, else the corrected text, else the raw one.
    pub fn best_text(&self) -> Option<&str> {
        self.edited_text.as_deref().or(self.final_text.as_deref()).or(self.raw_text.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewDictation {
    pub created_at: String,
    pub mode: Mode,
    pub app_name: Option<String>,
    pub app_bundle_id: Option<String>,
    pub audio_path: Option<String>,
    pub duration_ms: i64,
    pub raw_text: Option<String>,
    pub final_text: Option<String>,
    pub level: Level,
    pub transcriber: Option<String>,
    pub corrector: Option<String>,
    pub stt_ms: Option<i64>,
    pub llm_ms: Option<i64>,
    pub outcome: Outcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionUpdate {
    pub raw_text: Option<String>,
    pub final_text: Option<String>,
    pub transcriber: Option<String>,
    pub corrector: Option<String>,
    pub stt_ms: Option<i64>,
    pub llm_ms: Option<i64>,
    pub outcome: Outcome,
    pub error: Option<String>,
}
