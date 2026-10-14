import { describe, expect, it } from "bun:test";
import { lang } from "./i18n";
import { effortLabel, keysHelp, languageOptions } from "./settingsCopy";

describe("effortLabel", () => {
  it("translates the known levels and keeps unknown ones", () => {
    expect(["low", "medium", "high"].map(effortLabel)).toEqual(["Faible (rapide, recommandé)", "Moyenne", "Élevée (plus lent, plus cher)"]);
    expect(effortLabel("max")).toBe("max");
  });
  it("follows the UI language", () => {
    lang.value = "en";
    expect(effortLabel("low")).toBe("Low (fast, recommended)");
  });
});

describe("languageOptions", () => {
  it("offers the system language, then each language in its own name", () => {
    expect(languageOptions()).toEqual([
      { value: "system", label: "Système" }, { value: "en", label: "English" }, { value: "fr", label: "Français" },
    ]);
    lang.value = "en";
    expect(languageOptions().map((o) => o.label)).toEqual(["System", "English", "Français"]);
  });
});

describe("keysHelp", () => {
  it("names the OS secret store", () => {
    expect(keysHelp("mac")).toMatch(/^Stockées dans le trousseau macOS\./);
    expect(keysHelp("windows")).toMatch(/^Stockées dans le coffre Windows\./);
    lang.value = "en";
    expect(keysHelp("mac")).toMatch(/^Stored in the macOS Keychain\./);
  });
});
