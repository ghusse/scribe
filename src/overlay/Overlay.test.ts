import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { calls, commands, emit, resetTauri } from "../../tests/tauri";
import Overlay from "./Overlay.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));

const toast = (over: Record<string, unknown> = {}) => ({
  kind: "toast", level: "info", message: "Inséré", preview: "Bonjour", dictation_id: 7, ...over,
});

async function mount() {
  const r = render(Overlay);
  await waitFor(() => expect(emit("overlay", { kind: "idle" })).toBe(1));
  return r;
}

const fire = async (name: string, payload: unknown) => {
  emit(name, payload);
  await Promise.resolve();
};

beforeEach(() => {
  resetTauri();
  commands({ overlay_dismiss: () => undefined, copy_dictation: () => undefined, open_history: () => undefined });
});
afterEach(() => vi.useRealTimers());

describe("Overlay", () => {
  it("shows nothing while idle", async () => {
    const { container } = await mount();
    expect(container.querySelector(".pill, .toast")).toBeNull();
  });

  it("shows the recording pill with a live meter and the lock tag", async () => {
    const { container } = await mount();
    await fire("overlay", { kind: "recording", locked: false });
    const bars = () => [...container.querySelectorAll<HTMLElement>(".bars span")].map((b) => b.style.height);
    expect(bars()).toHaveLength(12);
    expect(screen.queryByText("Verrouillé")).toBeNull();
    await fire("audio-level", 1);
    expect(bars().at(-1)).toBe("28px");
    await fire("overlay", { kind: "recording", locked: true });
    expect(screen.getByText("Verrouillé")).toBeTruthy();
    expect(bars().at(-1)).toBe("28px"); // locking keeps the meter
  });

  it("shows the processing pill", async () => {
    await mount();
    await fire("overlay", { kind: "processing" });
    expect(screen.getByText("Transcription…")).toBeTruthy();
  });

  it("shows a toast with its preview and buttons; Copier copies then closes", async () => {
    const { container } = await mount();
    await fire("overlay", toast());
    expect(screen.getByText("Inséré")).toBeTruthy();
    expect(screen.getByText("Bonjour")).toBeTruthy();
    expect(container.querySelector(".toast.info")).toBeTruthy();
    await fireEvent.click(screen.getByText("Copier"));
    await waitFor(() => expect(screen.queryByText("Inséré")).toBeNull());
    expect(calls("copy_dictation")).toEqual([{ id: 7 }]);
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("an error toast has Voir but no Copier, and no preview line when there is none", async () => {
    const { container } = await mount();
    await fire("overlay", toast({ level: "error", preview: null, message: "Échec" }));
    expect(screen.queryByText("Copier")).toBeNull();
    expect(container.querySelector(".toast p")).toBeNull();
    await fireEvent.click(screen.getByText("Voir"));
    expect(calls("open_history")).toEqual([{ id: 7 }]);
    expect(screen.getByText("Échec")).toBeTruthy();
  });

  it("a toast without dictation only has the close button", async () => {
    await mount();
    await fire("overlay", toast({ dictation_id: null }));
    expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual(["Fermer"]);
    await fireEvent.click(screen.getByLabelText("Fermer"));
    expect(screen.queryByText("Inséré")).toBeNull();
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("a toast closes itself after its timeout", async () => {
    vi.useFakeTimers();
    await mount();
    await fire("overlay", toast());
    vi.advanceTimersByTime(5999);
    await Promise.resolve();
    expect(screen.getByText("Inséré")).toBeTruthy();
    vi.advanceTimersByTime(1);
    await vi.waitFor(() => expect(screen.queryByText("Inséré")).toBeNull());
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("a failed copy shows an error toast instead of an unhandled rejection", async () => {
    commands({ copy_dictation: () => Promise.reject("presse-papier occupé") });
    await mount();
    await fire("overlay", toast());
    await fireEvent.click(screen.getByText("Copier"));
    expect(await screen.findByText(/^Copie impossible\s: presse-papier occupé$/)).toBeTruthy();
    expect(calls("overlay_dismiss")).toHaveLength(0);
  });

  it("logs a failed dismiss instead of leaving an unhandled rejection", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    commands({ overlay_dismiss: () => Promise.reject("fenêtre fermée") });
    await mount();
    await fire("overlay", toast());
    await fireEvent.click(screen.getByLabelText("Fermer"));
    await waitFor(() => expect(logged).toHaveBeenCalledWith("fenêtre fermée"));
    expect(screen.queryByText("Inséré")).toBeNull();
    logged.mockRestore();
  });

  it("stops listening when unmounted", async () => {
    const { unmount } = await mount();
    unmount();
    await waitFor(() => expect(emit("overlay", { kind: "processing" }) + emit("audio-level", 0.5)).toBe(0));
  });
});
