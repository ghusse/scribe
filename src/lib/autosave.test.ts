import { describe, expect, it } from "vitest";
import { editAction, needsAttention, staleGuard, statusView, testSignature, type SaveState } from "./autosave";

describe("editAction", () => {
  it("schedules a save for a real change, even while another save is in flight", () => {
    expect(editAction('{"a":2}', '{"a":1}', false)).toBe("schedule");
    expect(editAction('{"a":2}', '{"a":1}', true)).toBe("schedule");
  });
  it("reverts to « enregistré » when the value comes back to the saved one", () => {
    expect(editAction('{"a":1}', '{"a":1}', false)).toBe("revert");
  });
  it("lets the in-flight save settle the state", () => {
    expect(editAction('{"a":1}', '{"a":1}', true)).toBe("ignore");
  });
});

describe("needsAttention", () => {
  it("flags only unsaved outcomes", () => {
    const all: SaveState[] = ["saved", "pending", "saving", "invalid", "failed"];
    expect(all.filter(needsAttention)).toEqual(["invalid", "failed"]);
  });
});

describe("testSignature", () => {
  const base = { stt_provider: "openai", stt_model: "m", llm_provider: "anthropic", llm_model: "c", llm_effort: "low", level: "clean" as const };
  it("changes with anything the provider test depends on", () => {
    for (const k of Object.keys(base) as (keyof typeof base)[]) {
      expect(testSignature({ ...base, [k]: k === "level" ? "raw" : "x" })).not.toBe(testSignature(base));
    }
  });
  it("ignores unrelated settings", () => {
    expect(testSignature({ ...base, ...{ restore_delay_ms: 5 } })).toBe(testSignature(base));
  });
});

describe("staleGuard", () => {
  it("drops a provider test result when a key was saved while the test ran", () => {
    const g = staleGuard();
    const t = g.token(); // test starts
    g.invalidate(); // writeKey during the test
    expect(g.isCurrent(t)).toBe(false);
  });
  it("keeps a result when nothing changed, and a new token is current again", () => {
    const g = staleGuard();
    expect(g.isCurrent(g.token())).toBe(true);
    g.invalidate();
    expect(g.isCurrent(g.token())).toBe(true);
  });
});

describe("statusView", () => {
  it("shows progress while saving", () => {
    expect(statusView("pending", null, false)).toMatchObject({ tone: "muted", text: "Enregistrement…", visible: true, alert: false });
    expect(statusView("saving", null, true).text).toBe("Enregistrement…");
  });
  it("shows the success only while it flashes", () => {
    expect(statusView("saved", null, true)).toMatchObject({ tone: "success", text: "✓ Réglages enregistrés", visible: true });
    // Same text while hidden: the opacity transition fades the message out instead of blanking it first.
    expect(statusView("saved", null, false)).toMatchObject({ visible: false, text: "✓ Réglages enregistrés" });
  });
  it("keeps an invalid value visible, without retry", () => {
    expect(statusView("invalid", "Seuil de maintien : entre 100 et 2000 ms", false)).toEqual({
      tone: "danger", text: "⚠ Non enregistré : Seuil de maintien : entre 100 et 2000 ms", alert: true, retry: false, details: null, visible: true,
    });
  });
  it("offers retry and details for a failed save", () => {
    expect(statusView("failed", "disque plein\nos error 112", false)).toEqual({
      tone: "danger", text: "⚠ Non enregistré : disque plein", alert: true, retry: true, details: "disque plein\nos error 112", visible: true,
    });
  });
});
