import { describe, expect, it } from "vitest";
import { effortLabel, keysHelp, LEVEL_OPTIONS } from "./settingsCopy";

describe("effortLabel", () => {
  it("translates the known levels and keeps unknown ones", () => {
    expect(["low", "medium", "high"].map(effortLabel)).toEqual(["Faible (rapide, recommandé)", "Moyenne", "Élevée (plus lent, plus cher)"]);
    expect(effortLabel("max")).toBe("max");
  });
});

describe("LEVEL_OPTIONS", () => {
  it("covers every level once", () => {
    expect(LEVEL_OPTIONS.map((o) => o.value)).toEqual(["raw", "clean", "formatted"]);
  });
});

describe("keysHelp", () => {
  it("names the OS secret store", () => {
    expect(keysHelp("mac")).toMatch(/^Stockées dans le trousseau macOS\./);
    expect(keysHelp("windows")).toMatch(/^Stockées dans le coffre Windows\./);
  });
});
