import { describe, expect, it } from "vitest";
import type { Provider } from "./api";
import { canSaveKey, deleteKeyQuestion, keyPlaceholder, keyState, keyUsage, missingKeys, unsavedDraftMessage, unsavedDrafts } from "./apiKeys";

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
