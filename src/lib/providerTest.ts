import type { ProviderError } from "./api";
import { errorText, providerErrorDetail, providerErrorText } from "./errors";
import { t } from "./i18n";

/** Outcome of one provider call in test_providers: its duration in ms, or the error. */
export type TestOutcome = { Ok: number } | { Err: ProviderError };

export interface TestLine {
  ok: boolean;
  /** « Transcription · OpenAI / gpt-transcribe » */
  title: string;
  /** « ✓ fonctionne (412 ms) », « ✗ modèle inconnu chez ce fournisseur »… */
  text: string;
  /** The untranslated error, for a « Détails » disclosure. */
  raw: string | null;
}

/** One line of the test result; `outcome` null means not used (raw level). */
export function testLine(role: string, provider: string, model: string, outcome: TestOutcome | null): TestLine {
  const m = t.value.settings.test;
  const title = `${role} · ${provider} / ${model}`;
  if (outcome === null) return { ok: true, title: role, text: m.unused, raw: null };
  if ("Ok" in outcome) return { ok: true, title, text: m.works(Math.round(outcome.Ok)), raw: null };
  return { ok: false, title, text: `✗ ${providerErrorText(outcome.Err, provider)}`, raw: providerErrorDetail(outcome.Err) };
}

/** A test that could not run (rejected `test_providers`), with the provider's technical text when there is one. */
export function testFailure(e: unknown): { text: string; raw: string | null } {
  const provider = (e as { code?: unknown } | null)?.code === "provider" ? (e as { error: ProviderError }).error : null;
  return { text: t.value.settings.test.failed(errorText(e)), raw: provider && providerErrorDetail(provider) };
}
