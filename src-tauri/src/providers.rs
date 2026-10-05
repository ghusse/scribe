//! Builds the real transcriber and corrector from the settings and the stored API keys.
use std::time::Duration;

use scribe_core::model::Level;
use scribe_core::pipeline::{Corrector, ProviderError};
use scribe_core::prompt::CorrectionPrompt;
use scribe_providers::anthropic::AnthropicCorrector;
use scribe_providers::catalog::{self, LlmApi, Provider};
use scribe_providers::openai_chat::OpenAiChatCorrector;
use scribe_providers::openai_compat::OpenAiCompatTranscriber;

use crate::secrets::{self, SecretStore};
use crate::services::Providers;
use crate::settings::Settings;

const HTTP_TIMEOUT: Duration = Duration::from_secs(120);

/// Stands in for the LLM when there is none (Raw level) or it cannot be built (missing key):
/// the pipeline then falls back to the raw text instead of failing the dictation.
pub struct NoCorrector(pub String);

#[async_trait::async_trait]
impl Corrector for NoCorrector {
    fn name(&self) -> String {
        "aucun".into()
    }
    async fn correct(&self, _p: &CorrectionPrompt) -> Result<String, ProviderError> {
        Err(ProviderError::Config(self.0.clone()))
    }
}

/// The configured effort, only when the model accepts one.
fn effort_for(provider: &Provider, s: &Settings) -> Option<String> {
    catalog::effort_levels(provider, &s.llm_model).contains(&s.llm_effort.as_str()).then(|| s.llm_effort.clone())
}

fn build_corrector(s: &Settings, llm_key: Option<String>) -> Box<dyn Corrector> {
    if s.level == Level::Raw {
        return Box::new(NoCorrector("correcteur désactivé".into()));
    }
    let Some(provider) = catalog::llm_provider(&s.llm_provider) else {
        return Box::new(NoCorrector(format!("fournisseur de correction inconnu : {}", s.llm_provider)));
    };
    let Some(key) = llm_key else {
        return Box::new(NoCorrector(format!("clé API {} manquante", provider.label)));
    };
    let effort = effort_for(provider, s);
    let built: Result<Box<dyn Corrector>, ProviderError> = match provider.llm_api {
        Some(LlmApi::Anthropic) => AnthropicCorrector::new(provider.base_url, key, &s.llm_model, effort, HTTP_TIMEOUT)
            .map(|c| Box::new(c) as Box<dyn Corrector>),
        _ => OpenAiChatCorrector::new(provider.base_url, key, &s.llm_model, effort, HTTP_TIMEOUT)
            .map(|c| Box::new(c) as Box<dyn Corrector>),
    };
    built.unwrap_or_else(|e| Box::new(NoCorrector(e.to_string())))
}

/// The app's `ProviderFactory`. A missing transcription key fails; a missing correction key only
/// disables the correction.
pub fn build(s: &Settings, store: &dyn SecretStore) -> Result<Providers, ProviderError> {
    let provider = catalog::stt_provider(&s.stt_provider)
        .ok_or_else(|| ProviderError::Config(format!("fournisseur de transcription inconnu : {}", s.stt_provider)))?;
    let stt_key = secrets::get_key(store, provider.id)
        .ok_or_else(|| ProviderError::Config(format!("clé API manquante pour « {} »", provider.label)))?;
    let stt = OpenAiCompatTranscriber::new(provider.base_url, stt_key, &s.stt_model, HTTP_TIMEOUT)?;
    let llm_key = if s.level == Level::Raw { None } else { secrets::get_key(store, &s.llm_provider) };
    Ok((Box::new(stt), build_corrector(s, llm_key)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemorySecretStore;
    use scribe_core::prompt;

    fn correction_error(c: &dyn Corrector) -> String {
        let p = prompt::build_correction_prompt("bonjour", &[], None, Level::Formatted);
        tauri::async_runtime::block_on(c.correct(&p)).unwrap_err().to_string()
    }

    fn settings(stt: &str, llm: &str, model: &str, level: Level) -> Settings {
        Settings { stt_provider: stt.into(), stt_model: "whisper-x".into(), llm_provider: llm.into(), llm_model: model.into(), level, ..Default::default() }
    }

    #[test]
    fn missing_llm_key_falls_back_to_raw_text_instead_of_failing() {
        let s = Settings { level: Level::Formatted, ..Default::default() };
        let c = build_corrector(&s, None);
        assert_eq!(c.name(), "aucun");
        assert!(correction_error(c.as_ref()).contains("clé API Anthropic manquante"));
    }

    #[test]
    fn raw_level_has_no_corrector() {
        let s = settings("openai", "anthropic", "claude-opus-5-5", Level::Raw);
        let c = build_corrector(&s, Some("sk".into()));
        assert!(correction_error(c.as_ref()).contains("correcteur désactivé"));
    }

    #[test]
    fn unknown_llm_provider_disables_the_correction() {
        let s = settings("openai", "nope", "m", Level::Formatted);
        let c = build_corrector(&s, Some("sk".into()));
        assert!(correction_error(c.as_ref()).contains("fournisseur de correction inconnu : nope"));
    }

    #[test]
    fn builds_the_api_of_the_llm_provider() {
        let c = build_corrector(&settings("openai", "anthropic", "claude-haiku-4-5", Level::Formatted), Some("sk".into()));
        assert_eq!(c.name(), "claude-haiku-4-5");
        let c = build_corrector(&settings("openai", "mistral", "mistral-small-latest", Level::Clean), Some("sk".into()));
        assert_eq!(c.name(), "mistral-small-latest");
    }

    #[test]
    fn effort_is_sent_only_to_models_that_take_one() {
        let anthropic = catalog::provider("anthropic").unwrap();
        let mut s = settings("openai", "anthropic", "claude-opus-5-5", Level::Formatted);
        s.llm_effort = "high".into();
        assert_eq!(effort_for(anthropic, &s).as_deref(), Some("high"));
        s.llm_model = "claude-haiku-4-5".into();
        assert_eq!(effort_for(anthropic, &s), None);
        s.llm_model = "claude-opus-5-5".into();
        s.llm_effort = "extreme".into();
        assert_eq!(effort_for(anthropic, &s), None);
    }

    #[test]
    fn transcription_needs_a_known_provider_and_its_key() {
        let store = MemorySecretStore::with(&[("openai", "sk-o"), ("anthropic", "sk-a")]);
        let err = build(&settings("anthropic", "anthropic", "claude-opus-5-5", Level::Formatted), &store).err().unwrap();
        assert_eq!(err, ProviderError::Config("fournisseur de transcription inconnu : anthropic".into()));
        let err = build(&settings("groq", "anthropic", "claude-opus-5-5", Level::Formatted), &store).err().unwrap();
        assert_eq!(err, ProviderError::Config("clé API manquante pour « Groq »".into()));
        let (stt, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", Level::Formatted), &store).unwrap();
        assert_eq!(stt.name(), "whisper-x");
        assert_eq!(llm.name(), "claude-opus-5-5");
    }

    #[test]
    fn raw_level_does_not_read_the_correction_key() {
        let store = MemorySecretStore::with(&[("openai", "sk-o"), ("anthropic", "sk-a")]);
        let (_, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", Level::Raw), &store).unwrap();
        assert_eq!(llm.name(), "aucun");
        assert_eq!(*store.reads.lock().unwrap(), vec!["openai".to_string()], "only the transcription key is read");
        store.reads.lock().unwrap().clear();
        let (_, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", Level::Formatted), &store).unwrap();
        assert_ne!(llm.name(), "aucun");
        assert_eq!(*store.reads.lock().unwrap(), vec!["openai".to_string(), "anthropic".to_string()]);
        let store = MemorySecretStore::with(&[("openai", "sk-o")]);
        let (_, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", Level::Formatted), &store).unwrap();
        assert_eq!(llm.name(), "aucun", "missing correction key: raw text instead of a failure");
    }
}
