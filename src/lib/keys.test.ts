import { describe, expect, it } from "vitest";
import { keyName } from "./keys";

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
