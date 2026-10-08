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

export interface Settings {
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

export type ToastLevel = "info" | "uncertain" | "copied" | "error";
export type OverlayEvent =
  | { kind: "idle" }
  /** `warning`: shown under the pill for the whole recording (e.g. microphone not exclusive). */
  | { kind: "recording"; locked: boolean; warning: string | null }
  | { kind: "processing" }
  | { kind: "toast"; level: ToastLevel; message: string; preview: string | null; dictation_id: number | null };

type Result<T> = { Ok: T } | { Err: string };
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
  overlayDismiss: () => invoke<void>("overlay_dismiss"),
  openHistory: (id: number | null) => invoke<void>("open_history", { id }),
};
