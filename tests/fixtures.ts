import type { Provider, Settings } from "../src/lib/api";

/** Backend defaults (settings.rs) with OpenAI transcription and Anthropic correction. */
export function settings(over: Partial<Settings> = {}): Settings {
  return {
    trigger_vk: 0xa5,
    lock_vk: 0x20,
    gesture: { hold_threshold_ms: 300, double_tap_window_ms: 350, double_tap_enabled: true, lock_key_enabled: true },
    level: "formatted",
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
