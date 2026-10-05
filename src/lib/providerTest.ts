/** Outcome of one provider call in test_providers: its duration in ms, or the raw error. */
export type TestOutcome = { Ok: number } | { Err: string };

/**
 * French summary of a provider error (the Display of ProviderError, or a build_providers error),
 * e.g. « clé refusée par Anthropic, vérifiez la clé ». Null when there is nothing better than the raw text.
 */
export function translateProviderError(raw: string, provider: string | null): string | null {
  const missing = /clé API manquante pour « (.+?) »/.exec(raw);
  if (missing) return `aucune clé ${missing[1]} enregistrée`;
  if (/clé API refusée/.test(raw) || /\bHTTP (401|403)\b/.test(raw)) return provider ? `clé refusée par ${provider}, vérifiez la clé` : "clé refusée, vérifiez la clé";
  if (/\bHTTP 404\b/.test(raw)) return "modèle inconnu chez ce fournisseur";
  if (/\bHTTP 429\b/.test(raw)) return "trop de requêtes, réessayez dans un instant";
  const server = /\bHTTP (5\d\d)\b/.exec(raw);
  if (server) return `erreur du fournisseur (HTTP ${server[1]}), réessayez plus tard`;
  if (/^erreur réseau|délai dépassé/.test(raw)) return "fournisseur injoignable";
  if (/fournisseur de (transcription|correction) inconnu/.test(raw)) return "fournisseur inconnu";
  return null;
}

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
  const title = `${role} · ${provider} / ${model}`;
  if (outcome === null) return { ok: true, title: `${role}`, text: "— non utilisée (niveau brut)", raw: null };
  if ("Ok" in outcome) return { ok: true, title, text: `✓ fonctionne (${Math.round(outcome.Ok)} ms)`, raw: null };
  const fr = translateProviderError(outcome.Err, provider);
  return { ok: false, title, text: `✗ ${fr ?? "échec"}`, raw: outcome.Err };
}
