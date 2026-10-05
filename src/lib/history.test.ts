import { describe, expect, it } from "vitest";
import { canCopy, canRetranscribe, currentText, displayText, editDraft, editPayload, latestOnly, OUTCOME_BADGES, STALE, timingLine } from "./history";

const texts = (raw: string | null, final: string | null, edited: string | null) => ({ raw_text: raw, final_text: final, edited_text: edited });

describe("history texts", () => {
  it("prefers the correction, then the corrected text, then the raw transcript", () => {
    expect(currentText(texts("r", "f", "e"))).toBe("e");
    expect(currentText(texts("r", "f", null))).toBe("f");
    expect(currentText(texts("r", null, null))).toBe("r");
    expect(currentText(texts(null, null, null))).toBeNull();
  });

  it("shows a dash and edits from empty when nothing was transcribed", () => {
    expect(displayText(texts(null, null, null))).toBe("—");
    expect(displayText(texts("r", null, null))).toBe("r");
    expect(editDraft(texts(null, null, null))).toBe("");
    expect(editDraft(texts("r", "f", null))).toBe("f");
  });

  it("copies only with a transcript", () => {
    expect(canCopy({ raw_text: null, final_text: null })).toBe(false);
    expect(canCopy({ raw_text: "", final_text: null })).toBe(false);
    expect(canCopy({ raw_text: "r", final_text: null })).toBe(true);
    expect(canCopy({ raw_text: null, final_text: "f" })).toBe(true);
  });

  it("retranscribes only with audio and when that card is not busy", () => {
    expect(canRetranscribe({ id: 1, audio_path: null }, null)).toBe(false);
    expect(canRetranscribe({ id: 1, audio_path: "a.wav" }, null)).toBe(true);
    expect(canRetranscribe({ id: 1, audio_path: "a.wav" }, 1)).toBe(false);
    expect(canRetranscribe({ id: 1, audio_path: "a.wav" }, 2)).toBe(true);
  });

  it("a blank correction clears it, anything else is sent as typed", () => {
    expect(editPayload("")).toBeNull();
    expect(editPayload("  \n ")).toBeNull();
    expect(editPayload(" texte ")).toBe(" texte ");
  });

  it("words the badges and timings", () => {
    expect(OUTCOME_BADGES.pasted_uncertain).toBe("Inséré ?");
    expect(timingLine({ duration_ms: 4200, stt_ms: null, llm_ms: null })).toBe("4.2 s");
    expect(timingLine({ duration_ms: 4200, stt_ms: 812, llm_ms: 640 })).toBe("4.2 s · STT 812 ms · LLM 640 ms");
    expect(timingLine({ duration_ms: 50, stt_ms: null, llm_ms: 3 })).toBe("0.1 s · LLM 3 ms");
  });
});

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("latestOnly", () => {
  it("passes a lone request through", async () => {
    const latest = latestOnly();
    await expect(latest(Promise.resolve(3))).resolves.toBe(3);
    await expect(latest(Promise.reject("boom"))).rejects.toBe("boom");
  });

  it("drops an older answer that arrives after a newer one", async () => {
    const latest = latestOnly();
    const a = deferred<string>();
    const b = deferred<string>();
    const first = latest(a.promise);
    const second = latest(b.promise);
    b.resolve("new");
    a.resolve("old");
    await expect(second).resolves.toBe("new");
    await expect(first).resolves.toBe(STALE);
  });

  it("drops an older answer even when it arrives first", async () => {
    const latest = latestOnly();
    const a = deferred<string>();
    const first = latest(a.promise);
    const second = latest(Promise.resolve("new"));
    a.resolve("old");
    await expect(first).resolves.toBe(STALE);
    await expect(second).resolves.toBe("new");
  });

  it("swallows a stale failure, reports the latest one", async () => {
    const latest = latestOnly();
    const a = deferred<string>();
    const first = latest(a.promise);
    const second = latest(Promise.reject("latest failed"));
    a.reject("old failed");
    await expect(first).resolves.toBe(STALE);
    await expect(second).rejects.toBe("latest failed");
  });
});
