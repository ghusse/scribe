use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::lang::Language;
use crate::model::Term;
use crate::prompt::{self, CorrectionPrompt};
use crate::style::{self, Style};

/// Which provider a setting or an error is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRole {
    Transcription,
    Correction,
}

impl std::fmt::Display for ProviderRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ProviderRole::Transcription => "transcription",
            ProviderRole::Correction => "correction",
        })
    }
}

/// Why a provider call (or the correction) failed. Sent to the UI as a code it translates (`src/lib/api.ts`), and
/// stored as JSON in `dictations.error`; `Display` is for the logs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ProviderError {
    #[error("network error: {detail}")]
    Network { detail: String },
    #[error("timed out")]
    Timeout,
    #[error("API key rejected")]
    Auth,
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("the model refused the request")]
    Refusal,
    #[error("invalid response: {detail}")]
    Malformed { detail: String },
    /// `provider`: the provider's label.
    #[error("missing {provider} API key")]
    MissingKey { provider: String },
    #[error("unknown {role} provider: {id}")]
    UnknownProvider { role: ProviderRole, id: String },
    #[error("correction disabled")]
    CorrectionDisabled,
    /// The correction exceeded its own budget (`llm_timeout`), unlike `Timeout`, the HTTP client's.
    #[error("correction took too long")]
    CorrectionTimeout,
    #[error("corrector response without an <output> tag")]
    NoOutputTag,
    #[error("configuration: {detail}")]
    Config { detail: String },
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Network { .. } | ProviderError::Timeout => true,
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
    /// The model called; None when the correction cannot reach one (turned off, missing key, unknown provider).
    fn name(&self) -> Option<String>;
    /// Returns the raw model response; the pipeline extracts the `<output>` part.
    async fn correct(&self, prompt: &CorrectionPrompt) -> Result<String, ProviderError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    pub correction_enabled: bool,
    pub hint_budget_chars: usize,
    pub llm_timeout_base_ms: u64,
    pub llm_timeout_per_char_ms: u64,
    pub stt_retry_delay_ms: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            correction_enabled: true,
            hint_budget_chars: 800,
            llm_timeout_base_ms: 3_000,
            llm_timeout_per_char_ms: 5,
            stt_retry_delay_ms: 400,
        }
    }
}

pub fn llm_timeout(cfg: &PipelineConfig, text: &str) -> Duration {
    let per_char = cfg.llm_timeout_per_char_ms.saturating_mul(text.chars().count() as u64);
    Duration::from_millis(cfg.llm_timeout_base_ms.saturating_add(per_char))
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineOutput {
    pub raw: String,
    pub final_text: String,
    pub stt_ms: u64,
    pub llm_ms: Option<u64>,
    /// Set when the corrector failed and `final_text` is the uncorrected body.
    pub correction_error: Option<ProviderError>,
    /// Id of the style whose task ran; None when correction is off.
    pub style: Option<String>,
    /// Byte offset of the body in `raw` when a style keyword was detected.
    pub body_start: Option<usize>,
}

/// What the corrector needs besides the text: the styles and the prompt context.
#[derive(Debug, Clone, Copy)]
pub struct StyleInputs<'a> {
    pub styles: &'a [Style],
    /// UI language: the prompt frame's, and the built-in tasks'.
    pub language: Language,
    pub terms: &'a [Term],
    pub app_name: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
    pub final_text: String,
    pub llm_ms: Option<u64>,
    /// Set when the corrector failed and `final_text` is the text given.
    pub error: Option<ProviderError>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PipelineError {
    #[error("transcription: {0}")]
    Transcription(ProviderError),
    #[error("no speech detected")]
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
    let normalized = lowered.trim_end_matches(|c: char| matches!(c, '.' | '!' | '?') || c.is_whitespace()).trim();
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
            let Some((start, _)) = body.char_indices().rev().nth(n - 1) else {
                continue;
            };
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

/// Transcribes with one retry on a retryable error and strips trailing hallucinations; blank → `Empty`.
/// Returns the transcript and the transcription time in ms.
pub async fn transcribe(
    cfg: &PipelineConfig,
    wav: &[u8],
    hints: &[String],
    transcriber: &dyn Transcriber,
) -> Result<(String, u64), PipelineError> {
    let t0 = Instant::now();
    let raw = match transcriber.transcribe(wav, hints).await {
        Ok(t) => t,
        Err(e) if e.is_retryable() => {
            tokio::time::sleep(Duration::from_millis(cfg.stt_retry_delay_ms)).await;
            transcriber.transcribe(wav, hints).await.map_err(PipelineError::Transcription)?
        }
        Err(e) => return Err(PipelineError::Transcription(e)),
    };
    let stt_ms = t0.elapsed().as_millis() as u64;
    let raw = strip_trailing_hallucinations(&raw);
    if is_blank_or_hallucination(&raw) {
        return Err(PipelineError::Empty);
    }
    Ok((raw, stt_ms))
}

/// One corrector call with `style`'s task, no short-text bypass. On any error the text comes back unchanged.
pub async fn correct(
    cfg: &PipelineConfig,
    text: &str,
    style: &Style,
    inp: &StyleInputs<'_>,
    corrector: &dyn Corrector,
) -> Correction {
    let (task, _) = style::task_text(style, inp.language);
    let p = prompt::build_correction_prompt(text, inp.terms, inp.app_name, task, inp.language);
    let t0 = Instant::now();
    let result = tokio::time::timeout(llm_timeout(cfg, text), corrector.correct(&p)).await;
    let llm_ms = Some(t0.elapsed().as_millis() as u64);
    let (final_text, error) = match result {
        Ok(Ok(resp)) => match prompt::extract_output(&resp) {
            Some(out) => (out, None),
            None => (text.to_string(), Some(ProviderError::NoOutputTag)),
        },
        Ok(Err(e)) => (text.to_string(), Some(e)),
        Err(_) => (text.to_string(), Some(ProviderError::CorrectionTimeout)),
    };
    Correction { final_text, llm_ms, error }
}

/// Transcribes, detects the style keyword, then corrects the body with that style's task (the default style's
/// when none was said). The keyword and its leading fillers are stripped even with correction off.
pub async fn run(
    cfg: &PipelineConfig,
    wav: &[u8],
    inp: &StyleInputs<'_>,
    transcriber: &dyn Transcriber,
    corrector: &dyn Corrector,
) -> Result<PipelineOutput, PipelineError> {
    let hints = prompt::transcriber_hints(&style::keywords(inp.styles), inp.terms, cfg.hint_budget_chars);
    let (raw, stt_ms) = transcribe(cfg, wav, &hints, transcriber).await?;
    let detected = style::detect(&raw, inp.styles);
    let body_start = detected.map(|d| d.body_start);
    let body = detected.map_or(raw.as_str(), |d| style::body(&raw, &d)).to_string();
    let output = |final_text: String, style: Option<&Style>, llm_ms, correction_error| PipelineOutput {
        raw: raw.clone(),
        final_text,
        stt_ms,
        llm_ms,
        correction_error,
        style: style.map(|s| s.id.clone()),
        body_start,
    };
    if !cfg.correction_enabled {
        return Ok(output(body, None, None, None));
    }
    let fallback;
    let style = match detected {
        Some(d) => d.style,
        None => match style::find(inp.styles, style::DEFAULT_STYLE_ID) {
            Some(s) => s,
            None => {
                fallback = style::catalog().remove(0);
                &fallback
            }
        },
    };
    if detected.is_none() && is_short(&raw) {
        return Ok(output(body, Some(style), None, None));
    }
    let c = correct(cfg, &body, style, inp, corrector).await;
    Ok(output(c.final_text, Some(style), c.llm_ms, c.error))
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
        fn name(&self) -> String {
            "fake-stt".into()
        }
        async fn transcribe(&self, _wav: &[u8], hints: &[String]) -> Result<String, ProviderError> {
            self.hints_seen.lock().unwrap().push(hints.to_vec());
            self.responses.lock().unwrap().pop_front().expect("unexpected call")
        }
    }

    struct FakeLlm {
        response: Result<String, ProviderError>,
        delay_ms: u64,
        calls: AtomicUsize,
        prompts: Mutex<Vec<CorrectionPrompt>>,
    }
    impl FakeLlm {
        fn new(response: Result<String, ProviderError>, delay_ms: u64) -> Self {
            Self { response, delay_ms, calls: AtomicUsize::new(0), prompts: Mutex::new(vec![]) }
        }
        fn last_prompt(&self) -> CorrectionPrompt {
            self.prompts.lock().unwrap().last().expect("corrector called").clone()
        }
    }
    #[async_trait]
    impl Corrector for FakeLlm {
        fn name(&self) -> Option<String> {
            Some("fake-llm".into())
        }
        async fn correct(&self, p: &CorrectionPrompt) -> Result<String, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.prompts.lock().unwrap().push(p.clone());
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            self.response.clone()
        }
    }

    fn cfg(correction_enabled: bool) -> PipelineConfig {
        PipelineConfig { correction_enabled, ..Default::default() }
    }

    async fn go(
        cfg: &PipelineConfig,
        terms: &[Term],
        app_name: Option<&str>,
        stt: &FakeStt,
        llm: &FakeLlm,
    ) -> Result<PipelineOutput, PipelineError> {
        let styles = style::catalog();
        let inp = StyleInputs { styles: &styles, language: Language::Fr, terms, app_name };
        run(cfg, b"wav", &inp, stt, llm).await
    }

    fn term(id: i64, t: &str) -> Term {
        Term {
            id,
            term: t.into(),
            variants: vec![],
            note: None,
            source: TermSource::Manual,
            use_count: 0,
            last_used_at: None,
            created_at: "2026-10-05T10:00:00.000Z".into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn without_keyword_the_default_style_corrects_the_raw_text() {
        let stt = FakeStt::new(vec![Ok(" cube ernetes c'est top ".into())]);
        let llm = FakeLlm::new(Ok("<output>Kubernetes, c'est top.</output>".into()), 10);
        let out = go(&cfg(true), &[term(1, "Kubernetes")], Some("Code"), &stt, &llm).await.unwrap();
        assert_eq!(out.raw, "cube ernetes c'est top");
        assert_eq!(out.final_text, "Kubernetes, c'est top.");
        assert_eq!(out.correction_error, None);
        assert!(out.llm_ms.is_some());
        assert_eq!((out.style.as_deref(), out.body_start), (Some("default"), None));
        assert_eq!(stt.hints_seen.lock().unwrap()[0], ["clean", "slack", "mail", "Kubernetes"], "keywords first");
        let p = llm.last_prompt();
        assert!(p.system.contains(style::preset_task(style::Preset::Default, Language::Fr)));
        assert!(p.user.contains("<transcript>\ncube ernetes c'est top\n</transcript>"));
    }

    #[tokio::test(start_paused = true)]
    async fn correction_off_skips_the_corrector() {
        let stt = FakeStt::new(vec![Ok("bonjour".into())]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        let out = go(&cfg(false), &[], None, &stt, &llm).await.unwrap();
        assert_eq!((out.final_text.as_str(), out.llm_ms, out.style, out.body_start), ("bonjour", None, None, None));
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn short_transcript_skips_corrector() {
        for raw in ["OK, merci beaucoup.", "à demain", "Oui."] {
            let stt = FakeStt::new(vec![Ok(raw.into())]);
            let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
            let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
            assert_eq!((out.final_text.as_str(), out.llm_ms, out.correction_error), (raw, None, None), "{raw}");
            assert_eq!(out.style.as_deref(), Some("default"), "{raw}");
            assert_eq!(llm.calls.load(Ordering::SeqCst), 0, "{raw}");
        }
        let stt = FakeStt::new(vec![Ok("OK, merci beaucoup Paul.".into())]);
        let llm = FakeLlm::new(Ok("<output>OK, merci beaucoup Paul !</output>".into()), 0);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "OK, merci beaucoup Paul !");
        assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_short_dictation_with_a_keyword_is_still_corrected_without_the_keyword() {
        let stt = FakeStt::new(vec![Ok("Euh, Slack : on décale.".into())]);
        let llm = FakeLlm::new(Ok("<output>On décale.</output>".into()), 0);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "On décale.");
        assert_eq!((out.style.as_deref(), out.body_start), (Some("slack"), Some(13)));
        assert_eq!(&out.raw[13..], "on décale.");
        let p = llm.last_prompt();
        assert!(p.user.contains("<transcript>\non décale.\n</transcript>"), "the LLM gets the body only");
        assert!(p.system.contains(style::preset_task(style::Preset::Slack, Language::Fr)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_keyword_alone_is_a_normal_dictation() {
        let stt = FakeStt::new(vec![Ok("Slack.".into())]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!((out.final_text.as_str(), out.style.as_deref(), out.body_start), ("Slack.", Some("default"), None));
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0, "short bypass applies");
    }

    #[tokio::test(start_paused = true)]
    async fn a_correction_error_falls_back_to_the_body() {
        let stt = FakeStt::new(vec![Ok("Mail, bonjour Paul, à demain.".into())]);
        let llm = FakeLlm::new(Err(ProviderError::Auth), 0);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour Paul, à demain.");
        assert_eq!((out.correction_error, out.style.as_deref()), (Some(ProviderError::Auth), Some("mail")));
    }

    #[tokio::test(start_paused = true)]
    async fn correction_off_strips_the_keyword_and_fillers() {
        let stt = FakeStt::new(vec![Ok("Alors, en mode Slack : salut.".into())]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        let out = go(&cfg(false), &[], None, &stt, &llm).await.unwrap();
        assert_eq!((out.final_text.as_str(), out.style, out.llm_ms), ("salut.", None, None));
        assert_eq!(out.body_start, Some(out.raw.len() - "salut.".len()));
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_style_task_and_ui_language_reach_the_prompt() {
        let mut styles = style::catalog();
        styles[2].custom_task =
            Some(style::CustomTask { text: "- Translate to English.".into(), language: Language::En });
        let inp = StyleInputs { styles: &styles, language: Language::En, terms: &[], app_name: Some("Slack") };
        let stt = FakeStt::new(vec![Ok("slack bonjour".into())]);
        let llm = FakeLlm::new(Ok("<output>Hello</output>".into()), 0);
        let out = run(&cfg(true), b"wav", &inp, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "Hello");
        let p = llm.last_prompt();
        assert!(p.system.contains("Task:\n- Translate to English."));
        assert!(p.user.starts_with("Target application: Slack"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_missing_default_style_falls_back_to_the_built_in_one() {
        let styles = vec![style::catalog().remove(2)];
        let inp = StyleInputs { styles: &styles, language: Language::Fr, terms: &[], app_name: None };
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Ok("<output>Bonjour à tous, et merci.</output>".into()), 0);
        let out = run(&cfg(true), b"wav", &inp, &stt, &llm).await.unwrap();
        assert_eq!(out.style.as_deref(), Some("default"));
        assert!(llm.last_prompt().system.contains(style::preset_task(style::Preset::Default, Language::Fr)));
    }

    #[tokio::test(start_paused = true)]
    async fn correct_returns_the_text_unchanged_on_error() {
        let styles = style::catalog();
        let inp = StyleInputs { styles: &styles, language: Language::Fr, terms: &[], app_name: None };
        let llm = FakeLlm::new(Ok("pas de balise".into()), 0);
        let c = correct(&cfg(true), "ok", &styles[1], &inp, &llm).await;
        assert_eq!((c.final_text.as_str(), c.error), ("ok", Some(ProviderError::NoOutputTag)));
        assert!(c.llm_ms.is_some());
        assert!(llm.last_prompt().system.contains(style::preset_task(style::Preset::Clean, Language::Fr)));
    }

    #[tokio::test(start_paused = true)]
    async fn transcribe_returns_the_cleaned_transcript() {
        let stt = FakeStt::new(vec![Ok(" Bonjour. Thanks for watching ".into())]);
        let (text, _) = transcribe(&cfg(true), b"wav", &["Slack".into()], &stt).await.unwrap();
        assert_eq!(text, "Bonjour.");
        assert_eq!(stt.hints_seen.lock().unwrap()[0], ["Slack"]);
    }

    #[tokio::test(start_paused = true)]
    async fn corrector_error_falls_back_to_raw() {
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Err(ProviderError::Http { status: 529, body: "overloaded".into() }), 0);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour à tous et merci");
        assert_eq!(out.correction_error, Some(ProviderError::Http { status: 529, body: "overloaded".into() }));
    }

    #[tokio::test(start_paused = true)]
    async fn corrector_timeout_falls_back_to_raw() {
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Ok("<output>Bonjour.</output>".into()), 60_000);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour à tous et merci");
        assert_eq!(out.correction_error, Some(ProviderError::CorrectionTimeout));
    }

    #[tokio::test(start_paused = true)]
    async fn corrector_without_output_tags_falls_back_to_raw() {
        let stt = FakeStt::new(vec![Ok("bonjour à tous et merci".into())]);
        let llm = FakeLlm::new(Ok("Voici le texte corrigé : Bonjour.".into()), 0);
        let out = go(&cfg(true), &[], None, &stt, &llm).await.unwrap();
        assert_eq!(out.final_text, "bonjour à tous et merci");
        assert_eq!(out.correction_error, Some(ProviderError::NoOutputTag));
    }

    #[tokio::test(start_paused = true)]
    async fn retryable_transcription_error_is_retried_once() {
        let stt = FakeStt::new(vec![Err(ProviderError::Timeout), Ok("bonjour".into())]);
        let llm = FakeLlm::new(Ok("<output>Bonjour.</output>".into()), 0);
        assert!(go(&cfg(true), &[], None, &stt, &llm).await.is_ok());
        assert_eq!(stt.calls(), 2);

        let stt = FakeStt::new(vec![
            Err(ProviderError::Network { detail: "x".into() }),
            Err(ProviderError::Network { detail: "y".into() }),
        ]);
        assert_eq!(
            go(&cfg(true), &[], None, &stt, &llm).await,
            Err(PipelineError::Transcription(ProviderError::Network { detail: "y".into() }))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn auth_error_is_not_retried() {
        let stt = FakeStt::new(vec![Err(ProviderError::Auth)]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        assert_eq!(go(&cfg(true), &[], None, &stt, &llm).await, Err(PipelineError::Transcription(ProviderError::Auth)));
        assert_eq!(stt.calls(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn blank_or_hallucinated_transcript_is_empty() {
        for raw in ["", "   ", "Sous-titres réalisés par la communauté d'Amara.org", "Merci d’avoir regardé !"] {
            let stt = FakeStt::new(vec![Ok(raw.into())]);
            let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
            assert_eq!(go(&cfg(true), &[], None, &stt, &llm).await, Err(PipelineError::Empty), "{raw}");
            assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn trailing_credit_after_real_speech_is_removed() {
        let stt = FakeStt::new(vec![Ok("Bonjour à tous. Sous-titres réalisés par la communauté d'Amara.org".into())]);
        let llm = FakeLlm::new(Ok("<output>X</output>".into()), 0);
        let out = go(&cfg(false), &[], None, &stt, &llm).await.unwrap();
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
            ProviderError::Network { detail: "connection reset".into() },
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
            ProviderError::Malformed { detail: "x".into() },
            ProviderError::MissingKey { provider: "OpenAI".into() },
            ProviderError::UnknownProvider { role: ProviderRole::Correction, id: "x".into() },
            ProviderError::CorrectionDisabled,
            ProviderError::CorrectionTimeout,
            ProviderError::NoOutputTag,
            ProviderError::Config { detail: "x".into() },
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
        let stt = FakeStt::new(vec![
            Err(ProviderError::Http { status: 503, body: "down".into() }),
            Ok("bonjour à tous et merci".into()),
        ]);
        assert_eq!(go(&cfg(true), &[], None, &stt, &llm).await.unwrap().final_text, "Bonjour à tous, et merci.");
        assert_eq!(stt.calls(), 2);

        let stt = FakeStt::new(vec![
            Err(ProviderError::Http { status: 429, body: String::new() }),
            Ok("bonjour à tous et merci".into()),
        ]);
        assert!(go(&cfg(true), &[], None, &stt, &llm).await.is_ok());
        assert_eq!(stt.calls(), 2);

        let bad = ProviderError::Http { status: 400, body: "bad audio".into() };
        let stt = FakeStt::new(vec![Err(bad.clone())]);
        assert_eq!(go(&cfg(true), &[], None, &stt, &llm).await, Err(PipelineError::Transcription(bad)));
        assert_eq!(stt.calls(), 1);
    }

    #[test]
    fn provider_errors_serialize_as_codes_for_the_ui() {
        use serde_json::json;
        let cases = [
            (ProviderError::Network { detail: "reset".into() }, json!({"code": "network", "detail": "reset"})),
            (ProviderError::Timeout, json!({"code": "timeout"})),
            (ProviderError::CorrectionTimeout, json!({"code": "correction_timeout"})),
            (ProviderError::Auth, json!({"code": "auth"})),
            (
                ProviderError::Http { status: 503, body: "down".into() },
                json!({"code": "http", "status": 503, "body": "down"}),
            ),
            (ProviderError::Refusal, json!({"code": "refusal"})),
            (ProviderError::Malformed { detail: "eof".into() }, json!({"code": "malformed", "detail": "eof"})),
            (
                ProviderError::MissingKey { provider: "OpenAI".into() },
                json!({"code": "missing_key", "provider": "OpenAI"}),
            ),
            (
                ProviderError::UnknownProvider { role: ProviderRole::Transcription, id: "x".into() },
                json!({"code": "unknown_provider", "role": "transcription", "id": "x"}),
            ),
            (
                ProviderError::UnknownProvider { role: ProviderRole::Correction, id: "y".into() },
                json!({"code": "unknown_provider", "role": "correction", "id": "y"}),
            ),
            (ProviderError::CorrectionDisabled, json!({"code": "correction_disabled"})),
            (ProviderError::NoOutputTag, json!({"code": "no_output_tag"})),
            (ProviderError::Config { detail: "tls".into() }, json!({"code": "config", "detail": "tls"})),
        ];
        for (e, expected) in cases {
            assert_eq!(serde_json::to_value(&e).unwrap(), expected, "{e:?}");
            assert_eq!(serde_json::from_value::<ProviderError>(expected).unwrap(), e, "round trip");
        }
    }

    #[test]
    fn errors_read_in_english_in_the_logs() {
        assert_eq!(ProviderError::Http { status: 404, body: "nope".into() }.to_string(), "HTTP 404: nope");
        assert_eq!(
            ProviderError::UnknownProvider { role: ProviderRole::Correction, id: "x".into() }.to_string(),
            "unknown correction provider: x"
        );
        assert_eq!(PipelineError::Transcription(ProviderError::Auth).to_string(), "transcription: API key rejected");
        assert_eq!(PipelineError::Empty.to_string(), "no speech detected");
    }
}
