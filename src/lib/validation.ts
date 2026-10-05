import type { Settings } from "./api";

export interface NumberField {
  /** Used both in the form label and in the error messages. */
  label: string;
  unit: string;
  /** Extra text in the form label's parentheses. */
  note?: string;
  min: number;
  max: number;
  integer: boolean;
  get: (s: Settings) => number;
}

/** Bounds mirror the HTML inputs and stay within settings.rs::validate. */
export const NUMBER_FIELDS = {
  hold_threshold_ms: { label: "Seuil de maintien", unit: "ms", min: 100, max: 2000, integer: true, get: (s) => s.gesture.hold_threshold_ms },
  double_tap_window_ms: { label: "Fenêtre de double-tap", unit: "ms", min: 150, max: 1000, integer: true, get: (s) => s.gesture.double_tap_window_ms },
  restore_delay_ms: { label: "Délai avant restauration du presse-papier", unit: "ms", min: 0, max: 2000, integer: true, get: (s) => s.restore_delay_ms },
  audio_retention_days: { label: "Conserver l'audio", unit: "jours", note: "0 = toujours", min: 0, max: 36500, integer: true, get: (s) => s.audio_retention_days },
  max_recording_min: { label: "Durée maximale d'une dictée", unit: "min", min: 1, max: 10, integer: false, get: (s) => s.max_recording_ms / 60000 },
} satisfies Record<string, NumberField>;

export type NumberFieldId = keyof typeof NUMBER_FIELDS;

export function formLabel(f: NumberField): string {
  return `${f.label} (${f.unit}${f.note ? `, ${f.note}` : ""})`;
}

export function rangeMessage(f: NumberField): string {
  return `${f.label} : entre ${f.min} et ${f.max} ${f.unit}`;
}

/** The value of a number input: NaN when empty or not a number (so the check rejects it). */
export function parseNumber(raw: string): number {
  return raw.trim() === "" ? NaN : Number(raw);
}

/** First number field out of its bounds, with a French message quoting the range. */
export function checkNumbers(s: Settings): { field: NumberFieldId; message: string } | null {
  for (const [id, f] of Object.entries(NUMBER_FIELDS) as [NumberFieldId, NumberField][]) {
    const v = f.get(s);
    if (typeof v !== "number" || !Number.isFinite(v) || (f.integer && !Number.isInteger(v))) {
      return { field: id, message: `${f.label} : entrez un nombre${f.integer ? " entier" : ""} entre ${f.min} et ${f.max} ${f.unit}` };
    }
    if (v < f.min || v > f.max) return { field: id, message: rangeMessage(f) };
  }
  return null;
}

/** Hand-typed model id: trimmed, never empty. */
export function checkModelId(raw: string): { ok: true; value: string } | { ok: false; message: string } {
  const value = raw.trim();
  return value ? { ok: true, value } : { ok: false, message: "Identifiant du modèle : ne peut pas être vide" };
}
