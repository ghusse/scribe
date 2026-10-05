import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import type { Dictation } from "../lib/api";
import { calls, commands, emit, resetTauri } from "../../tests/tauri";
import History from "./History.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));

const dictation = (id: number, over: Partial<Dictation> = {}): Dictation => ({
  id, created_at: "2026-10-05T10:00:00Z", mode: "hold", app_name: "Code", audio_path: `a${id}.wav`, duration_ms: 4200,
  raw_text: `brut ${id}`, final_text: `final ${id}`, edited_text: null, level: "formatted", transcriber: "openai/whisper",
  corrector: "anthropic/claude", stt_ms: 800, llm_ms: 600, outcome: "pasted", error: null, ...over,
});

let rows: Dictation[];
const scrolled = vi.fn();

beforeEach(() => {
  resetTauri();
  scrolled.mockClear();
  Element.prototype.scrollIntoView = scrolled;
  rows = [dictation(1), dictation(2, { outcome: "error", raw_text: null, final_text: null, audio_path: null, error: "clé refusée", app_name: null, stt_ms: null, llm_ms: null })];
  commands({
    list_dictations: () => rows,
    save_edited_text: () => undefined,
    copy_dictation: () => undefined,
    retranscribe: () => undefined,
    delete_dictation: () => undefined,
  });
});

const card = (id: number) => document.getElementById(`d-${id}`)!;
const button = (id: number, name: string | RegExp) => within(card(id)).getByRole("button", { name });

async function mount(props: Record<string, unknown> = {}) {
  const r = render(History, { props });
  await screen.findByText("final 1");
  return r;
}

describe("History", () => {
  it("shows each dictation with its badge, app, timings and text", async () => {
    await mount();
    expect(calls("list_dictations")).toEqual([{ query: null, limit: 50, offset: 0 }]);
    expect(within(card(1)).getByText("Inséré")).toBeTruthy();
    expect(within(card(1)).getByText("· Code")).toBeTruthy();
    expect(within(card(1)).getByText("4.2 s · STT 800 ms · LLM 600 ms")).toBeTruthy();
    expect(within(card(2)).getByText("Erreur")).toBeTruthy();
    expect(within(card(2)).getByText("—")).toBeTruthy();
    expect(within(card(2)).getByText("clé refusée")).toBeTruthy();
    expect(within(card(2)).getByText("4.2 s")).toBeTruthy();
  });

  it("disables copy/edit without a transcript and retranscription without audio", async () => {
    await mount();
    for (const name of ["Copier", "Corriger", "Retranscrire"]) {
      expect((button(2, name) as HTMLButtonElement).disabled).toBe(true);
      expect((button(1, name) as HTMLButtonElement).disabled).toBe(false);
    }
  });

  it("shows the empty state and a load error", async () => {
    rows = [];
    commands({ list_dictations: () => Promise.reject("base indisponible") });
    render(History);
    expect(await screen.findByText("base indisponible")).toBeTruthy();
    expect(screen.getByText(/Aucune dictée/)).toBeTruthy();
  });

  it("searches after a pause in typing, and shows only the latest answer", async () => {
    await mount();
    let answerOld!: (d: Dictation[]) => void;
    commands({
      list_dictations: (a) =>
        a!.query === "ab"
          ? new Promise<Dictation[]>((res) => (answerOld = res))
          : [dictation(5, { final_text: "nouveau" })],
    });
    const search = screen.getByPlaceholderText(/Rechercher/);
    await fireEvent.input(search, { target: { value: "a" } });
    await fireEvent.input(search, { target: { value: "ab" } });
    await waitFor(() => expect(calls("list_dictations")).toHaveLength(2)); // debounced: one request for « ab »
    expect(calls("list_dictations")[1]).toEqual({ query: "ab", limit: 50, offset: 0 });
    await fireEvent.input(search, { target: { value: "abc" } });
    expect(await screen.findByText("nouveau")).toBeTruthy();
    answerOld([dictation(6, { final_text: "périmé" })]);
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText("périmé")).toBeNull();
    expect(screen.getByText("nouveau")).toBeTruthy();
  });

  it("loads more pages after a full one", async () => {
    rows = Array.from({ length: 50 }, (_, i) => dictation(i + 1));
    await mount();
    rows = [dictation(51, { final_text: "page 2" })];
    await fireEvent.click(screen.getByText("Plus"));
    expect(await screen.findByText("page 2")).toBeTruthy();
    expect(calls("list_dictations")[1]).toEqual({ query: null, limit: 50, offset: 50 });
    expect(screen.getByText("final 1")).toBeTruthy();
    expect(screen.queryByText("Plus")).toBeNull();
  });

  it("reloads when the history changes", async () => {
    await mount();
    rows = [dictation(3, { final_text: "neuve" })];
    emit("history-changed");
    expect(await screen.findByText("neuve")).toBeTruthy();
  });

  it("edits from the shown text and saves the correction", async () => {
    rows = [dictation(1, { edited_text: "ma version" })];
    render(History);
    await screen.findByText("ma version");
    await fireEvent.click(button(1, "Corriger"));
    const area = within(card(1)).getByRole("textbox") as HTMLTextAreaElement;
    expect(area.value).toBe("ma version");
    await fireEvent.input(area, { target: { value: "corrigé" } });
    rows = [dictation(1, { edited_text: "corrigé" })];
    await fireEvent.click(screen.getByText("Enregistrer la correction"));
    expect(await screen.findByText("corrigé")).toBeTruthy();
    expect(calls("save_edited_text")).toEqual([{ id: 1, text: "corrigé" }]);
    expect(within(card(1)).queryByRole("textbox")).toBeNull();
  });

  it("a blank correction clears it; a failed save keeps the editor and the draft", async () => {
    await mount();
    await fireEvent.click(button(1, "Corriger"));
    const area = within(card(1)).getByRole("textbox") as HTMLTextAreaElement;
    commands({ save_edited_text: () => Promise.reject("trop long") });
    await fireEvent.input(area, { target: { value: "   " } });
    await fireEvent.click(screen.getByText("Enregistrer la correction"));
    expect(await screen.findByText("trop long")).toBeTruthy();
    expect(calls("save_edited_text")).toEqual([{ id: 1, text: null }]);
    expect((within(card(1)).getByRole("textbox") as HTMLTextAreaElement).value).toBe("   ");
    await fireEvent.click(within(card(1)).getByText("Annuler"));
    expect(within(card(1)).queryByRole("textbox")).toBeNull();
  });

  it("copies a dictation and reports a failure", async () => {
    await mount();
    await fireEvent.click(button(1, "Copier"));
    expect(calls("copy_dictation")).toEqual([{ id: 1 }]);
    commands({ copy_dictation: () => Promise.reject("presse-papier occupé") });
    await fireEvent.click(button(1, "Copier"));
    expect(await screen.findByText("presse-papier occupé")).toBeTruthy();
    commands({ copy_dictation: () => undefined });
    await fireEvent.click(button(1, "Copier"));
    await waitFor(() => expect(screen.queryByText("presse-papier occupé")).toBeNull());
  });

  it("retranscribes, showing the card busy meanwhile", async () => {
    let done!: () => void;
    commands({ retranscribe: () => new Promise<void>((res) => (done = res)) });
    await mount();
    await fireEvent.click(button(1, "Retranscrire"));
    const busy = button(1, "…") as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
    done();
    await waitFor(() => expect(button(1, "Retranscrire")).toBeTruthy());
    expect(calls("retranscribe")).toEqual([{ id: 1 }]);
  });

  it("shows and hides the details", async () => {
    rows = [dictation(1, { edited_text: "édité" }), dictation(2, { raw_text: null, final_text: null, transcriber: null, corrector: null })];
    render(History);
    await screen.findByText("édité");
    await fireEvent.click(button(1, "Détails"));
    expect(within(card(1)).getByText("Votre correction")).toBeTruthy();
    expect(within(card(1)).getByText("openai/whisper / anthropic/claude")).toBeTruthy();
    await fireEvent.click(button(2, "Détails"));
    expect(within(card(1)).queryByText("Votre correction")).toBeNull();
    expect(within(card(2)).getByText("— / —")).toBeTruthy();
    expect(within(card(2)).queryByText("Votre correction")).toBeNull();
    await fireEvent.click(button(2, "Moins"));
    expect(within(card(2)).queryByText("Brut")).toBeNull();
  });

  it("deletes only after confirmation", async () => {
    await mount();
    await fireEvent.click(button(1, "Supprimer"));
    await fireEvent.click(button(1, "Annuler"));
    expect(calls("delete_dictation")).toHaveLength(0);
    await fireEvent.click(button(1, "Supprimer"));
    rows = [rows[1]];
    await fireEvent.click(button(1, "Confirmer la suppression"));
    await waitFor(() => expect(document.getElementById("d-1")).toBeNull());
    expect(calls("delete_dictation")).toEqual([{ id: 1 }]);
  });

  it("keeps the confirmation and shows the error when deleting fails", async () => {
    commands({ delete_dictation: () => Promise.reject("fichier verrouillé") });
    await mount();
    await fireEvent.click(button(1, "Supprimer"));
    await fireEvent.click(button(1, "Confirmer la suppression"));
    expect(await screen.findByText("fichier verrouillé")).toBeTruthy();
    expect(button(1, "Confirmer la suppression")).toBeTruthy();
  });

  it("opened on a dictation: loads, highlights and scrolls to it, then acknowledges", async () => {
    const onfocused = vi.fn();
    render(History, { props: { focus: { id: 2 }, onfocused } });
    await waitFor(() => expect(onfocused).toHaveBeenCalledTimes(1));
    expect(calls("list_dictations")).toHaveLength(1); // no second load from onMount
    expect(card(2).classList.contains("highlighted")).toBe(true);
    expect(card(1).classList.contains("highlighted")).toBe(false);
    expect(scrolled).toHaveBeenCalledWith({ behavior: "smooth", block: "center" });
  });

  it("a focus request clears the search", async () => {
    const { rerender } = render(History, { props: { focus: null } });
    await screen.findByText("final 1");
    const search = screen.getByPlaceholderText(/Rechercher/) as HTMLInputElement;
    await fireEvent.input(search, { target: { value: "xyz" } });
    await rerender({ focus: { id: 1 } });
    await waitFor(() => expect(card(1).classList.contains("highlighted")).toBe(true));
    expect(search.value).toBe("");
    expect(calls("list_dictations").at(-1)).toEqual({ query: null, limit: 50, offset: 0 });
  });

  it("stops listening when unmounted", async () => {
    const { unmount } = await mount();
    unmount();
    await waitFor(() => expect(emit("history-changed")).toBe(0));
  });
});

