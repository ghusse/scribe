import { describe, expect, it } from "bun:test";
import type { Provider } from "./api";
import { lang } from "./i18n";
import { canSaveKey, keyPlaceholder, keyState, keyStateLabel, keyUsage, listParts, listText, missingKeys, missingKeyWarning, needsSetup, rolesText, setupBannerParts, splitProviders, unsavedDraftMessage, unsavedDrafts } from "./apiKeys";

const p = (id: string, label: string): Provider => ({ id, label, base_url: "", stt_models: [], llm_api: null, llm_models: [] });
const providers = [p("openai", "OpenAI"), p("anthropic", "Anthropic"), p("mistral", "Mistral")];
const s = { stt_provider: "openai", llm_provider: "anthropic", correction_enabled: true };

describe("keyUsage", () => {
  it("needs the transcription key always and the correction key when correction is on", () => {
    expect(keyUsage(s)).toEqual({ openai: ["transcription"], anthropic: ["correction"] });
    expect(keyUsage({ ...s, correction_enabled: false })).toEqual({ openai: ["transcription"] });
  });
  it("merges the roles of a provider used for both", () => {
    expect(keyUsage({ ...s, llm_provider: "openai" })).toEqual({ openai: ["transcription", "correction"] });
  });
});

describe("missingKeys / keyState", () => {
  it("lists required keys that are not stored", () => {
    expect(missingKeys(s, { openai: false, anthropic: false })).toEqual([
      { provider: "openai", roles: ["transcription"] },
      { provider: "anthropic", roles: ["correction"] },
    ]);
    expect(missingKeys(s, { openai: true, anthropic: false })).toEqual([{ provider: "anthropic", roles: ["correction"] }]);
    expect(missingKeys({ ...s, correction_enabled: false }, { openai: true })).toEqual([]);
  });
  it("classifies each provider", () => {
    const st = { openai: true, anthropic: false, mistral: false };
    expect(["openai", "anthropic", "mistral"].map((id) => keyState(id, s, st))).toEqual(["saved", "required", "unconfigured"]);
  });
});

describe("key row helpers", () => {
  it("never saves an empty key (that would delete it)", () => {
    expect(canSaveKey(undefined)).toBe(false);
    expect(canSaveKey("  ")).toBe(false);
    expect(canSaveKey("sk-1")).toBe(true);
  });
  it("words the placeholder", () => {
    expect(keyPlaceholder(false)).toBe("Coller la clé API…");
    expect(keyPlaceholder(true)).toBe("Remplacer la clé…");
    lang.value = "en";
    expect(keyPlaceholder(false)).toBe("Paste the API key…");
  });
});

describe("unsaved drafts", () => {
  it("names the providers with a typed key", () => {
    expect(unsavedDraftMessage(unsavedDrafts({ openai: "sk", anthropic: " " }, providers))).toBe("Clé OpenAI saisie mais non enregistrée. Enregistrer ?");
    expect(unsavedDraftMessage(unsavedDrafts({ openai: "a", anthropic: "b", mistral: "c" }, providers))).toBe(
      "Clés OpenAI, Anthropic et Mistral saisies mais non enregistrées. Enregistrer ?",
    );
    expect(unsavedDraftMessage(unsavedDrafts({}, providers))).toBeNull();
  });
  it("in English too", () => {
    lang.value = "en";
    expect(unsavedDraftMessage(unsavedDrafts({ openai: "a", anthropic: "b" }, providers))).toBe("OpenAI and Anthropic keys typed but not saved. Save them?");
  });
});

describe("key status display", () => {
  it("words each state", () => {
    expect((["saved", "required", "unconfigured"] as const).map(keyStateLabel)).toEqual(["✓ enregistrée", "✗ requise", "— non configurée"]);
  });
  it("joins roles", () => {
    expect(rolesText(["correction"])).toBe("Correction");
    expect(rolesText(["transcription", "correction"])).toBe("Transcription et Correction");
    expect(rolesText(["transcription", "correction"], true)).toBe("transcription et correction");
    lang.value = "en";
    expect(rolesText(["transcription", "correction"])).toBe("Transcription and Correction");
  });
});

describe("lists", () => {
  it("joins items the way the UI language does, each item kept apart", () => {
    expect(listText(["A", "B", "C"])).toBe("A, B et C");
    expect(listParts(["A", "B"])).toEqual([
      { type: "element", value: "A" }, { type: "literal", value: " et " }, { type: "element", value: "B" },
    ]);
    lang.value = "en";
    expect(listText(["A", "B", "C"])).toBe("A, B, and C");
  });
});

describe("splitProviders", () => {
  it("puts the providers in use first, in usage order", () => {
    const r = splitProviders(providers, keyUsage({ stt_provider: "mistral", llm_provider: "openai", correction_enabled: true }));
    expect(r.used.map((x) => x.id)).toEqual(["mistral", "openai"]);
    expect(r.others.map((x) => x.id)).toEqual(["anthropic"]);
  });
  it("counts a provider used for both once", () => {
    const r = splitProviders(providers, keyUsage({ stt_provider: "openai", llm_provider: "openai", correction_enabled: true }));
    expect(r.used.map((x) => x.id)).toEqual(["openai"]);
    expect(r.others.map((x) => x.id)).toEqual(["anthropic", "mistral"]);
  });
  it("drops the correction provider when correction is off", () => {
    expect(splitProviders(providers, keyUsage({ ...s, correction_enabled: false })).used.map((x) => x.id)).toEqual(["openai"]);
  });
});

describe("setup banner and warnings", () => {
  const label = (id: string) => providers.find((x) => x.id === id)!.label;
  it("names each missing key with its roles", () => {
    expect(setupBannerParts(missingKeys(s, {}), label)).toEqual([
      { label: "OpenAI", roles: "transcription" }, { label: "Anthropic", roles: "correction" },
    ]);
    expect(setupBannerParts(missingKeys({ ...s, llm_provider: "openai" }, {}), label)).toEqual([
      { label: "OpenAI", roles: "transcription et correction" },
    ]);
    expect(setupBannerParts(missingKeys(s, { openai: true, anthropic: true }), label)).toEqual([]);
  });
  it("words the inline warning per role", () => {
    expect(missingKeyWarning("Anthropic", "correction")).toBe("⚠ Aucune clé Anthropic : la correction échouera.");
    expect(missingKeyWarning("OpenAI", "transcription")).toBe("⚠ Aucune clé OpenAI : la transcription échouera.");
    lang.value = "en";
    expect(missingKeyWarning("OpenAI", "transcription")).toBe("⚠ No OpenAI key: transcription will fail.");
  });
});

// Same cases as needs_setup_follows_the_key_usage_rule in src-tauri/src/bootstrap.rs: the two rules must not diverge.
describe("needsSetup (parity with bootstrap::needs_setup)", () => {
  const settings = (stt: string, llm: string, correction_enabled: boolean) => ({ stt_provider: stt, llm_provider: llm, correction_enabled });
  const keys = (stored: string[]) => Object.fromEntries(stored.map((k) => [k, true]));
  const cases: [string, ReturnType<typeof settings>, string[], boolean][] = [
    ["no key at all", settings("openai", "anthropic", true), [], true],
    ["correction key missing", settings("openai", "anthropic", true), ["openai"], true],
    ["transcription key missing", settings("openai", "anthropic", true), ["anthropic"], true],
    ["both stored", settings("openai", "anthropic", true), ["openai", "anthropic"], false],
    ["correction off: no correction key needed", settings("openai", "anthropic", false), ["openai"], false],
    ["correction off: transcription key still needed", settings("openai", "anthropic", false), ["anthropic"], true],
    ["one provider for both roles", settings("openai", "openai", true), ["openai"], false],
  ];
  it.each(cases)("%s", (_name, st, stored, expected) => {
    expect(needsSetup(st, keys(stored))).toBe(expected);
  });

  it("a key marked false is not stored", () => {
    expect(needsSetup(settings("openai", "openai", true), { openai: false })).toBe(true);
  });
});
