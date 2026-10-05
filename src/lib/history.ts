import type { Dictation, Outcome } from "./api";

export const OUTCOME_BADGES: Record<Outcome, string> = { pasted: "Inséré", pasted_uncertain: "Inséré ?", clipboard: "Copié", error: "Erreur" };

type Texts = Pick<Dictation, "edited_text" | "final_text" | "raw_text">;

/** The best text of a dictation: the user's correction, else the corrected text, else the raw transcript. */
export function currentText(d: Texts): string | null {
  return d.edited_text ?? d.final_text ?? d.raw_text;
}

/** What a history card shows: « — » when nothing was transcribed (an error). */
export function displayText(d: Texts): string {
  return currentText(d) ?? "—";
}

/** The editor starts from the text shown; empty when there is none. */
export function editDraft(d: Texts): string {
  return currentText(d) ?? "";
}

/** Copier / Corriger need a transcript (the backend copies final, else raw). */
export function canCopy(d: Pick<Dictation, "raw_text" | "final_text">): boolean {
  return !!d.raw_text || !!d.final_text;
}

/** Retranscrire needs the kept audio, and no action already running on that card. */
export function canRetranscribe(d: Pick<Dictation, "id" | "audio_path">, busy: number | null): boolean {
  return !!d.audio_path && busy !== d.id;
}

/** What « Enregistrer la correction » sends: a blank draft clears the correction (null). */
export function editPayload(draft: string): string | null {
  return draft.trim() === "" ? null : draft;
}

/** « 4.2 s · STT 812 ms · LLM 640 ms » */
export function timingLine(d: Pick<Dictation, "duration_ms" | "stt_ms" | "llm_ms">): string {
  let s = `${(d.duration_ms / 1000).toFixed(1)} s`;
  if (d.stt_ms !== null) s += ` · STT ${d.stt_ms} ms`;
  if (d.llm_ms !== null) s += ` · LLM ${d.llm_ms} ms`;
  return s;
}

/** Thrown away result of a request overtaken by a newer one. */
export const STALE = Symbol("stale");

/**
 * Keeps only the latest request's outcome: a search typed quickly can get its answers out of
 * order, and an older page must never overwrite a newer one. A stale request's result is STALE,
 * and its failure is dropped (a newer request decides what is shown).
 */
export function latestOnly() {
  let latest = 0;
  return async function <T>(request: Promise<T>): Promise<T | typeof STALE> {
    const mine = ++latest;
    try {
      const value = await request;
      return mine === latest ? value : STALE;
    } catch (e) {
      if (mine !== latest) return STALE;
      throw e;
    }
  };
}
