import { describe, expect, it } from "vitest";
import { assignHotkey, captureOutcome, isTypingKey } from "./hotkeys";

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
      note: "Touches échangées : Déclenchement = Espace, Verrouillage = Ctrl droit.",
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
