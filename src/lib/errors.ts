import type { AppError, Notice, PresetView, ProviderError, SettingsError, Style } from "./api";
import { t } from "./i18n";
import { OS, type Os } from "./platform";
import { styleName, triggerOwner } from "./styles";

/** The styles a settings error refers to by index: the edited ones, named as in the Styles pane. */
export interface StyleContext {
  styles: Style[];
  presets: PresetView[];
}

/** Summary of a provider error; `provider` (its label) names who refused the key. */
export function providerErrorText(e: ProviderError, provider: string | null = null): string {
  const m = t.value.errors.provider;
  switch (e.code) {
    case "missing_key":
      return m.missingKey(e.provider);
    case "auth":
      return m.auth(provider);
    case "http":
      if (e.status === 401 || e.status === 403) return m.auth(provider);
      if (e.status === 404) return m.notFound;
      if (e.status === 400) return m.badRequest;
      if (e.status === 429) return m.tooManyRequests;
      return e.status >= 500 ? m.server(e.status) : m.http(e.status);
    case "network":
    case "timeout":
      return m.unreachable;
    case "unknown_provider":
      return m.unknownProvider;
    case "refusal":
      return m.refusal;
    case "malformed":
      return m.malformed;
    case "correction_disabled":
      return m.correctionDisabled;
    case "correction_timeout":
      return m.correctionTimeout;
    case "no_output_tag":
      return m.noOutputTag;
    case "config":
      return m.config;
  }
}

/** The untranslated technical text of a provider error, for a « Details » disclosure. */
export function providerErrorDetail(e: ProviderError): string | null {
  switch (e.code) {
    case "network":
    case "malformed":
    case "config":
      return e.detail;
    case "http":
      return `HTTP ${e.status}: ${e.body}`;
    default:
      return null;
  }
}

export function settingsErrorText(e: SettingsError, ctx: StyleContext = { styles: [], presets: [] }): string {
  const m = t.value.errors.settings;
  const name = (index: number) => styleName(ctx.styles[index], ctx.presets, index);
  switch (e.code) {
    case "invalid_hotkey":
      return m.invalidHotkey;
    case "escape_in_hotkey":
      return m.escapeInHotkey;
    case "lock_key_in_hotkey":
      return m.lockKeyInHotkey;
    case "max_recording_too_short":
      return m.maxRecordingTooShort(e.min_seconds);
    case "max_recording_too_long":
      return m.maxRecordingTooLong(e.max_minutes);
    case "min_recording_not_below_max":
      return m.minRecordingNotBelowMax;
    case "silence_threshold_out_of_range":
      return m.silenceThreshold(e.min_dbfs, e.max_dbfs);
    case "hotkey_delay_zero":
      return m.hotkeyDelayZero;
    case "audio_retention_too_long":
      return m.audioRetentionTooLong(e.max_days);
    case "restore_delay_too_long":
      return m.restoreDelayTooLong(e.max_ms);
    case "correction_timeout_too_long":
      return m.correctionTimeoutTooLong;
    case "unknown_provider":
      return m.unknownProvider(e.role, e.id);
    case "model_required":
      return m.modelRequired;
    case "default_style_missing":
      return m.defaultStyleMissing;
    case "duplicate_style_id":
      return m.duplicateStyleId;
    case "too_many_styles":
      return m.tooManyStyles(e.max);
    case "too_many_aliases":
      return m.tooManyAliases(name(e.index), e.max);
    case "style_name_required":
      return m.styleNameRequired(name(e.index));
    case "trigger_empty":
      return m.triggerEmpty(name(e.index));
    case "duplicate_trigger": {
      const owner = triggerOwner(ctx.styles, e.trigger);
      return m.duplicateTrigger(e.trigger, owner === null ? null : name(owner));
    }
    case "task_empty":
      return m.taskEmpty(name(e.index));
    case "task_too_long":
      return m.taskTooLong(name(e.index), e.max_chars);
  }
}

function appErrorText(e: AppError, ctx?: StyleContext): string {
  const m = t.value.errors.app;
  switch (e.code) {
    case "system":
      return e.detail;
    case "dictation_not_found":
      return m.dictationNotFound;
    case "no_text":
      return m.noText;
    case "audio_purged":
      return m.audioPurged;
    case "no_speech":
      return m.noSpeech;
    case "duplicate_term":
      return m.duplicateTerm(e.term);
    case "invalid_settings":
      return settingsErrorText(e.error, ctx);
    case "provider":
      return providerErrorText(e.error);
    case "no_update":
      return m.noUpdate;
  }
}

function isCoded(e: unknown): e is { code: string } {
  return typeof e === "object" && e !== null && typeof (e as { code?: unknown }).code === "string";
}

/** Text of a rejected command: an `AppError`, else whatever was thrown. `ctx` names the styles of a settings error. */
export function errorText(e: unknown, ctx?: StyleContext): string {
  return isCoded(e) ? appErrorText(e as AppError, ctx) : String(e);
}

/** A history row's error: a JSON `ProviderError`, or the plain text older versions stored. */
export function storedErrorText(stored: string): string {
  try {
    const e: unknown = JSON.parse(stored);
    if (isCoded(e)) return providerErrorText(e as ProviderError);
  } catch {
    // Older rows hold plain text.
  }
  return stored;
}

export function noticeText(n: Notice, os: Os = OS): string {
  const m = t.value.errors.notice;
  const note = (message: string, correction: ProviderError | null) =>
    correction ? m.notCorrected(message, providerErrorText(correction)) : message;
  switch (n.code) {
    case "pasted_without_correction":
      return m.pastedWithoutCorrection(providerErrorText(n.correction));
    case "pasted_uncertain":
      return note(m.pastedUncertain, n.correction);
    case "copied":
      return note(m.copied, n.correction);
    case "not_pasted":
      return note(m.notPasted(os === "mac" ? "⌘V" : "Ctrl+V"), n.correction);
    case "clipboard_unavailable":
      return note(m.clipboardUnavailable, n.correction);
    case "transcription_failed":
      return m.transcriptionFailed(providerErrorText(n.error));
    case "audio_encoding_failed":
      return m.audioEncodingFailed(n.detail);
    case "microphone_unavailable":
      return m.microphoneUnavailable(n.detail);
    case "recording_lost":
      return m.recordingLost(n.detail);
    case "hook_unavailable":
      return m.hookUnavailable(n.detail);
    case "update_available":
      return m.updateAvailable(n.version);
    case "copy_failed":
      return m.copyFailed(n.detail);
  }
}
