import type { Settings } from "./api";

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
export function testSignature(s: Pick<Settings, "stt_provider" | "stt_model" | "llm_provider" | "llm_model" | "llm_effort" | "level">): string {
  return JSON.stringify([s.stt_provider, s.stt_model, s.llm_provider, s.llm_model, s.llm_effort, s.level]);
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
