import type { PresetView, Provider, Settings, Style, StyleCatalog } from "../src/lib/api";

const builtin = (id: Style["id"], keyword: string | null, aliases: string[] = []): Style => ({
  id, preset: id as Style["preset"], name: null, keyword, aliases, custom_task: null,
});

/** `style::catalog()`. */
export const STYLES: Style[] = [builtin("default", null), builtin("clean", "clean", ["propre"]), builtin("slack", "slack"), builtin("mail", "mail", ["email", "e-mail", "courriel"])];

/** `style_catalog().presets` in French (short tasks). */
export const PRESETS: PresetView[] = [
  { preset: "default", name: "Par défaut", task: "- Corrige et mets en forme." },
  { preset: "clean", name: "Propre", task: "- Corrige seulement." },
  { preset: "slack", name: "Slack", task: "- Rédige un message Slack." },
  { preset: "mail", name: "Mail", task: "- Mets en forme d'e-mail." },
];

export function styleCatalog(over: Partial<StyleCatalog> = {}): StyleCatalog {
  return { presets: PRESETS, styles: PRESETS.map((p) => ({ id: p.preset, name: p.name })), correction_enabled: true, ...over };
}

/** Backend defaults (settings.rs) with OpenAI transcription and Anthropic correction. */
export function settings(over: Partial<Settings> = {}): Settings {
  return {
    language: "system",
    trigger_keys: [0xa5],
    lock_vk: 0x20,
    gesture: { hold_threshold_ms: 300, double_tap_window_ms: 350, double_tap_enabled: true, lock_key_enabled: true },
    correction_enabled: true,
    styles: STYLES.map((st) => structuredClone(st)),
    offered_builtins: STYLES.map((st) => st.id),
    stt_provider: "openai",
    stt_model: "gpt-4o-transcribe",
    llm_provider: "anthropic",
    llm_model: "claude-sonnet-4-5",
    llm_effort: "low",
    restore_delay_ms: 150,
    min_recording_ms: 300,
    max_recording_ms: 600000,
    silence_threshold_dbfs: -50,
    llm_timeout_base_ms: 5000,
    llm_timeout_per_char_ms: 10,
    hint_budget_chars: 2000,
    audio_retention_days: 30,
    mute_audio_during_dictation: true,
    ...over,
  };
}

export const PROVIDERS: Provider[] = [
  { id: "openai", label: "OpenAI", base_url: "", stt_models: ["gpt-4o-transcribe", "whisper-1"], llm_api: "open_ai_chat", llm_models: [{ id: "gpt-5", efforts: [] }] },
  {
    id: "anthropic", label: "Anthropic", base_url: "", stt_models: [], llm_api: "anthropic",
    llm_models: [{ id: "claude-sonnet-4-5", efforts: ["low", "medium", "high"] }, { id: "claude-haiku-4-5", efforts: [] }],
  },
  { id: "mistral", label: "Mistral", base_url: "", stt_models: ["voxtral"], llm_api: "open_ai_chat", llm_models: [{ id: "mistral-large", efforts: [] }] },
  { id: "groq", label: "Groq", base_url: "", stt_models: ["whisper-large-v3"], llm_api: null, llm_models: [] },
];
