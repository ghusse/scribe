import { describe, expect, it } from "vitest";
import { joinVariants, noteOrNull, splitVariants } from "./glossary";

describe("glossary fields", () => {
  it("splits variants on commas, trims them and drops blanks", () => {
    expect(splitVariants("")).toEqual([]);
    expect(splitVariants(" ,  , ")).toEqual([]);
    expect(splitVariants("cube, kubernetes ,  ku ber netes,")).toEqual(["cube", "kubernetes", "ku ber netes"]);
  });

  it("joins variants for editing, and splitting them back is lossless", () => {
    const v = ["cube", "ku ber netes"];
    expect(joinVariants(v)).toBe("cube, ku ber netes");
    expect(splitVariants(joinVariants(v))).toEqual(v);
    expect(joinVariants([])).toBe("");
  });

  it("an empty or blank note is no note", () => {
    expect(noteOrNull("")).toBeNull();
    expect(noteOrNull("   ")).toBeNull();
    expect(noteOrNull("infra")).toBe("infra");
  });
});
