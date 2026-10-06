import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, resetTauri } from "../../tests/tauri";
import { api, effortLevels, type Provider, type Settings } from "./api";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));

beforeEach(() => {
  resetTauri();
  invoke.mockImplementation(async (cmd: string) => `ok:${cmd}`);
});

describe("api", () => {
  // Command names and argument names must match the #[tauri::command] signatures in src-tauri.
  const settings = { level: "raw" } as Settings;
  const table: [string, () => Promise<unknown>, string, Record<string, unknown> | undefined][] = [
    ["listDictations defaults", () => api.listDictations(null), "list_dictations", { query: null, limit: 50, offset: 0 }],
    ["listDictations page", () => api.listDictations("abc", 10, 20), "list_dictations", { query: "abc", limit: 10, offset: 20 }],
    ["saveEditedText", () => api.saveEditedText(3, null), "save_edited_text", { id: 3, text: null }],
    ["deleteDictation", () => api.deleteDictation(3), "delete_dictation", { id: 3 }],
    ["copyDictation", () => api.copyDictation(3), "copy_dictation", { id: 3 }],
    ["retranscribe", () => api.retranscribe(3), "retranscribe", { id: 3 }],
    ["listTerms", () => api.listTerms(), "list_terms", undefined],
    ["addTerm", () => api.addTerm("K8s", ["cube"], null), "add_term", { term: "K8s", variants: ["cube"], note: null }],
    ["updateTerm", () => api.updateTerm(2, "K8s", [], "n"), "update_term", { id: 2, term: "K8s", variants: [], note: "n" }],
    ["deleteTerm", () => api.deleteTerm(2), "delete_term", { id: 2 }],
    ["getSettings", () => api.getSettings(), "get_settings", undefined],
    ["saveSettings", () => api.saveSettings(settings), "save_settings", { settings }],
    ["keyStatus", () => api.keyStatus(), "key_status", undefined],
    ["setApiKey", () => api.setApiKey("openai", "sk"), "set_api_key", { provider: "openai", key: "sk" }],
    ["testProviders", () => api.testProviders(), "test_providers", undefined],
    ["captureKey", () => api.captureKey(), "capture_key", undefined],
    ["cancelCapture", () => api.cancelCapture(), "cancel_capture", undefined],
    ["providers", () => api.providers(), "providers", undefined],
    ["getAutostart", () => api.getAutostart(), "get_autostart", undefined],
    ["checkUpdate", () => api.checkUpdate(), "check_update", undefined],
    ["installUpdate", () => api.installUpdate(), "install_update", undefined],
    ["setAutostart", () => api.setAutostart(true), "set_autostart", { enabled: true }],
    ["overlayDismiss", () => api.overlayDismiss(), "overlay_dismiss", undefined],
    ["openHistory", () => api.openHistory(null), "open_history", { id: null }],
  ];

  it.each(table)("%s invokes its command with its arguments", async (_name, call, cmd, args) => {
    await expect(call()).resolves.toBe(`ok:${cmd}`);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0]).toEqual(args === undefined ? [cmd] : [cmd, args]);
  });

  it("covers every api function", () => {
    expect(new Set(table.map(([n]) => n.split(" ")[0]))).toEqual(new Set(Object.keys(api)));
  });
});

describe("effortLevels", () => {
  const anthropic: Provider = {
    id: "anthropic", label: "Anthropic", base_url: "", stt_models: [], llm_api: "anthropic",
    llm_models: [{ id: "claude-x", efforts: ["low", "high"] }, { id: "claude-haiku-9", efforts: [] }],
  };
  const openai: Provider = { ...anthropic, id: "openai", llm_api: "open_ai_chat", llm_models: [] };

  it("uses the catalog's levels for a known model", () => {
    expect(effortLevels(anthropic, "claude-x")).toEqual(["low", "high"]);
    expect(effortLevels(anthropic, "claude-haiku-9")).toEqual([]);
  });

  it("gives hand-typed Anthropic models effort, except Haiku and 3.x", () => {
    expect(effortLevels(anthropic, "claude-new")).toEqual(["low", "medium", "high"]);
    expect(effortLevels(anthropic, "claude-haiku-5")).toEqual([]);
    expect(effortLevels(anthropic, "claude-3-opus")).toEqual([]);
  });

  it("none for other APIs or no provider", () => {
    expect(effortLevels(openai, "gpt-x")).toEqual([]);
    expect(effortLevels(undefined, "claude-new")).toEqual([]);
  });
});
