import { describe, expect, it } from "vitest";
import type { Settings } from "./api";
import { checkModelId, checkNumbers, fieldHelp, NUMBER_FIELDS, parseNumber, rangeMessage, retentionOptions } from "./validation";

const ok: Settings = {
  trigger_keys: [0xa3], lock_vk: 0x20,
  gesture: { hold_threshold_ms: 300, double_tap_window_ms: 350, double_tap_enabled: true, lock_key_enabled: true },
  level: "formatted", stt_provider: "openai", stt_model: "gpt-transcribe", llm_provider: "anthropic", llm_model: "claude-opus-5-5",
  llm_effort: "low", restore_delay_ms: 150, min_recording_ms: 300, max_recording_ms: 600000, silence_threshold_dbfs: -45,
  llm_timeout_base_ms: 5000, llm_timeout_per_char_ms: 5, hint_budget_chars: 800, audio_retention_days: 30,
};
const edited = (edit: (s: Settings) => void) => {
  const s = structuredClone(ok);
  edit(s);
  return s;
};

describe("checkNumbers", () => {
  it("accepts the defaults and the bounds themselves", () => {
    expect(checkNumbers(ok)).toBeNull();
    expect(checkNumbers(edited((s) => { s.gesture.hold_threshold_ms = 100; s.restore_delay_ms = 0; s.max_recording_ms = 60000; }))).toBeNull();
    expect(checkNumbers(edited((s) => { s.gesture.hold_threshold_ms = 2000; s.audio_retention_days = 36500; }))).toBeNull();
  });
  it("rejects the intermediate value of a half-typed number, quoting the range", () => {
    expect(checkNumbers(edited((s) => (s.gesture.hold_threshold_ms = 2)))).toEqual({
      field: "hold_threshold_ms", message: "Seuil de maintien : entre 100 et 2000 ms",
    });
  });
  it("rejects values above the max", () => {
    expect(checkNumbers(edited((s) => (s.gesture.double_tap_window_ms = 5000)))?.field).toBe("double_tap_window_ms");
    expect(checkNumbers(edited((s) => (s.restore_delay_ms = 2001)))?.message).toBe("Restauration du presse-papier : entre 0 et 2000 ms");
    expect(checkNumbers(edited((s) => (s.max_recording_ms = 11 * 60000)))?.message).toBe("Durée max. d'une dictée : entre 1 et 10 min");
  });
  it("rejects empty, decimal and negative values", () => {
    expect(checkNumbers(edited((s) => (s.audio_retention_days = NaN)))?.message).toBe("Conservation de l'audio : entrez un nombre entier entre 0 et 36500 jours");
    expect(checkNumbers(edited((s) => (s.gesture.hold_threshold_ms = 250.5)))?.field).toBe("hold_threshold_ms");
    expect(checkNumbers(edited((s) => (s.audio_retention_days = -1)))?.field).toBe("audio_retention_days");
    expect(checkNumbers(edited((s) => ((s as unknown as { restore_delay_ms: null }).restore_delay_ms = null)))?.field).toBe("restore_delay_ms");
  });
  it("allows fractional minutes", () => {
    expect(checkNumbers(edited((s) => (s.max_recording_ms = 90000)))).toBeNull();
  });
  it("bounds the correction timeout between 1 and 60 s, fractions allowed", () => {
    expect(checkNumbers(edited((s) => (s.llm_timeout_base_ms = 1000)))).toBeNull();
    expect(checkNumbers(edited((s) => (s.llm_timeout_base_ms = 2500)))).toBeNull();
    expect(checkNumbers(edited((s) => (s.llm_timeout_base_ms = 60000)))).toBeNull();
    expect(checkNumbers(edited((s) => (s.llm_timeout_base_ms = 500)))?.message).toBe("Délai max. de correction : entre 1 et 60 s");
    expect(checkNumbers(edited((s) => (s.llm_timeout_base_ms = 61000)))?.field).toBe("llm_timeout_s");
  });
});

describe("labels and messages share the constants", () => {
  it("builds both from the same field", () => {
    expect(fieldHelp(NUMBER_FIELDS.restore_delay_ms)).toBe("Augmentez si le texte collé est parfois l'ancien contenu. Défaut : 150 ms.");
    expect(fieldHelp(NUMBER_FIELDS.max_recording_min)).toBe("Défaut : 10 min.");
    expect(fieldHelp(NUMBER_FIELDS.llm_timeout_s)).toBe("Au-delà, le texte est collé sans correction. Défaut : 5 s.");
    expect(rangeMessage(NUMBER_FIELDS.double_tap_window_ms)).toBe("Fenêtre de double-tap : entre 150 et 1000 ms");
  });
});

describe("parseNumber", () => {
  it("maps an empty field to NaN", () => {
    expect(parseNumber("")).toBeNaN();
    expect(parseNumber("  ")).toBeNaN();
    expect(parseNumber("250")).toBe(250);
  });
});

describe("checkModelId", () => {
  it("trims and refuses empty ids", () => {
    expect(checkModelId("  gpt-4o-mini ")).toEqual({ ok: true, value: "gpt-4o-mini" });
    expect(checkModelId("   ").ok).toBe(false);
  });
});

describe("retentionOptions", () => {
  it("offers 7 / 30 / 90 days and « Toujours » (0)", () => {
    expect(retentionOptions(30)).toEqual([
      { value: 7, label: "7 jours" }, { value: 30, label: "30 jours" }, { value: 90, label: "90 jours" }, { value: 0, label: "Toujours" },
    ]);
    expect(retentionOptions(0).map((o) => o.value)).toEqual([7, 30, 90, 0]);
  });
  it("keeps a stored value that is not one of the choices", () => {
    expect(retentionOptions(1).map((o) => o.label)).toEqual(["1 jour", "7 jours", "30 jours", "90 jours", "Toujours"]);
    expect(retentionOptions(365).map((o) => o.value)).toEqual([7, 30, 90, 365, 0]);
  });
});
