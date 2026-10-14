//! Builds the real transcriber and corrector from the settings and the stored API keys.
use std::time::Duration;

use scribe_core::pipeline::{Corrector, ProviderError, ProviderRole};
use scribe_core::prompt::CorrectionPrompt;
use scribe_providers::anthropic::AnthropicCorrector;
use scribe_providers::catalog::{self, LlmApi, Provider};
use scribe_providers::openai_chat::OpenAiChatCorrector;
use scribe_providers::openai_compat::OpenAiCompatTranscriber;

use crate::secrets::{self, SecretStore};
use crate::services::Providers;
use crate::settings::Settings;

const HTTP_TIMEOUT: Duration = Duration::from_secs(120);

/// Stands in for the LLM when there is none (correction off) or it cannot be built (missing key):
/// the pipeline then falls back to the raw text instead of failing the dictation.
pub struct NoCorrector(pub ProviderError);

#[async_trait::async_trait]
impl Corrector for NoCorrector {
    fn name(&self) -> Option<String> {
        None
    }
    async fn correct(&self, _p: &CorrectionPrompt) -> Result<String, ProviderError> {
        Err(self.0.clone())
    }
}

/// The configured effort, only when the model accepts one.
fn effort_for(provider: &Provider, s: &Settings) -> Option<String> {
    catalog::effort_levels(provider, &s.llm_model).contains(&s.llm_effort.as_str()).then(|| s.llm_effort.clone())
}

/// The app's `CorrectorFactory`: reads only the correction key, so it works without a transcription key.
pub fn build_corrector(s: &Settings, store: &dyn SecretStore) -> Box<dyn Corrector> {
    if !s.correction_enabled {
        return Box::new(NoCorrector(ProviderError::CorrectionDisabled));
    }
    let Some(provider) = catalog::llm_provider(&s.llm_provider) else {
        return Box::new(NoCorrector(ProviderError::UnknownProvider {
            role: ProviderRole::Correction,
            id: s.llm_provider.clone(),
        }));
    };
    let Some(key) = secrets::get_key(store, provider.id) else {
        return Box::new(NoCorrector(ProviderError::MissingKey { provider: provider.label.into() }));
    };
    let effort = effort_for(provider, s);
    let built: Result<Box<dyn Corrector>, ProviderError> = match provider.llm_api {
        Some(LlmApi::Anthropic) => AnthropicCorrector::new(provider.base_url, key, &s.llm_model, effort, HTTP_TIMEOUT)
            .map(|c| Box::new(c) as Box<dyn Corrector>),
        _ => OpenAiChatCorrector::new(provider.base_url, key, &s.llm_model, effort, HTTP_TIMEOUT)
            .map(|c| Box::new(c) as Box<dyn Corrector>),
    };
    built.unwrap_or_else(|e| Box::new(NoCorrector(e)))
}

/// The app's `ProviderFactory`. A missing transcription key fails; a missing correction key only
/// disables the correction.
pub fn build(s: &Settings, store: &dyn SecretStore) -> Result<Providers, ProviderError> {
    let provider = catalog::stt_provider(&s.stt_provider).ok_or_else(|| ProviderError::UnknownProvider {
        role: ProviderRole::Transcription,
        id: s.stt_provider.clone(),
    })?;
    let stt_key = secrets::get_key(store, provider.id)
        .ok_or_else(|| ProviderError::MissingKey { provider: provider.label.into() })?;
    let stt = OpenAiCompatTranscriber::new(provider.base_url, stt_key, &s.stt_model, HTTP_TIMEOUT)?;
    Ok((Box::new(stt), build_corrector(s, store)))
}

/// The hosts the next dictation will call: the transcription provider, then the correction one when the correction is
/// on and it is another host. Unknown providers are skipped (the dictation reports them).
pub fn warm_up_urls(s: &Settings) -> Vec<&'static str> {
    let stt = catalog::stt_provider(&s.stt_provider).map(|p| p.base_url);
    let llm = catalog::llm_provider(&s.llm_provider).filter(|_| s.correction_enabled).map(|p| p.base_url);
    let mut urls: Vec<&'static str> = stt.into_iter().collect();
    urls.extend(llm.filter(|url| Some(*url) != stt));
    urls
}

/// The app's `WarmUp`: one request per host on the async runtime, never awaited.
pub fn warm_up(s: &Settings) {
    for url in warm_up_urls(s) {
        tauri::async_runtime::spawn(scribe_providers::http::warm_up(url));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemorySecretStore;
    use scribe_core::lang::Language;
    use scribe_core::prompt;

    fn correction_error(c: &dyn Corrector) -> ProviderError {
        let p = prompt::build_correction_prompt("bonjour", &[], None, "- Corrige.", Language::Fr);
        tauri::async_runtime::block_on(c.correct(&p)).unwrap_err()
    }

    fn settings(stt: &str, llm: &str, model: &str, correction_enabled: bool) -> Settings {
        Settings {
            stt_provider: stt.into(),
            stt_model: "whisper-x".into(),
            llm_provider: llm.into(),
            llm_model: model.into(),
            correction_enabled,
            ..Default::default()
        }
    }

    fn llm_key() -> MemorySecretStore {
        MemorySecretStore::with(&[("anthropic", "sk"), ("mistral", "sk")])
    }

    #[test]
    fn warms_up_the_transcription_host_then_the_correction_host() {
        let openai = "https://api.openai.com/v1";
        let anthropic = "https://api.anthropic.com";
        assert_eq!(warm_up_urls(&settings("openai", "anthropic", "claude-opus-5-5", true)), vec![openai, anthropic]);
        assert_eq!(warm_up_urls(&settings("openai", "openai", "gpt-6-astra", true)), vec![openai], "one host, once");
        assert_eq!(
            warm_up_urls(&settings("openai", "anthropic", "claude-opus-5-5", false)),
            vec![openai],
            "no correction"
        );
        assert_eq!(warm_up_urls(&settings("nope", "anthropic", "claude-opus-5-5", true)), vec![anthropic]);
        assert!(warm_up_urls(&settings("anthropic", "nope", "m", true)).is_empty(), "Anthropic has no transcription");
        warm_up(&settings("nope", "nope", "m", true)); // nothing to open
    }

    #[test]
    fn missing_llm_key_falls_back_to_raw_text_instead_of_failing() {
        let c = build_corrector(&Settings::default(), &MemorySecretStore::default());
        assert_eq!(c.name(), None);
        assert_eq!(correction_error(c.as_ref()), ProviderError::MissingKey { provider: "Anthropic".into() });
    }

    #[test]
    fn correction_off_has_no_corrector_and_reads_no_key() {
        let store = llm_key();
        let c = build_corrector(&settings("openai", "anthropic", "claude-opus-5-5", false), &store);
        assert_eq!(correction_error(c.as_ref()), ProviderError::CorrectionDisabled);
        assert!(store.reads.lock().unwrap().is_empty());
    }

    #[test]
    fn unknown_llm_provider_disables_the_correction() {
        let c = build_corrector(&settings("openai", "nope", "m", true), &llm_key());
        assert_eq!(
            correction_error(c.as_ref()),
            ProviderError::UnknownProvider { role: ProviderRole::Correction, id: "nope".into() }
        );
    }

    #[test]
    fn builds_the_api_of_the_llm_provider() {
        let c = build_corrector(&settings("openai", "anthropic", "claude-haiku-4-5", true), &llm_key());
        assert_eq!(c.name().as_deref(), Some("claude-haiku-4-5"));
        let c = build_corrector(&settings("openai", "mistral", "mistral-small-latest", true), &llm_key());
        assert_eq!(c.name().as_deref(), Some("mistral-small-latest"));
    }

    #[test]
    fn the_corrector_needs_no_transcription_key() {
        let store = MemorySecretStore::with(&[("anthropic", "sk-a")]);
        let c = build_corrector(&settings("groq", "anthropic", "claude-opus-5-5", true), &store);
        assert_eq!(c.name().as_deref(), Some("claude-opus-5-5"));
        assert_eq!(*store.reads.lock().unwrap(), vec!["anthropic".to_string()], "only the correction key is read");
    }

    #[test]
    fn effort_is_sent_only_to_models_that_take_one() {
        let anthropic = catalog::provider("anthropic").unwrap();
        let mut s = settings("openai", "anthropic", "claude-opus-5-5", true);
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
        let err = build(&settings("anthropic", "anthropic", "claude-opus-5-5", true), &store).err().unwrap();
        assert_eq!(err, ProviderError::UnknownProvider { role: ProviderRole::Transcription, id: "anthropic".into() });
        let err = build(&settings("groq", "anthropic", "claude-opus-5-5", true), &store).err().unwrap();
        assert_eq!(err, ProviderError::MissingKey { provider: "Groq".into() });
        let (stt, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", true), &store).unwrap();
        assert_eq!(stt.name(), "whisper-x");
        assert_eq!(llm.name().as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn correction_off_does_not_read_the_correction_key() {
        let store = MemorySecretStore::with(&[("openai", "sk-o"), ("anthropic", "sk-a")]);
        let (_, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", false), &store).unwrap();
        assert_eq!(llm.name(), None);
        assert_eq!(*store.reads.lock().unwrap(), vec!["openai".to_string()], "only the transcription key is read");
        store.reads.lock().unwrap().clear();
        let (_, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", true), &store).unwrap();
        assert!(llm.name().is_some());
        assert_eq!(*store.reads.lock().unwrap(), vec!["openai".to_string(), "anthropic".to_string()]);
        let store = MemorySecretStore::with(&[("openai", "sk-o")]);
        let (_, llm) = build(&settings("openai", "anthropic", "claude-opus-5-5", true), &store).unwrap();
        assert_eq!(llm.name(), None, "missing correction key: raw text instead of a failure");
    }
}
