import type { Provider, Settings } from "./api";

export type KeyRole = "transcription" | "correction";
export const ROLE_LABELS: Record<KeyRole, string> = { transcription: "Transcription", correction: "Correction" };

type UsageSettings = Pick<Settings, "stt_provider" | "llm_provider" | "level">;

/**
 * Providers whose key the current settings need, with what for. Same rule as needs_setup in
 * src-tauri/src/bootstrap.rs: transcription always, correction unless the level is raw.
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

/**
 * First launch: open on the settings when a required key is missing. Same rule and case table as
 * needs_setup in src-tauri/src/bootstrap.rs (which shows the main window at startup).
 */
export function needsSetup(s: UsageSettings, keyStatus: Record<string, boolean>): boolean {
  return missingKeys(s, keyStatus).length > 0;
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

export type KeyState = ReturnType<typeof keyState>;

/** Status text of a key row: red when needed, grey when unused, green when stored. */
export const KEY_STATE_LABELS: Record<KeyState, string> = {
  saved: "✓ enregistrée",
  required: "✗ requise",
  unconfigured: "— non configurée",
};

/** « Transcription et Correction » for the « utilisée pour » tag. */
export function rolesText(roles: KeyRole[], lower = false): string {
  const names = roles.map((r) => (lower ? ROLE_LABELS[r].toLowerCase() : ROLE_LABELS[r]));
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} et ${names[names.length - 1]}` : (names[0] ?? "");
}

/** Key rows: providers in use first (transcription, then correction), the others after, catalog order. */
export function splitProviders<P extends { id: string }>(providers: P[], usage: Record<string, KeyRole[]>): { used: P[]; others: P[] } {
  const order = Object.keys(usage);
  const used = providers.filter((p) => usage[p.id]).sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id));
  return { used, others: providers.filter((p) => !usage[p.id]) };
}

/**
 * Parts of the setup banner (« Pour commencer, ajoutez la clé **OpenAI** (transcription) et
 * **Anthropic** (correction). »): one bold provider name each, then its roles.
 */
export function setupBannerParts(missing: { provider: string; roles: KeyRole[] }[], labelOf: (id: string) => string): { label: string; roles: string }[] {
  return missing.map((m) => ({ label: labelOf(m.provider), roles: rolesText(m.roles, true) }));
}

/** Under a provider select whose key is missing. */
export function missingKeyWarning(label: string, role: KeyRole): string {
  return `⚠ Aucune clé ${label}\u00a0: la ${ROLE_LABELS[role].toLowerCase()} échouera.`;
}
