import { invoke } from "@tauri-apps/api/core";

export type Level = "raw" | "clean" | "formatted";
export type Outcome = "pasted" | "pasted_uncertain" | "clipboard" | "error";

export interface Dictation {
  id: number;
  created_at: string;
  mode: string;
  app_name: string | null;
  audio_path: string | null;
  duration_ms: number;
  raw_text: string | null;
  final_text: string | null;
  edited_text: string | null;
  level: Level;
  transcriber: string | null;
  corrector: string | null;
  stt_ms: number | null;
  llm_ms: number | null;
  outcome: Outcome;
  /** A JSON `ProviderError` (`parseStoredError`); plain French text in older rows. */
  error: string | null;
}

export interface Term {
  id: number;
  term: string;
  variants: string[];
  note: string | null;
  source: "manual" | "correction" | "mined";
  use_count: number;
  last_used_at: string | null;
  created_at: string;
}

export interface GestureConfig {
  hold_threshold_ms: number;
  double_tap_window_ms: number;
  double_tap_enabled: boolean;
  lock_key_enabled: boolean;
}

/** UI language: « system » follows the OS. */
export type LanguagePref = "system" | Language;
export type Language = "en" | "fr";

export interface Settings {
  language: LanguagePref;
  /** Trigger combination (1 to 4 virtual-key codes, canonical order: Ctrl, Maj, Alt, Win, others). */
  trigger_keys: number[];
  lock_vk: number;
  gesture: GestureConfig;
  level: Level;
  stt_provider: string;
  stt_model: string;
  llm_provider: string;
  llm_model: string;
  llm_effort: string;
  restore_delay_ms: number;
  min_recording_ms: number;
  max_recording_ms: number;
  silence_threshold_dbfs: number;
  llm_timeout_base_ms: number;
  llm_timeout_per_char_ms: number;
  hint_budget_chars: number;
  audio_retention_days: number;
  /** Mutes every audio output while recording. */
  mute_audio_during_dictation: boolean;
  /** Other apps get no microphone input while recording (falls back to shared input when refused). */
  exclusive_microphone_during_dictation: boolean;
}

export interface LlmModel { id: string; efforts: string[] }
export interface Provider {
  id: string;
  label: string;
  base_url: string;
  /** Newest first: the first one is the default. */
  stt_models: string[];
  llm_api: "anthropic" | "open_ai_chat" | null;
  llm_models: LlmModel[];
}

/** Mirror of catalog::effort_levels: hand-typed Anthropic models get effort unless Haiku/3.x. */
export function effortLevels(p: Provider | undefined, model: string): string[] {
  if (!p) return [];
  const known = p.llm_models.find((m) => m.id === model);
  if (known) return known.efforts;
  if (p.llm_api === "anthropic" && !model.startsWith("claude-haiku") && !model.startsWith("claude-3")) return ["low", "medium", "high"];
  return [];
}

/**
 * Why a provider call (or the correction) failed; mirror of `scribe_core::pipeline::ProviderError`.
 * `detail` and `body` are untranslated technical text.
 */
export type ProviderError =
  | { code: "network"; detail: string }
  | { code: "timeout" }
  | { code: "auth" }
  | { code: "http"; status: number; body: string }
  | { code: "refusal" }
  | { code: "malformed"; detail: string }
  /** `provider`: the provider's label. */
  | { code: "missing_key"; provider: string }
  | { code: "unknown_provider"; role: ProviderRole; id: string }
  | { code: "correction_disabled" }
  /** The correction exceeded its own time budget (Advanced settings), unlike `timeout` (no HTTP answer). */
  | { code: "correction_timeout" }
  | { code: "no_output_tag" }
  | { code: "config"; detail: string };
export type ProviderRole = "transcription" | "correction";

/** Why `save_settings` refused the settings; mirror of `settings::SettingsError`. */
export type SettingsError =
  | { code: "invalid_hotkey" }
  | { code: "escape_in_hotkey" }
  | { code: "lock_key_in_hotkey" }
  | { code: "max_recording_too_short"; min_seconds: number }
  | { code: "max_recording_too_long"; max_minutes: number }
  | { code: "min_recording_not_below_max" }
  | { code: "silence_threshold_out_of_range"; min_dbfs: number; max_dbfs: number }
  | { code: "hotkey_delay_zero" }
  | { code: "audio_retention_too_long"; max_days: number }
  | { code: "restore_delay_too_long"; max_ms: number }
  | { code: "correction_timeout_too_long" }
  | { code: "unknown_provider"; role: ProviderRole; id: string }
  | { code: "model_required" };

/** A command's error (the rejection of `invoke`); mirror of `commands::AppError`. `detail` is untranslated. */
export type AppError =
  | { code: "system"; detail: string }
  | { code: "dictation_not_found" }
  | { code: "no_text" }
  | { code: "audio_purged" }
  | { code: "no_speech" }
  | { code: "duplicate_term"; term: string }
  | { code: "invalid_settings"; error: SettingsError }
  | { code: "provider"; error: ProviderError }
  | { code: "no_update" };

/** What a toast says; mirror of `overlay::Notice`. `correction`: why the text was not corrected. */
export type Notice =
  | { code: "pasted_without_correction"; correction: ProviderError }
  | { code: "pasted_uncertain"; correction: ProviderError | null }
  | { code: "copied"; correction: ProviderError | null }
  | { code: "not_pasted"; correction: ProviderError | null }
  | { code: "clipboard_unavailable"; correction: ProviderError | null }
  | { code: "transcription_failed"; error: ProviderError }
  | { code: "audio_encoding_failed"; detail: string }
  | { code: "microphone_unavailable"; detail: string }
  | { code: "recording_lost"; detail: string }
  | { code: "hook_unavailable"; detail: string }
  | { code: "update_available"; version: string }
  /** Raised by the overlay itself when its « Copy » button fails. */
  | { code: "copy_failed"; detail: string };

/** Shown under the pill for the whole recording. */
export type RecordingWarning = "shared_microphone";

export type ToastLevel = "info" | "uncertain" | "copied" | "error";
export type OverlayEvent =
  | { kind: "idle" }
  | { kind: "recording"; locked: boolean; warning: RecordingWarning | null }
  | { kind: "processing" }
  | { kind: "toast"; level: ToastLevel; message: Notice; preview: string | null; dictation_id: number | null };

type Result<T> = { Ok: T } | { Err: ProviderError };
/** Each call's duration in ms or its error; `llm` is null at the raw level (no correction tested). */
export interface ProviderTest { stt: Result<number>; llm: Result<number> | null }

export interface UpdateStatus {
  current: string;
  available: { version: string; notes: string | null } | null;
}

export type Permission = "accessibility" | "microphone";
export type PermissionState = "granted" | "not_determined" | "denied";
/** An OS permission (macOS privacy settings); Windows reports none. */
export interface PermissionStatus { permission: Permission; state: PermissionState }

export const api = {
  listDictations: (query: string | null, limit = 50, offset = 0) =>
    invoke<Dictation[]>("list_dictations", { query, limit, offset }),
  saveEditedText: (id: number, text: string | null) => invoke<void>("save_edited_text", { id, text }),
  deleteDictation: (id: number) => invoke<void>("delete_dictation", { id }),
  copyDictation: (id: number) => invoke<void>("copy_dictation", { id }),
  retranscribe: (id: number) => invoke<void>("retranscribe", { id }),
  listTerms: () => invoke<Term[]>("list_terms"),
  addTerm: (term: string, variants: string[], note: string | null) => invoke<number>("add_term", { term, variants, note }),
  updateTerm: (id: number, term: string, variants: string[], note: string | null) =>
    invoke<void>("update_term", { id, term, variants, note }),
  deleteTerm: (id: number) => invoke<void>("delete_term", { id }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  keyStatus: () => invoke<Record<string, boolean>>("key_status"),
  setApiKey: (provider: string, key: string) => invoke<void>("set_api_key", { provider, key }),
  testProviders: () => invoke<ProviderTest>("test_providers"),
  captureKey: () => invoke<number[] | null>("capture_key"),
  /** Makes a pending captureKey resolve to null at once. */
  cancelCapture: () => invoke<void>("cancel_capture"),
  providers: () => invoke<Provider[]>("providers"),
  getAutostart: () => invoke<boolean>("get_autostart"),
  checkUpdate: () => invoke<UpdateStatus>("check_update"),
  /** Installs the latest version and restarts Scribe. */
  installUpdate: () => invoke<void>("install_update"),
  /** Returns the state actually in place afterwards. */
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  permissions: () => invoke<PermissionStatus[]>("permissions"),
  /** Shows the OS prompt when it still can; returns the states right after (read again once answered). */
  requestPermission: (permission: Permission) => invoke<PermissionStatus[]>("request_permission", { permission }),
  openPermissionSettings: (permission: Permission) => invoke<void>("open_permission_settings", { permission }),
  /** The language the UI must use: the saved choice, else the system's. Changes are sent as the `language` event. */
  language: () => invoke<Language>("language"),
  overlayDismiss: () => invoke<void>("overlay_dismiss"),
  openHistory: (id: number | null) => invoke<void>("open_history", { id }),
};
