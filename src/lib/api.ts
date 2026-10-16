import { invoke } from "@tauri-apps/api/core";

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
  /** Id of the style whose task ran; null when correction was off. */
  style: string | null;
  /** Byte offset of the body in `raw_text` when a keyword was detected. */
  body_start: number | null;
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
  correction_enabled: boolean;
  /** The default style first; validated by the backend (`style::validate`). */
  styles: Style[];
  /** Ids of the built-in styles already offered to this user (backend bookkeeping, sent back as is). */
  offered_builtins: string[];
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
}

export type Preset = "default" | "clean" | "slack" | "mail";

export interface CustomTask {
  text: string;
  /** UI language when the text was written: the text keeps it when the UI language changes. */
  language: Language;
}

/** Mirror of `scribe_core::style::Style`. */
export interface Style {
  /** "default", a built-in id, or a UUID for a user style. */
  id: string;
  /** Built-in origin: name and task fallback, « reset » target. */
  preset: Preset | null;
  /** null: the preset's localized name. */
  name: string | null;
  /** null only for the default style. */
  keyword: string | null;
  aliases: string[];
  /** null: the preset's task in the current UI language. */
  custom_task: CustomTask | null;
}

/** A built-in preset in the current UI language. */
export interface PresetView { preset: Preset; name: string; task: string }
export interface StyleBadge { id: string; name: string }
export interface StyleCatalog { presets: PresetView[]; styles: StyleBadge[]; correction_enabled: boolean }

/** Mirror of `scribe_core::prompt::ImproveRequest`. */
export interface ImproveRequest { name: string; keyword: string | null; draft: string; instructions: string }

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
  | { code: "model_required" }
  /** Style errors (`style::StyleError`): `index` is the style's position in `Settings.styles`. */
  | { code: "default_style_missing" }
  | { code: "duplicate_style_id" }
  | { code: "too_many_styles"; max: number }
  | { code: "too_many_aliases"; index: number; max: number }
  | { code: "style_name_required"; index: number }
  | { code: "trigger_empty"; index: number }
  /** `trigger`: the later of the two, as typed. */
  | { code: "duplicate_trigger"; trigger: string }
  | { code: "task_empty"; index: number }
  | { code: "task_too_long"; index: number; max_chars: number };

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
  | { code: "no_update" }
  /** The style asked for is no longer in the settings. */
  | { code: "style_not_found" }
  /** The dictation has no transcript to rewrite. */
  | { code: "nothing_to_restyle" };

/** What a toast says; mirror of `overlay::Notice`. `correction`: why the text was not corrected. */
export type Notice =
  /** Clean paste of a dictation with a keyword style: hosts its badge. */
  | { code: "pasted" }
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
  /** A style switch replaced the pasted text in place. */
  | { code: "restyled" }
  /** A style switch could not replace the pasted text: the new text is in the clipboard. */
  | { code: "restyle_copied" }
  /** A style switch could not rewrite the text; nothing changed. */
  | { code: "restyle_failed"; error: ProviderError }
  /** Raised by the overlay itself when its « Copy » button fails. */
  | { code: "copy_failed"; detail: string }
  /** Raised by the overlay itself when the backend refuses a style switch (`restyle_dictation` rejected). */
  | { code: "restyle_refused"; detail: string };

/** Shown under the pill for the whole recording. */

export type ToastLevel = "info" | "uncertain" | "copied" | "error";
/** `style`: the keyword style of the dictation, when one was detected (never the default style). */
export type OverlayEvent =
  | { kind: "idle" }
  | { kind: "recording"; locked: boolean; style: StyleBadge | null }
  | { kind: "processing"; style: StyleBadge | null }
  | {
      kind: "toast";
      level: ToastLevel;
      message: Notice;
      preview: string | null;
      dictation_id: number | null;
      style: StyleBadge | null;
    };

type Result<T> = { Ok: T } | { Err: ProviderError };
/** Each call's duration in ms or its error; `llm` is null when correction is off (not tested). */
export interface ProviderTest { stt: Result<number>; llm: Result<number> | null }

export interface UpdateStatus {
  current: string;
  available: { version: string; notes: string | null } | null;
}

export type Permission = "accessibility" | "microphone";
export type PermissionState = "granted" | "not_determined" | "denied";
/** An OS permission (macOS privacy settings); Windows reports none. */
export interface PermissionStatus { permission: Permission; state: PermissionState }

/** An area of the overlay: `[x, y, width, height]` in CSS pixels. */
export type HitArea = [number, number, number, number];

/** The overlay's visible surface (null while idle: the window is transparent) and its buttons, in DOM order. */
export interface OverlayAreas { surface: HitArea | null; buttons: HitArea[] }

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
  /** Built-in presets, style names and the correction switch, in the current UI language. */
  styleCatalog: () => invoke<StyleCatalog>("style_catalog"),
  /** `styles` plus the missing built-ins whose id and keywords are free; nothing is saved. */
  restoreBuiltinStyles: (styles: Style[]) => invoke<Style[]>("restore_builtin_styles", { styles }),
  /** A new task text proposed by the corrector (only the correction key is needed). */
  improveStyle: (request: ImproveRequest) => invoke<string>("improve_style", { request }),
  overlayDismiss: () => invoke<void>("overlay_dismiss"),
  openHistory: (id: number | null) => invoke<void>("open_history", { id }),
  /** Rewrites a dictation with another style; the result arrives as an overlay toast. */
  restyleDictation: (id: number, styleId: string) => invoke<void>("restyle_dictation", { id, styleId }),
  overlayHitAreas: (areas: OverlayAreas) => invoke<void>("overlay_hit_areas", { ...areas }),
};
