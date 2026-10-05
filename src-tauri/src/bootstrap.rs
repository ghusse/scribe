//! Decisions taken by the Tauri bootstrap (`main.rs`), kept out of it so they are unit-tested.
use scribe_core::model::Level;

use crate::settings::Settings;

/// Label of the settings/history window (see tauri.conf.json).
pub const MAIN_WINDOW: &str = "main";

/// First launch without the needed API keys: open the main window on start. Same rule as `missingKeys`
/// in src/lib/apiKeys.ts: the transcription key always, the correction key unless the level is raw.
pub fn needs_setup(settings: &Settings, has_key: impl Fn(&str) -> bool) -> bool {
    !has_key(&settings.stt_provider) || (settings.level != Level::Raw && !has_key(&settings.llm_provider))
}

/// Closing the main window only hides it: Scribe keeps running in the tray. Other windows (the overlay)
/// close normally.
pub fn hides_on_close(window_label: &str) -> bool {
    window_label == MAIN_WINDOW
}

/// Toast shown when the global keyboard hook cannot be installed.
pub fn hook_unavailable_message(error: &str) -> String {
    format!("Raccourci indisponible : {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(stt: &str, llm: &str, level: Level) -> Settings {
        Settings { stt_provider: stt.into(), llm_provider: llm.into(), level, ..Default::default() }
    }

    fn keys(stored: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |id| stored.contains(&id)
    }

    // Same case table as `missingKeys` in src/lib/apiKeys.test.ts.
    #[test]
    fn needs_setup_follows_the_key_usage_rule() {
        let s = settings("openai", "anthropic", Level::Formatted);
        assert!(needs_setup(&s, keys(&[])), "no key at all");
        assert!(needs_setup(&s, keys(&["openai"])), "correction key missing");
        assert!(needs_setup(&s, keys(&["anthropic"])), "transcription key missing");
        assert!(!needs_setup(&s, keys(&["openai", "anthropic"])), "both stored");
        let raw = settings("openai", "anthropic", Level::Raw);
        assert!(!needs_setup(&raw, keys(&["openai"])), "raw: no correction key needed");
        assert!(needs_setup(&raw, keys(&["anthropic"])), "raw: transcription key still needed");
        let same = settings("openai", "openai", Level::Formatted);
        assert!(!needs_setup(&same, keys(&["openai"])), "one provider for both roles");
    }

    #[test]
    fn needs_setup_asks_for_the_configured_providers() {
        let s = settings("mistral", "openai", Level::Formatted);
        let asked = std::cell::RefCell::new(Vec::new());
        needs_setup(&s, |id| {
            asked.borrow_mut().push(id.to_string());
            true
        });
        assert_eq!(asked.into_inner(), vec!["mistral", "openai"]);
    }

    #[test]
    fn only_the_main_window_hides_on_close() {
        assert!(hides_on_close("main"));
        assert!(!hides_on_close("overlay"));
        assert!(!hides_on_close("Main"));
    }

    #[test]
    fn hook_failure_message_names_the_error() {
        assert_eq!(hook_unavailable_message("accès refusé"), "Raccourci indisponible : accès refusé");
    }
}
