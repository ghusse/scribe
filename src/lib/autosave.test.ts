import { describe, expect, it } from "vitest";
import { editAction, needsAttention, testSignature, type SaveState } from "./autosave";

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
