import { describe, expect, it } from "vitest";
import { testLine, translateProviderError } from "./providerTest";

describe("translateProviderError", () => {
  it("translates refused keys (Auth or a raw 401/403)", () => {
    expect(translateProviderError("clé API refusée", "Anthropic")).toBe("clé refusée par Anthropic, vérifiez la clé");
    expect(translateProviderError('HTTP 401 Unauthorized: {"type":"error"}', "Anthropic")).toBe("clé refusée par Anthropic, vérifiez la clé");
    expect(translateProviderError("HTTP 403 : forbidden", null)).toBe("clé refusée, vérifiez la clé");
  });
  it("translates unknown models, rate limits and server errors", () => {
    expect(translateProviderError('HTTP 404 : {"error":"model_not_found"}', "OpenAI")).toBe("modèle inconnu chez ce fournisseur");
    expect(translateProviderError("HTTP 429 : slow down", "OpenAI")).toBe("trop de requêtes, réessayez dans un instant");
    expect(translateProviderError("HTTP 529 : overloaded", "Anthropic")).toBe("erreur du fournisseur (HTTP 529), réessayez plus tard");
  });
  it("translates network failures and timeouts", () => {
    expect(translateProviderError("erreur réseau : error sending request", "Groq")).toBe("fournisseur injoignable");
    expect(translateProviderError("délai dépassé", "Groq")).toBe("fournisseur injoignable");
  });
  it("translates a missing key reported before any call", () => {
    expect(translateProviderError("configuration : clé API manquante pour « OpenAI »", null)).toBe("aucune clé OpenAI enregistrée");
  });
  it("gives up on anything else (the raw text is shown in the details)", () => {
    expect(translateProviderError("réponse invalide : pas de <output>", "Anthropic")).toBeNull();
    expect(translateProviderError("HTTP 400 : bad request", "Anthropic")).toBeNull();
  });
});

describe("testLine", () => {
  it("reports a success with its duration", () => {
    expect(testLine("Transcription", "OpenAI", "gpt-transcribe", { Ok: 412.4 })).toEqual({
      ok: true, title: "Transcription · OpenAI / gpt-transcribe", text: "✓ fonctionne (412 ms)", raw: null,
    });
  });
  it("reports a failure translated, keeping the raw text", () => {
    expect(testLine("Correction", "Anthropic", "claude-opus-5-5", { Err: "clé API refusée" })).toEqual({
      ok: false, title: "Correction · Anthropic / claude-opus-5-5", text: "✗ clé refusée par Anthropic, vérifiez la clé", raw: "clé API refusée",
    });
    expect(testLine("Correction", "Anthropic", "x", { Err: "HTTP 400 : nope" }).text).toBe("✗ échec");
  });
  it("marks the correction unused in raw mode", () => {
    expect(testLine("Correction", "Anthropic", "x", null)).toEqual({ ok: true, title: "Correction", text: "— non utilisée (niveau brut)", raw: null });
  });
});
