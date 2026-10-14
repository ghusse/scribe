import type { Settings } from "./api";
import { t } from "./i18n";

export interface NumberField {
  /** The backend default (settings.rs), quoted in the help. */
  default: number;
  min: number;
  max: number;
  integer: boolean;
  get: (s: Settings) => number;
}

/** Bounds mirror the HTML inputs and stay within settings.rs::validate. */
export const NUMBER_FIELDS = {
  hold_threshold_ms: { default: 300, min: 100, max: 2000, integer: true, get: (s) => s.gesture.hold_threshold_ms },
  double_tap_window_ms: { default: 350, min: 150, max: 1000, integer: true, get: (s) => s.gesture.double_tap_window_ms },
  restore_delay_ms: { default: 150, min: 0, max: 2000, integer: true, get: (s) => s.restore_delay_ms },
  audio_retention_days: { default: 30, min: 0, max: 36500, integer: true, get: (s) => s.audio_retention_days },
  max_recording_min: { default: 10, min: 1, max: 10, integer: false, get: (s) => s.max_recording_ms / 60000 },
  llm_timeout_s: { default: 5, min: 1, max: 60, integer: false, get: (s) => s.llm_timeout_base_ms / 1000 },
} satisfies Record<string, NumberField>;

export type NumberFieldId = keyof typeof NUMBER_FIELDS;

/** A field's label (form and messages), unit (after the input and in the messages) and optional grey help. */
export function fieldText(id: NumberFieldId): { label: string; unit: string; help?: string } {
  return t.value.settings.fields[id];
}

/** The grey help under a number field: its explanation, then the default value. */
export function fieldHelp(id: NumberFieldId): string {
  const { unit, help } = fieldText(id);
  const byDefault = t.value.settings.numbers.default(NUMBER_FIELDS[id].default, unit);
  return help ? `${help} ${byDefault}` : byDefault;
}

/** Choices of the audio retention select (0 = forever); a stored value outside them is kept as an extra option. */
export function retentionOptions(current: number): { value: number; label: string }[] {
  const m = t.value.settings.numbers;
  const days = [7, 30, 90];
  if (current > 0 && !days.includes(current)) days.push(current);
  days.sort((a, b) => a - b);
  return [...days.map((d) => ({ value: d, label: m.days(d) })), { value: 0, label: m.forever }];
}

export function rangeMessage(id: NumberFieldId): string {
  const { label, unit } = fieldText(id);
  const f = NUMBER_FIELDS[id];
  return t.value.settings.numbers.range(label, f.min, f.max, unit);
}

/** The value of a number input: NaN when empty or not a number (so the check rejects it). */
export function parseNumber(raw: string): number {
  return raw.trim() === "" ? NaN : Number(raw);
}

/** First number field out of its bounds, with a message quoting the range. */
export function checkNumbers(s: Settings): { field: NumberFieldId; message: string } | null {
  for (const [id, f] of Object.entries(NUMBER_FIELDS) as [NumberFieldId, NumberField][]) {
    const v = f.get(s);
    if (typeof v !== "number" || !Number.isFinite(v) || (f.integer && !Number.isInteger(v))) {
      const { label, unit } = fieldText(id);
      const m = t.value.settings.numbers;
      return { field: id, message: (f.integer ? m.enterInteger : m.enterNumber)(label, f.min, f.max, unit) };
    }
    if (v < f.min || v > f.max) return { field: id, message: rangeMessage(id) };
  }
  return null;
}

/** Hand-typed model id: trimmed, never empty. */
export function checkModelId(raw: string): { ok: true; value: string } | { ok: false; message: string } {
  const value = raw.trim();
  return value ? { ok: true, value } : { ok: false, message: t.value.settings.modelPicker.empty };
}
