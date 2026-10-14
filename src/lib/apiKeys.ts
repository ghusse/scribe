import type { Provider, Settings } from "./api";
import { lang, t } from "./i18n";

export type KeyRole = "transcription" | "correction";

export function roleLabel(role: KeyRole): string {
  return t.value.settings.roles[role];
}

/** « A, B et C » in the UI language; `formatToParts` keeps each item apart, so it can be a link. */
export function listParts(items: string[]): { type: "element" | "literal"; value: string }[] {
  return new Intl.ListFormat(lang.value, { type: "conjunction" }).formatToParts(items);
}

export function listText(items: string[]): string {
  return listParts(items).map((p) => p.value).join("");
}

type UsageSettings = Pick<Settings, "stt_provider" | "llm_provider" | "correction_enabled">;

/**
 * Providers whose key the current settings need, with what for. Same rule as needs_setup in
 * src-tauri/src/bootstrap.rs: transcription always, correction when it is enabled.
 */
export function keyUsage(s: UsageSettings): Record<string, KeyRole[]> {
  const usage: Record<string, KeyRole[]> = { [s.stt_provider]: ["transcription"] };
  if (s.correction_enabled) (usage[s.llm_provider] ??= []).push("correction");
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
  const m = t.value.settings.apiKeys;
  return stored ? m.replace : m.paste;
}

/** Providers with a typed but unsaved key (a test would run without it), in catalog order. */
export function unsavedDrafts(drafts: Record<string, string | undefined>, providers: Provider[]): Provider[] {
  return providers.filter((p) => canSaveKey(drafts[p.id]));
}

export function unsavedDraftMessage(unsaved: Provider[]): string | null {
  const m = t.value.settings.test;
  if (unsaved.length === 0) return null;
  if (unsaved.length === 1) return m.unsavedDraft(unsaved[0].label);
  return m.unsavedDrafts(listText(unsaved.map((p) => p.label)));
}

export type KeyState = ReturnType<typeof keyState>;

/** Status text of a key row: red when needed, grey when unused, green when stored. */
export function keyStateLabel(state: KeyState): string {
  return t.value.settings.apiKeys.states[state];
}

/** « Transcription et Correction » for the « utilisée pour » tag. */
export function rolesText(roles: KeyRole[], lower = false): string {
  return listText(roles.map((r) => (lower ? roleLabel(r).toLowerCase() : roleLabel(r))));
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
  return t.value.settings.models.missingKey(label, roleLabel(role).toLowerCase());
}
