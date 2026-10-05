import { describe, expect, it } from "vitest";
import type { Provider } from "./api";
import { canSaveKey, deleteKeyQuestion, KEY_STATE_LABELS, keyPlaceholder, keyState, keyUsage, missingKeys, missingKeyWarning, needsSetup, rolesText, setupBannerParts, splitProviders, unsavedDraftMessage, unsavedDrafts } from "./apiKeys";

const p = (id: string, label: string): Provider => ({ id, label, base_url: "", stt_models: [], llm_api: null, llm_models: [] });
const providers = [p("openai", "OpenAI"), p("anthropic", "Anthropic"), p("mistral", "Mistral")];
const s = { stt_provider: "openai", llm_provider: "anthropic", level: "formatted" as const };

describe("keyUsage", () => {
  it("needs the transcription key always and the correction key unless raw", () => {
    expect(keyUsage(s)).toEqual({ openai: ["transcription"], anthropic: ["correction"] });
    expect(keyUsage({ ...s, level: "raw" })).toEqual({ openai: ["transcription"] });
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
    expect(missingKeys({ ...s, level: "raw" }, { openai: true })).toEqual([]);
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
  it("words the placeholder and the confirmation", () => {
    expect(keyPlaceholder(false)).toBe("Coller la clé API…");
    expect(keyPlaceholder(true)).toBe("Remplacer la clé…");
    expect(deleteKeyQuestion("Anthropic")).toBe("Supprimer la clé Anthropic ?");
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
});

describe("key status display", () => {
  it("words each state", () => {
    expect(KEY_STATE_LABELS).toEqual({ saved: "✓ enregistrée", required: "✗ requise", unconfigured: "— non configurée" });
  });
  it("joins roles", () => {
    expect(rolesText(["correction"])).toBe("Correction");
    expect(rolesText(["transcription", "correction"])).toBe("Transcription et Correction");
    expect(rolesText(["transcription", "correction"], true)).toBe("transcription et correction");
  });
});

describe("splitProviders", () => {
  it("puts the providers in use first, in usage order", () => {
    const r = splitProviders(providers, keyUsage({ stt_provider: "mistral", llm_provider: "openai", level: "clean" }));
    expect(r.used.map((x) => x.id)).toEqual(["mistral", "openai"]);
    expect(r.others.map((x) => x.id)).toEqual(["anthropic"]);
  });
  it("counts a provider used for both once", () => {
    const r = splitProviders(providers, keyUsage({ stt_provider: "openai", llm_provider: "openai", level: "clean" }));
    expect(r.used.map((x) => x.id)).toEqual(["openai"]);
    expect(r.others.map((x) => x.id)).toEqual(["anthropic", "mistral"]);
  });
  it("drops the correction provider in raw mode", () => {
    expect(splitProviders(providers, keyUsage({ ...s, level: "raw" })).used.map((x) => x.id)).toEqual(["openai"]);
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
  });
});

// Same cases as needs_setup_follows_the_key_usage_rule in src-tauri/src/bootstrap.rs: the two rules must not diverge.
describe("needsSetup (parity with bootstrap::needs_setup)", () => {
  const settings = (stt: string, llm: string, level: "raw" | "clean" | "formatted") => ({ stt_provider: stt, llm_provider: llm, level });
  const keys = (stored: string[]) => Object.fromEntries(stored.map((k) => [k, true]));
  const cases: [string, ReturnType<typeof settings>, string[], boolean][] = [
    ["no key at all", settings("openai", "anthropic", "formatted"), [], true],
    ["correction key missing", settings("openai", "anthropic", "formatted"), ["openai"], true],
    ["transcription key missing", settings("openai", "anthropic", "formatted"), ["anthropic"], true],
    ["both stored", settings("openai", "anthropic", "formatted"), ["openai", "anthropic"], false],
    ["raw: no correction key needed", settings("openai", "anthropic", "raw"), ["openai"], false],
    ["raw: transcription key still needed", settings("openai", "anthropic", "raw"), ["anthropic"], true],
    ["one provider for both roles", settings("openai", "openai", "formatted"), ["openai"], false],
  ];
  it.each(cases)("%s", (_name, st, stored, expected) => {
    expect(needsSetup(st, keys(stored))).toBe(expected);
  });

  it("a key marked false is not stored", () => {
    expect(needsSetup(settings("openai", "openai", "clean"), { openai: false })).toBe(true);
  });
});
