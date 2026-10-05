import { describe, expect, it } from "vitest";
import { chordContains, chordName, isModifier, keyName, reservedWarning } from "./keys";

describe("keyName", () => {
  it("names no key, named keys, digits, letters and function keys", () => {
    expect(keyName(0)).toBe("Aucune");
    expect(keyName(0x20)).toBe("Espace");
    expect(keyName(0xa5)).toBe("Alt droit (AltGr)");
    expect(keyName(0x30)).toBe("0");
    expect(keyName(0x39)).toBe("9");
    expect(keyName(0x41)).toBe("A");
    expect(keyName(0x5a)).toBe("Z");
    expect(keyName(0x70)).toBe("F1");
    expect(keyName(0x87)).toBe("F24");
  });

  it("falls back to the hex code just outside each range", () => {
    expect(keyName(0x2f)).toBe("Touche 0x2F");
    expect(keyName(0x3a)).toBe("Touche 0x3A");
    expect(keyName(0x40)).toBe("Touche 0x40");
    expect(keyName(0x6f)).toBe("Touche 0x6F");
    expect(keyName(0x88)).toBe("Touche 0x88");
  });
});

describe("combinations", () => {
  it("knows the modifiers, generic and per side", () => {
    for (const vk of [0x10, 0x11, 0x12, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0x5b, 0x5c]) expect(isModifier(vk)).toBe(true);
    for (const vk of [0x41, 0x20, 0x5d, 0x70, 0]) expect(isModifier(vk)).toBe(false);
  });

  it("names a single key with its side and a combination without", () => {
    expect(chordName([])).toBe("Aucun");
    expect(chordName([0xa3])).toBe("Ctrl droit");
    expect(chordName([0xa2, 0xa0, 0x41])).toBe("Ctrl + Maj + A");
    expect(chordName([0xa4, 0x5b])).toBe("Alt + Win");
    expect(chordName([0x41, 0x70])).toBe("A + F1");
  });

  it("matches modifiers on either side only inside a combination", () => {
    expect(chordContains([0xa3], 0xa3)).toBe(true);
    expect(chordContains([0xa3], 0xa2)).toBe(false);
    expect(chordContains([0xa2, 0x41], 0xa3)).toBe(true);
    expect(chordContains([0xa2, 0x41], 0x11)).toBe(true);
    expect(chordContains([0xa2, 0x41], 0xa0)).toBe(false);
    expect(chordContains([0x41, 0x42], 0x43)).toBe(false);
    expect(chordContains([0xa2, 0x41], 0)).toBe(false);
  });

  it("flags the combinations Windows keeps", () => {
    expect(reservedWarning([0x5b, 0x4c])).toContain("Win + L");
    expect(reservedWarning([0xa2, 0x5c, 0x4c])).toContain("Win + L");
    expect(reservedWarning([0xa0, 0xa4])).toContain("Alt + Maj");
    expect(reservedWarning([0xa2, 0xa1])).toContain("Ctrl + Maj");
    expect(reservedWarning([0xa2, 0xa0, 0x41])).toBeNull();
    expect(reservedWarning([0xa4, 0xa0, 0x41])).toBeNull();
    expect(reservedWarning([0xa4, 0x5b])).toBeNull();
    expect(reservedWarning([0x4c])).toBeNull();
    expect(reservedWarning([0xa4, 0x41])).toBeNull();
    expect(reservedWarning([0xa3])).toBeNull();
  });
});
