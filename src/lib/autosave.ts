import type { Settings } from "./api";
import { t } from "./i18n";

export type SaveState = "saved" | "pending" | "saving" | "invalid" | "failed";

/**
 * What an edit of the settings means for the autosave, given the JSON of the edited
 * settings and of the last saved ones:
 * - "revert": back to the saved value: cancel the pending save, show « enregistré » again;
 * - "ignore": same as saved but a save is in flight: its follow-up will settle the state;
 * - "schedule": a real change: (re)start the debounce.
 */
export function editAction(json: string, lastSaved: string, inFlight: boolean): "revert" | "ignore" | "schedule" {
  if (json !== lastSaved) return "schedule";
  return inFlight ? "ignore" : "revert";
}

/** States the user must see even from another tab: the edits are not saved. */
export function needsAttention(state: SaveState): boolean {
  return state === "invalid" || state === "failed";
}

/** The settings a provider test depends on: when it changes, a displayed test result is stale. */
export function testSignature(s: Pick<Settings, "stt_provider" | "stt_model" | "llm_provider" | "llm_model" | "llm_effort" | "correction_enabled">): string {
  return JSON.stringify([s.stt_provider, s.stt_model, s.llm_provider, s.llm_model, s.llm_effort, s.correction_enabled]);
}

/**
 * Drops late async results: take a token before the call, `invalidate()` whenever what the result
 * depends on changes (settings, a saved or deleted key), and keep the result only if still current.
 */
export function staleGuard() {
  let gen = 0;
  return {
    token: () => gen,
    invalidate: () => void gen++,
    isCurrent: (t: number) => t === gen,
  };
}

/** How long « ✓ Réglages enregistrés » stays in the status bar after a save. */
export const SAVED_FADE_MS = 2000;

export interface StatusView {
  tone: "muted" | "success" | "danger";
  /** One line (the bar ellipsizes it); empty when the bar is hidden. */
  text: string;
  /** Errors are announced as alerts, progress and success as polite status. */
  alert: boolean;
  /** « Réessayer » re-sends a payload the backend failed to save. */
  retry: boolean;
  /** Full error text for the « Détails » disclosure. */
  details: string | null;
  visible: boolean;
}

/**
 * The sticky status bar: progress while saving, a success that fades out (`flashing` is true
 * during the SAVED_FADE_MS after a save), errors that stay until fixed.
 */
export function statusView(state: SaveState, error: string | null, flashing: boolean): StatusView {
  const m = t.value.settings.status;
  const base = { alert: false, retry: false, details: null, visible: true };
  switch (state) {
    case "pending":
    case "saving":
      return { ...base, tone: "muted", text: m.saving };
    case "saved":
      // The text stays while hidden so the bar fades out with the message still in it.
      return { ...base, tone: "success", text: m.saved, visible: flashing };
    case "invalid":
      return { ...base, tone: "danger", text: m.notSaved(error ?? m.invalidValue), alert: true };
    case "failed": {
      const full = error ?? m.unknownError;
      const first = full.split("\n")[0];
      return { tone: "danger", text: m.notSaved(first), alert: true, retry: true, details: full, visible: true };
    }
  }
}
