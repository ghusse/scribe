import { describe, expect, it } from "vitest";
import { assignHotkey, CAPTURE_GRACE_MS, captureOutcome, isTypingKey, normalizeLock, pendingSummary, swallowsKey } from "./hotkeys";

const CTRL_R = 0xa3, SPACE = 0x20, F8 = 0x77, A = 0x41;
const cur = { trigger_vk: CTRL_R, lock_vk: SPACE };

describe("assignHotkey", () => {
  it("assigns a free key to the role", () => {
    expect(assignHotkey("trigger", F8, cur)).toEqual({ ok: true, keys: { trigger_vk: F8, lock_vk: SPACE }, note: null, confirm: false });
    expect(assignHotkey("lock", F8, cur)).toEqual({ ok: true, keys: { trigger_vk: CTRL_R, lock_vk: F8 }, note: null, confirm: false });
  });
  it("swaps when the trigger takes the lock key", () => {
    expect(assignHotkey("trigger", SPACE, cur)).toEqual({
      ok: true,
      keys: { trigger_vk: SPACE, lock_vk: CTRL_R },
      note: "Touches échangées : Déclenchement = Espace, Verrouillage = Ctrl droit.",
      confirm: true,
    });
  });
  it("swaps when the lock takes the trigger key, and asks before Space becomes the trigger", () => {
    const r = assignHotkey("lock", CTRL_R, cur);
    expect(r).toMatchObject({ ok: true, keys: { trigger_vk: SPACE, lock_vk: CTRL_R }, confirm: true });
  });
  it("never leaves the trigger empty", () => {
    expect(assignHotkey("lock", CTRL_R, { trigger_vk: CTRL_R, lock_vk: 0 })).toEqual({
      ok: false, note: "Ctrl droit est déjà la touche de déclenchement.",
    });
  });
  it("asks before making a typing key the trigger, not the lock", () => {
    expect(assignHotkey("trigger", A, cur)).toMatchObject({ ok: true, confirm: true });
    expect(assignHotkey("lock", A, cur)).toMatchObject({ ok: true, confirm: false });
  });
  it("does not ask again for the current trigger", () => {
    expect(assignHotkey("trigger", SPACE, { trigger_vk: SPACE, lock_vk: 0 })).toMatchObject({ ok: true, confirm: false, note: null });
  });
});

describe("captureOutcome", () => {
  it("treats Échap and the cancel button as a cancel", () => {
    expect(captureOutcome(0x1b, false)).toBe("cancelled");
    expect(captureOutcome(null, true)).toBe("cancelled");
    expect(captureOutcome(F8, true)).toBe("cancelled");
  });
  it("distinguishes the time-out from a key", () => {
    expect(captureOutcome(null, false)).toBe("timeout");
    expect(captureOutcome(F8, false)).toBe("key");
  });
});

describe("isTypingKey", () => {
  it("covers letters, digits, Space, Enter and Tab only", () => {
    for (const vk of [0x41, 0x5a, 0x30, 0x39, 0x60, 0x20, 0x0d, 0x09]) expect(isTypingKey(vk)).toBe(true);
    for (const vk of [CTRL_R, F8, 0xa5, 0x1b, 0x14]) expect(isTypingKey(vk)).toBe(false);
  });
});

describe("swallowsKey", () => {
  it("blocks Space/Enter on the capturing role's button so they cannot cancel or restart the capture", () => {
    expect(swallowsKey(" ", "lock", "lock", null, 0)).toBe(true);
    expect(swallowsKey("Enter", "lock", "lock", null, 0)).toBe(true);
    expect(swallowsKey("Enter", "trigger", "lock", null, 0)).toBe(false);
  });
  it("keeps blocking just after the capture ended (the key may arrive after the hook's reply)", () => {
    const ended = { role: "lock" as const, at: 1000 };
    expect(swallowsKey(" ", "lock", null, ended, 1000 + CAPTURE_GRACE_MS - 1)).toBe(true);
    expect(swallowsKey(" ", "lock", null, ended, 1000 + CAPTURE_GRACE_MS)).toBe(false);
    expect(swallowsKey(" ", "trigger", null, ended, 1001)).toBe(false);
  });
  it("never blocks Tab, and nothing when idle", () => {
    expect(swallowsKey("Tab", "lock", "lock", null, 0)).toBe(false);
    expect(swallowsKey("Enter", "lock", null, null, 0)).toBe(false);
  });
});

describe("pendingSummary", () => {
  it("shows the lock as well when the assignment swaps the keys", () => {
    const a = assignHotkey("trigger", SPACE, cur);
    if (!a.ok) throw new Error("expected ok");
    expect(pendingSummary(a.keys, cur)).toEqual({ trigger: "Espace", lock: "Ctrl droit" });
  });
  it("shows only the trigger when the lock is unchanged", () => {
    const a = assignHotkey("trigger", A, cur);
    if (!a.ok) throw new Error("expected ok");
    expect(pendingSummary(a.keys, cur).lock).toBeNull();
  });
});

describe("normalizeLock", () => {
  it("maps a disabled lock key to « Aucune »", () => {
    const s = { lock_vk: 0x20, gesture: { lock_key_enabled: false, hold_threshold_ms: 300 } };
    expect(normalizeLock(s)).toEqual({ lock_vk: 0, gesture: { lock_key_enabled: true, hold_threshold_ms: 300 } });
    expect(s.lock_vk).toBe(0x20); // not mutated
  });
  it("keeps an enabled lock key as is", () => {
    const s = { lock_vk: 0x20, gesture: { lock_key_enabled: true } };
    expect(normalizeLock(s)).toBe(s);
  });
});
