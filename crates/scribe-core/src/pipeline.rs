use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::model::{Level, Term};
use crate::prompt::{self, CorrectionPrompt};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("erreur réseau : {0}")]
    Network(String),
    #[error("délai dépassé")]
    Timeout,
    #[error("clé API refusée")]
    Auth,
    #[error("HTTP {status} : {body}")]
    Http { status: u16, body: String },
    #[error("le modèle a refusé la requête")]
    Refusal,
    #[error("réponse invalide : {0}")]
    Malformed(String),
    #[error("configuration : {0}")]
    Config(String),
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Network(_) | ProviderError::Timeout => true,
            ProviderError::Http { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

#[async_trait]
pub trait Transcriber: Send + Sync {
    fn name(&self) -> String;
    async fn transcribe(&self, wav: &[u8], hints: &[String]) -> Result<String, ProviderError>;
}

#[async_trait]
pub trait Corrector: Send + Sync {
    fn name(&self) -> String;
    /// Returns the raw model response; the pipeline extracts the `<output>` part.
    async fn correct(&self, prompt: &CorrectionPrompt) -> Result<String, ProviderError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    pub level: Level,
    pub hint_budget_chars: usize,
    pub llm_timeout_base_ms: u64,
    pub llm_timeout_per_char_ms: u64,
    pub stt_retry_delay_ms: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            level: Level::Formatted,
            hint_budget_chars: 800,
            llm_timeout_base_ms: 3_000,
            llm_timeout_per_char_ms: 5,
            stt_retry_delay_ms: 400,
        }
    }
}

pub fn llm_timeout(cfg: &PipelineConfig, raw: &str) -> Duration {
    let per_char = cfg.llm_timeout_per_char_ms.saturating_mul(raw.chars().count() as u64);
    Duration::from_millis(cfg.llm_timeout_base_ms.saturating_add(per_char))
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineOutput {
    pub raw: String,
    pub final_text: String,
    pub stt_ms: u64,
    pub llm_ms: Option<u64>,
    /// Set when the corrector failed and `final_text` is the raw transcript.
    pub correction_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PipelineError {
    #[error("transcription : {0}")]
    Transcription(ProviderError),
    #[error("aucune parole détectée")]
    Empty,
}

/// Phrases Whisper-like models emit on silence (subtitle credits from their training data).
const HALLUCINATIONS: &[&str] = &[
    "sous-titres réalisés par la communauté d'amara.org",
    "sous-titres réalisés para la communauté d'amara.org",
    "sous-titrage st' 501",
    "sous-titrage société radio-canada",
    "merci d'avoir regardé",
    "merci d'avoir regardé cette vidéo",
    "thank you for watching",
    "thanks for watching",
];

pub fn is_blank_or_hallucination(text: &str) -> bool {
    let lowered = text.trim().to_lowercase().replace('’', "'");
    let normalized = lowered
        .trim_end_matches(|c: char| matches!(c, '.' | '!' | '?') || c.is_whitespace())
        .trim();
    normalized.is_empty() || HALLUCINATIONS.contains(&normalized)
}

fn normalize_quotes_lower(s: &str) -> String {
    s.to_lowercase().replace('’', "'")
}

/// Removes known credit phrases appended as trailing sentences after real speech
/// (the usual Whisper failure on trailing silence): "Bonjour à tous. Sous-titres réalisés par…".
/// Only whole trailing sentences are removed: "Merci d'avoir regardé ma PR" is kept.
pub fn strip_trailing_hallucinations(text: &str) -> String {
    let mut s = text.trim();
    'outer: loop {
        let body = s.trim_end_matches(|c: char| matches!(c, '.' | '!' | '?' | '…') || c.is_whitespace());
        for phrase in HALLUCINATIONS {
            let n = phrase.chars().count();
            let Some((start, _)) = body.char_indices().rev().nth(n - 1) else { continue };
            if normalize_quotes_lower(&body[start..]) != *phrase {
                continue;
            }
            let head = body[..start].trim_end();
            if head.is_empty() || head.ends_with(['.', '!', '?', '…']) {
                s = head;
                continue 'outer;
            }
        }
        return s.to_string();
    }
}

/// A transcript of at most this many words is delivered without correction: the transcription model already
/// capitalizes and punctuates it, and the corrector would add seconds of latency to « OK, merci ».
pub const SHORT_TRANSCRIPT_MAX_WORDS: usize = 3;

fn is_short(raw: &str) -> bool {
    raw.split_whitespace().count() <= SHORT_TRANSCRIPT_MAX_WORDS
}

pub async fn run(
    cfg: &PipelineConfig,
    wav: &[u8],
    terms: &[Term],
    app_name: Option<&str>,
    transcriber: &dyn Transcriber,
    corrector: &dyn Corrector,
) -> Result<PipelineOutput, PipelineError> {
    let hints = prompt::select_hints(terms, cfg.hint_budget_chars);
    let t0 = Instant::now();
    let raw = match transcriber.transcribe(wav, &hints).await {
        Ok(t) => t,
        Err(e) if e.is_retryable() => {
            tokio::time::sleep(Duration::from_millis(cfg.stt_retry_delay_ms)).await;
            transcriber.transcribe(wav, &hints).await.map_err(PipelineError::Transcription)?
        }
        Err(e) => return Err(PipelineError::Transcription(e)),
    };
    let stt_ms = t0.elapsed().as_millis() as u64;
    let raw = strip_trailing_hallucinations(&raw);
    if is_blank_or_hallucination(&raw) {
        return Err(PipelineError::Empty);
    }
    if cfg.level == Level::Raw || is_short(&raw) {
        return Ok(PipelineOutput { final_text: raw.clone(), raw, stt_ms, llm_ms: None, correction_error: None });
    }

    let p = prompt::build_correction_prompt(&raw, terms, app_name, cfg.level);
    let t1 = Instant::now();
    let result = tokio::time::timeout(llm_timeout(cfg, &raw), corrector.correct(&p)).await;
    let llm_ms = Some(t1.elapsed().as_millis() as u64);
    let (final_text, correction_error) = match result {
        Ok(Ok(resp)) => match prompt::extract_output(&resp) {
            Some(text) => (text, None),
            None => (raw.clone(), Some("réponse du correcteur sans balise <output>".to_string())),
        },
        Ok(Err(e)) => (raw.clone(), Some(e.to_string())),
        Err(_) => (raw.clone(), Some("délai du correcteur dépassé".to_string())),
    };
    Ok(PipelineOutput { raw, final_text, stt_ms, llm_ms, correction_error })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TermSource;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct FakeStt {
        responses: Mutex<VecDeque<Result<String, ProviderError>>>,
        hints_seen: Mutex<Vec<Vec<String>>>,
    }
    impl FakeStt {
        fn new(responses: Vec<Result<String, ProviderError>>) -> Self {
            Self { responses: Mutex::new(responses.into()), hints_seen: Mutex::new(vec![]) }
        }
        fn calls(&self) -> usize {
            self.hints_seen.lock().unwrap().len()
        }
    }
    #[async_trait]
    impl Transcriber for FakeStt {
        fn name(&self) -> String { "fake-stt".into() }
        async fn transcribe(&self, _wav: &[u8], hints: &[String]) -> Result<String, ProviderError> {
            self.hints_seen.lock().unwrap().push(hints.to_vec());
            self.responses.lock().unwrap().pop_front().expect("unexpected call")
        }
    }

    struct FakeLlm {
        response: Result<String, ProviderError>,
        delay_ms: u64,
        calls: AtomicUsize,
    }
    impl FakeLlm {
        fn new(response: Result<String, ProviderError>, delay_ms: u64) -> Self {
            Self { response, delay_ms, calls: AtomicUsize::new(0) }
        }
    }
    #[async_trait]
    impl Corrector for FakeLlm {
        fn name(&self) -> String { "fake-llm".into() }
        async fn correct(&self, _p: &CorrectionPrompt) -> Result<String, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            self.response.clone()
        }
    }

    fn cfg(level: Level) -> PipelineConfig {
        PipelineConfig { level, ..Default::default() }
    }

    fn term(id: i64, t: &str) -> Term {
        Term { id, term: t.into(), variants: vec![], note: None, source: TermSource::Manual, use_count: 0,
               last_used_at: None, created_at: "2026-10-05T10:00:00.000Z".into() }
    }

    #[tokio::test(start_paused = true)]
    async fn clean_level_uses_corrector_output() {
        let stt = FakeStt::new(vec![Ok(" cube ernetes c'est top ".into())]);
        let llm = FakeLlm::new(Ok("<output>Kubernetes, c'est top.</output>".into()), 10);
        let out = run(&cfg(Level::Clean), b"wav", &[term(1, "Kubernetes")], Some("Code"), &stt, &llm).await.unwrap();
        assert_eq!(out.raw, "cube ernetes c'est top");
        assert_eq!(out.final_text, "Kubernetes, c'est top.");
        assert_eq!(out.correction_error, None);
        assert!(out.llm_ms.is_some());
        assert_eq!(stt.hints_seen.lock().unwrap()[0], vec!["Kubernetes".to_string()]);
    }

    #[tokio::test(start_paused = true)]
    async fn raw_level_skips_corrector() {
        let stt = FakeStt::new(vec![Ok("bonjour".into())]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        let out = run(&cfg(Level::Raw), b"wav", &[], None, &stt, &llm).await.unwrap();
        assert_eq!((out.final_text.as_str(), out.llm_ms), ("bonjour", None));
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn short_transcript_skips_corrector() {
        for (raw, level) in [("OK, merci beaucoup.", Level::Clean), ("à demain", Level::Formatted), ("Oui.", Level::Formatted)] {
            let stt = FakeStt::new(vec![Ok(raw.into())]);
            let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
            let out = run(&cfg(level), b"wav", &[], None, &stt, &llm).await.unwrap();
            assert_eq!((out.final_text.as_str(), out.llm_ms, out.correction_error), (raw, None, None), "{raw}");
            assert_eq!(llm.calls.load(Ordering::SeqCst), 0, "{raw}");
        }
        let stt = FakeStt::new(vec![Ok("OK, merci beaucoup Paul.".into())]);
        let llm = FakeLlm::new(Ok("<output>OK, merci beaucoup Paul !</output>".into()), 0);
        let out = run(&cfg(Level::Formatted), b"wav", &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "OK, merci beaucoup Paul !");
        assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn corrector_error_falls_back_to_raw() {
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Err(ProviderError::Http { status: 529, body: "overloaded".into() }), 0);
        let out = run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour à tous et merci");
        assert!(out.correction_error.unwrap().contains("529"));
    }

    #[tokio::test(start_paused = true)]
    async fn corrector_timeout_falls_back_to_raw() {
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Ok("<output>Bonjour.</output>".into()), 60_000);
        let out = run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour à tous et merci");
        assert!(out.correction_error.unwrap().contains("délai"));
    }

    #[tokio::test(start_paused = true)]
    async fn corrector_without_output_tags_falls_back_to_raw() {
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Ok("Voici le texte corrigé : Bonjour.".into()), 0);
        let out = run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour à tous et merci");
        assert!(out.correction_error.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn retryable_transcription_error_is_retried_once() {
        let stt = FakeStt::new(vec![Err(ProviderError::Timeout), Ok("bonjour".into())]);
        let llm = FakeLlm::new(Ok("<output>Bonjour.</output>".into()), 0);
        assert!(run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await.is_ok());
        assert_eq!(stt.calls(), 2);

        let stt = FakeStt::new(vec![Err(ProviderError::Network("x".into())), Err(ProviderError::Network("y".into()))]);
        assert_eq!(
            run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await,
            Err(PipelineError::Transcription(ProviderError::Network("y".into())))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn auth_error_is_not_retried() {
        let stt = FakeStt::new(vec![Err(ProviderError::Auth)]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        assert_eq!(
            run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await,
            Err(PipelineError::Transcription(ProviderError::Auth))
        );
        assert_eq!(stt.calls(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn blank_or_hallucinated_transcript_is_empty() {
        for raw in ["", "   ", "Sous-titres réalisés par la communauté d'Amara.org", "Merci d’avoir regardé !"] {
            let stt = FakeStt::new(vec![Ok(raw.into())]);
            let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
            assert_eq!(run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await, Err(PipelineError::Empty), "{raw}");
            assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn trailing_credit_after_real_speech_is_removed() {
        let stt = FakeStt::new(vec![Ok("Bonjour à tous. Sous-titres réalisés par la communauté d'Amara.org".into())]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        let out = run(&cfg(Level::Raw), b"wav", &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "Bonjour à tous.");
        assert_eq!(out.raw, "Bonjour à tous.");
    }

    #[test]
    fn strip_trailing_hallucinations_only_removes_whole_sentences() {
        assert_eq!(strip_trailing_hallucinations("Bonjour. Merci d’avoir regardé !"), "Bonjour.");
        assert_eq!(
            strip_trailing_hallucinations("C'est prêt ? Thanks for watching. Sous-titrage ST' 501"),
            "C'est prêt ?"
        );
        assert_eq!(strip_trailing_hallucinations("Merci d'avoir regardé ma PR."), "Merci d'avoir regardé ma PR.");
        assert_eq!(strip_trailing_hallucinations("Je dis merci d'avoir regardé"), "Je dis merci d'avoir regardé");
        assert_eq!(strip_trailing_hallucinations("Thanks for watching"), "");
    }

    #[test]
    fn real_short_utterances_are_not_hallucinations() {
        assert!(!is_blank_or_hallucination("Merci."));
        assert!(!is_blank_or_hallucination("Merci d'avoir regardé ma PR, je corrige."));
    }

    #[test]
    fn llm_timeout_grows_with_transcript_length() {
        let c = PipelineConfig::default();
        assert_eq!(llm_timeout(&c, ""), Duration::from_millis(3_000));
        assert_eq!(llm_timeout(&c, &"a".repeat(1_000)), Duration::from_millis(8_000));
        let huge = PipelineConfig { llm_timeout_per_char_ms: u64::MAX, ..PipelineConfig::default() };
        assert_eq!(llm_timeout(&huge, "ab"), Duration::from_millis(u64::MAX));
    }

    #[test]
    fn retry_classification() {
        let retried = [
            ProviderError::Network("connexion réinitialisée".into()),
            ProviderError::Timeout,
            ProviderError::Http { status: 429, body: String::new() },
            ProviderError::Http { status: 500, body: String::new() },
            ProviderError::Http { status: 503, body: String::new() },
            ProviderError::Http { status: 529, body: String::new() },
        ];
        let final_ = [
            ProviderError::Auth,
            ProviderError::Http { status: 400, body: String::new() },
            ProviderError::Http { status: 404, body: String::new() },
            ProviderError::Http { status: 428, body: String::new() },
            ProviderError::Http { status: 499, body: String::new() },
            ProviderError::Refusal,
            ProviderError::Malformed("x".into()),
            ProviderError::Config("x".into()),
        ];
        for e in &retried {
            assert!(e.is_retryable(), "{e:?}");
        }
        for e in &final_ {
            assert!(!e.is_retryable(), "{e:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn server_errors_are_retried_and_client_errors_are_not() {
        let llm = FakeLlm::new(Ok("<output>Bonjour à tous, et merci.</output>".into()), 0);
        let stt = FakeStt::new(vec![Err(ProviderError::Http { status: 503, body: "down".into() }), Ok("bonjour à tous et merci".into())]);
        assert_eq!(run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await.unwrap().final_text, "Bonjour à tous, et merci.");
        assert_eq!(stt.calls(), 2);

        let stt = FakeStt::new(vec![Err(ProviderError::Http { status: 429, body: String::new() }), Ok("bonjour à tous et merci".into())]);
        assert!(run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await.is_ok());
        assert_eq!(stt.calls(), 2);

        let bad = ProviderError::Http { status: 400, body: "bad audio".into() };
        let stt = FakeStt::new(vec![Err(bad.clone())]);
        assert_eq!(run(&cfg(Level::Clean), b"wav", &[], None, &stt, &llm).await, Err(PipelineError::Transcription(bad)));
        assert_eq!(stt.calls(), 1);
    }
}
