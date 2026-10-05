import type { Provider, Settings } from "./api";

export type KeyRole = "transcription" | "correction";
export const ROLE_LABELS: Record<KeyRole, string> = { transcription: "Transcription", correction: "Correction" };

type UsageSettings = Pick<Settings, "stt_provider" | "llm_provider" | "level">;

/**
 * Providers whose key the current settings need, with what for. Same rule as needs_setup in
 * main.rs: transcription always, correction unless the level is raw.
 */
export function keyUsage(s: UsageSettings): Record<string, KeyRole[]> {
  const usage: Record<string, KeyRole[]> = { [s.stt_provider]: ["transcription"] };
  if (s.level !== "raw") (usage[s.llm_provider] ??= []).push("correction");
  return usage;
}

/** Required keys that are not stored, in usage order (transcription first). */
export function missingKeys(s: UsageSettings, keyStatus: Record<string, boolean>): { provider: string; roles: KeyRole[] }[] {
  return Object.entries(keyUsage(s))
    .filter(([id]) => !keyStatus[id])
    .map(([provider, roles]) => ({ provider, roles }));
}

/** Status of one provider's key: stored, missing but needed, or missing and unused. */
export function keyState(id: string, s: UsageSettings, keyStatus: Record<string, boolean>): "saved" | "required" | "unconfigured" {
  if (keyStatus[id]) return "saved";
  return keyUsage(s)[id] ? "required" : "unconfigured";
}

/** A key can be saved only when something was typed (an empty key would delete the stored one). */
export function canSaveKey(draft: string | undefined): boolean {
  return (draft ?? "").trim() !== "";
}

export function keyPlaceholder(stored: boolean): string {
  return stored ? "Remplacer la clé…" : "Coller la clé API…";
}

export function deleteKeyQuestion(label: string): string {
  return `Supprimer la clé ${label} ?`;
}

/** Providers with a typed but unsaved key (a test would run without it), in catalog order. */
export function unsavedDrafts(drafts: Record<string, string | undefined>, providers: Provider[]): Provider[] {
  return providers.filter((p) => canSaveKey(drafts[p.id]));
}

export function unsavedDraftMessage(unsaved: Provider[]): string | null {
  if (unsaved.length === 0) return null;
  if (unsaved.length === 1) return `Clé ${unsaved[0].label} saisie mais non enregistrée. Enregistrer ?`;
  const names = unsaved.map((p) => p.label);
  return `Clés ${names.slice(0, -1).join(", ")} et ${names[names.length - 1]} saisies mais non enregistrées. Enregistrer ?`;
}
