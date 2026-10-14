import { describe, expect, it } from "bun:test";
import { lang } from "./i18n";
import { testFailure, testLine } from "./providerTest";

describe("testLine", () => {
  it("reports a success with its duration", () => {
    expect(testLine("Transcription", "OpenAI", "gpt-transcribe", { Ok: 412.4 })).toEqual({
      ok: true, title: "Transcription · OpenAI / gpt-transcribe", text: "✓ fonctionne (412 ms)", raw: null,
    });
  });
  it("reports a failure translated, keeping the technical text when there is one", () => {
    expect(testLine("Correction", "Anthropic", "claude-opus-5-5", { Err: { code: "auth" } })).toEqual({
      ok: false, title: "Correction · Anthropic / claude-opus-5-5", text: "✗ clé refusée par Anthropic, vérifiez la clé", raw: null,
    });
    expect(testLine("Correction", "Anthropic", "x", { Err: { code: "http", status: 400, body: "bad model" } })).toMatchObject({
      raw: "HTTP 400: bad model",
    });
  });
  it("marks the correction unused in raw mode", () => {
    expect(testLine("Correction", "Anthropic", "x", null)).toEqual({ ok: true, title: "Correction", text: "— non utilisée (niveau brut)", raw: null });
  });
  it("follows the UI language", () => {
    lang.value = "en";
    expect(testLine("Transcription", "OpenAI", "m", { Ok: 10 }).text).toBe("✓ works (10 ms)");
  });
});

describe("testFailure", () => {
  it("translates a provider error and keeps its technical text", () => {
    expect(testFailure({ code: "provider", error: { code: "network", detail: "dns error" } })).toEqual({
      text: "✗ Test impossible : fournisseur injoignable", raw: "dns error",
    });
    expect(testFailure({ code: "provider", error: { code: "missing_key", provider: "OpenAI" } }).raw).toBeNull();
  });
  it("shows any other rejection as is, without details", () => {
    expect(testFailure("panique")).toEqual({ text: "✗ Test impossible : panique", raw: null });
    expect(testFailure(null).raw).toBeNull();
  });
});
