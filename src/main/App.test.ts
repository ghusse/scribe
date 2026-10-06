import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { calls, commands, emit, resetTauri } from "../../tests/tauri";
import { PROVIDERS, settings } from "../../tests/fixtures";
import App from "./App.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));

beforeEach(() => {
  resetTauri();
  Element.prototype.scrollIntoView = vi.fn();
  commands({
    get_settings: () => settings(),
    providers: () => PROVIDERS,
    key_status: () => ({ openai: true, anthropic: true }),
    list_dictations: () => [],
    list_terms: () => [],
    save_settings: () => undefined,
    get_autostart: () => false,
  });
});

const tab = (name: string) => screen.getByRole("button", { name: new RegExp(`^${name}`) });
const settingsPanel = () => screen.getByText("Raccourci").closest("div[hidden], div:not([hidden])") as HTMLElement;

describe("App", () => {
  it("opens on the history when the keys are set, Settings mounted but hidden", async () => {
    render(App);
    expect(await screen.findByText(/Aucune dictée/)).toBeTruthy();
    expect(tab("Historique").classList.contains("active")).toBe(true);
    await screen.findByText("Raccourci");
    expect(settingsPanel().hidden).toBe(true);
  });

  it("first launch without the keys: opens on the settings", async () => {
    commands({ key_status: () => ({ openai: true }) });
    render(App);
    await waitFor(() => expect(tab("Réglages").classList.contains("active")).toBe(true));
    expect(settingsPanel().hidden).toBe(false);
    expect(screen.queryByText(/Aucune dictée/)).toBeNull();
  });

  it("stays on the history when the settings cannot be read", async () => {
    commands({ get_settings: () => Promise.reject("db") });
    render(App);
    await waitFor(() => expect(calls("key_status").length).toBeGreaterThan(0));
    await new Promise((r) => setTimeout(r, 0));
    expect(tab("Historique").classList.contains("active")).toBe(true);
  });

  it("switches tabs; the settings keep their state while hidden", async () => {
    render(App);
    await fireEvent.click(tab("Vocabulaire"));
    expect(await screen.findByText(/Glossaire vide/)).toBeTruthy();
    expect(screen.queryByText(/Aucune dictée/)).toBeNull();
    await fireEvent.click(tab("Réglages"));
    expect(settingsPanel().hidden).toBe(false);
    expect(screen.queryByText(/Glossaire vide/)).toBeNull();
    await fireEvent.click(tab("Historique"));
    expect(await screen.findByText(/Aucune dictée/)).toBeTruthy();
    expect(calls("get_settings")).toHaveLength(2); // App + the one Settings mount
  });

  it("a « Voir » from a toast shows that dictation in the history, even on first launch", async () => {
    const d = {
      id: 4, created_at: "2026-10-05T10:00:00Z", mode: "hold", app_name: null, audio_path: null, duration_ms: 1000, raw_text: "vu",
      final_text: null, edited_text: null, level: "raw", transcriber: null, corrector: null, stt_ms: null, llm_ms: null, outcome: "clipboard", error: null,
    };
    commands({ key_status: () => ({}), list_dictations: () => [d] });
    render(App);
    await fireEvent.click(tab("Vocabulaire"));
    await waitFor(() => expect(emit("focus-dictation", 4)).toBe(1));
    await waitFor(() => expect(tab("Historique").classList.contains("active")).toBe(true));
    await waitFor(() => expect(document.getElementById("d-4")?.classList.contains("highlighted")).toBe(true));
    expect(tab("Historique").classList.contains("active")).toBe(true); // not overridden by the first-launch rule
  });

  it("flags unsaved settings on the tab", async () => {
    commands({ save_settings: () => Promise.reject("disque plein") });
    render(App);
    await fireEvent.click(tab("Réglages"));
    await screen.findByText("Raccourci");
    expect(screen.queryByLabelText("(réglages non enregistrés)")).toBeNull();
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    expect(await screen.findByLabelText("(réglages non enregistrés)", {}, { timeout: 2000 })).toBeTruthy();
  });

  it("stops listening when unmounted", async () => {
    const { unmount } = render(App);
    await waitFor(() => expect(emit("focus-dictation", 1)).toBe(1));
    unmount();
    await waitFor(() => expect(emit("focus-dictation", 1)).toBe(0));
  });
});
