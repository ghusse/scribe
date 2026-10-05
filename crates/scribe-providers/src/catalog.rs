//! Known providers and models. In each list the first model is the default (newest first).
//! Sources (2026-10-05): developers.openai.com/api/docs/models, console.groq.com/docs/models,
//! docs.mistral.ai models overview, Anthropic model table.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmApi {
    /// `POST /v1/messages` (prompt caching, `output_config.effort`).
    Anthropic,
    /// `POST /chat/completions` (OpenAI, Mistral, Groq…), `reasoning_effort` when supported.
    OpenAiChat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LlmModel {
    pub id: &'static str,
    /// Accepted effort levels; empty when the model takes no effort parameter.
    pub efforts: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub stt_models: &'static [&'static str],
    pub llm_api: Option<LlmApi>,
    pub llm_models: &'static [LlmModel],
}

const EFFORTS: &[&str] = &["low", "medium", "high"];

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        label: "OpenAI",
        base_url: "https://api.openai.com/v1",
        stt_models: &["gpt-transcribe", "gpt-4o-transcribe", "gpt-4o-mini-transcribe"],
        llm_api: Some(LlmApi::OpenAiChat),
        llm_models: &[
            LlmModel { id: "gpt-6.1-sol", efforts: EFFORTS },
            LlmModel { id: "gpt-6-astra", efforts: EFFORTS },
            LlmModel { id: "gpt-6-luna", efforts: &[] },
        ],
    },
    Provider {
        id: "anthropic",
        label: "Anthropic",
        base_url: "https://api.anthropic.com",
        stt_models: &[],
        llm_api: Some(LlmApi::Anthropic),
        // Opus 5.5 stays first: Fable 5.1 always thinks and can run for minutes, too slow for dictation.
        llm_models: &[
            LlmModel { id: "claude-opus-5-5", efforts: EFFORTS },
            LlmModel { id: "claude-sonnet-5-5", efforts: EFFORTS },
            LlmModel { id: "claude-fable-5-1", efforts: EFFORTS },
            LlmModel { id: "claude-haiku-4-5", efforts: &[] },
        ],
    },
    Provider {
        id: "mistral",
        label: "Mistral",
        base_url: "https://api.mistral.ai/v1",
        stt_models: &["voxtral-mini-latest", "voxtral-mini-2602"],
        llm_api: Some(LlmApi::OpenAiChat),
        llm_models: &[
            LlmModel { id: "mistral-medium-latest", efforts: &[] },
            LlmModel { id: "mistral-large-latest", efforts: &[] },
            LlmModel { id: "mistral-small-latest", efforts: &[] },
        ],
    },
    Provider {
        id: "groq",
        label: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        stt_models: &["whisper-large-v3-turbo", "whisper-large-v3"],
        llm_api: Some(LlmApi::OpenAiChat),
        llm_models: &[
            LlmModel { id: "openai/gpt-oss-120b", efforts: EFFORTS },
            LlmModel { id: "openai/gpt-oss-20b", efforts: EFFORTS },
            LlmModel { id: "llama-3.3-70b-versatile", efforts: &[] },
            LlmModel { id: "llama-3.1-8b-instant", efforts: &[] },
        ],
    },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

pub fn stt_provider(id: &str) -> Option<&'static Provider> {
    provider(id).filter(|p| !p.stt_models.is_empty())
}

pub fn llm_provider(id: &str) -> Option<&'static Provider> {
    provider(id).filter(|p| p.llm_api.is_some())
}

/// Effort levels for a model. A model typed by hand (not in the catalog) only gets them on
/// Anthropic, whose current models all accept `effort` except Haiku and the 3.x family.
pub fn effort_levels(provider: &Provider, model: &str) -> &'static [&'static str] {
    if let Some(m) = provider.llm_models.iter().find(|m| m.id == model) {
        return m.efforts;
    }
    match provider.llm_api {
        Some(LlmApi::Anthropic) if !model.starts_with("claude-haiku") && !model.starts_with("claude-3") => EFFORTS,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_urls_are_https() {
        for (i, p) in PROVIDERS.iter().enumerate() {
            assert!(p.base_url.starts_with("https://"), "{}", p.id);
            assert!(PROVIDERS[i + 1..].iter().all(|q| q.id != p.id), "duplicate {}", p.id);
            assert_eq!(p.llm_api.is_some(), !p.llm_models.is_empty(), "{}", p.id);
        }
    }

    #[test]
    fn defaults_are_the_first_listed_models() {
        assert_eq!(stt_provider("openai").unwrap().stt_models[0], "gpt-transcribe");
        assert_eq!(llm_provider("anthropic").unwrap().llm_models[0].id, "claude-opus-5-5");
        assert_eq!(llm_provider("mistral").unwrap().llm_models[0].id, "mistral-medium-latest");
    }

    #[test]
    fn capability_lookups() {
        assert!(stt_provider("anthropic").is_none());
        assert!(llm_provider("anthropic").is_some());
        assert!(provider("grok").is_none());
    }

    #[test]
    fn effort_levels_follow_catalog_then_anthropic_heuristic() {
        let anthropic = provider("anthropic").unwrap();
        let mistral = provider("mistral").unwrap();
        assert_eq!(effort_levels(anthropic, "claude-opus-5-5"), EFFORTS);
        assert!(effort_levels(anthropic, "claude-haiku-4-5").is_empty());
        assert_eq!(effort_levels(anthropic, "claude-opus-6"), EFFORTS);
        assert!(effort_levels(anthropic, "claude-3-7-sonnet").is_empty());
        assert!(effort_levels(mistral, "mistral-medium-latest").is_empty());
        assert!(effort_levels(provider("openai").unwrap(), "gpt-7").is_empty());
    }
}
