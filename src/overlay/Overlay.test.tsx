import { afterEach, beforeEach, describe, expect, it, vi } from "bun:test";
import { fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { calls, commands, emit, resetTauri } from "../../tests/tauri";
import { lang } from "../lib/i18n";
import Overlay from "./Overlay";
import styles from "./Overlay.module.css";

const toast = (over: Record<string, unknown> = {}) => ({
  kind: "toast", level: "info", message: { code: "copied", correction: null }, preview: "Bonjour", dictation_id: 7, style: null, ...over,
});
const SLACK = { id: "slack", name: "Slack" };

async function mount() {
  const r = render(<Overlay />);
  await waitFor(() => expect(emit("overlay", { kind: "idle" })).toBe(1));
  return r;
}

const COPIED = "Texte copié dans le presse-papier";

const fire = async (name: string, payload: unknown) => {
  emit(name, payload);
  await Promise.resolve();
};

beforeEach(() => {
  resetTauri();
  commands({ overlay_dismiss: () => undefined, copy_dictation: () => undefined, open_history: () => undefined, overlay_hit_areas: () => undefined });
});
afterEach(() => {
  vi.useRealTimers();
});

describe("Overlay", () => {
  it("shows nothing while idle", async () => {
    const { container } = await mount();
    expect(container.querySelector(`.${styles.pill}, .${styles.toast}`)).toBeNull();
  });

  it("shows the recording pill with a live meter and the lock tag", async () => {
    const { container } = await mount();
    await fire("overlay", { kind: "recording", locked: false, warning: null, style: null });
    const bars = () => [...container.querySelectorAll<HTMLElement>(`.${styles.bars} span`)].map((b) => b.style.height);
    expect(bars()).toHaveLength(12);
    expect(screen.queryByText("Verrouillé")).toBeNull();
    await fire("audio-level", 1);
    expect(bars().at(-1)).toBe("28px");
    await fire("overlay", { kind: "recording", locked: true, warning: null, style: null });
    expect(screen.getByText("Verrouillé")).toBeTruthy();
    expect(bars().at(-1)).toBe("28px"); // locking keeps the meter
  });

  it("warns under the pill when the microphone is not exclusive, locked or not", async () => {
    await mount();
    await fire("overlay", { kind: "recording", locked: false, warning: null, style: null });
    expect(screen.queryByRole("alert")).toBeNull();
    const text = "Micro non exclusif : les autres applications vous entendent";
    await fire("overlay", { kind: "recording", locked: false, warning: "shared_microphone", style: null });
    expect(screen.getByRole("alert").textContent).toBe(text);
    await fire("overlay", { kind: "recording", locked: true, warning: "shared_microphone", style: null });
    expect(screen.getByRole("alert").textContent).toBe(text);
    expect(screen.getByText("Verrouillé")).toBeTruthy();
    await fire("overlay", { kind: "processing", style: null });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows the processing pill", async () => {
    await mount();
    await fire("overlay", { kind: "processing", style: null });
    expect(screen.getByText("Transcription…")).toBeTruthy();
  });

  it("shows a toast with its preview and buttons; Copier copies then closes", async () => {
    const { container } = await mount();
    await fire("overlay", toast());
    expect(screen.getByText(COPIED)).toBeTruthy();
    expect(screen.getByText("Bonjour")).toBeTruthy();
    expect(container.querySelector(`.${styles.toast}[data-level="info"]`)).toBeTruthy();
    await fireEvent.click(screen.getByText("Copier"));
    await waitFor(() => expect(screen.queryByText(COPIED)).toBeNull());
    expect(calls("copy_dictation")).toEqual([{ id: 7 }]);
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("an error toast has Voir but no Copier, and no preview line when there is none", async () => {
    const { container } = await mount();
    await fire("overlay", toast({ level: "error", preview: null, message: { code: "transcription_failed", error: { code: "timeout" } } }));
    expect(screen.queryByText("Copier")).toBeNull();
    expect(container.querySelector(`.${styles.toast}.${styles.error}[data-level="error"]`)).toBeTruthy();
    expect(container.querySelector(`.${styles.toast} p`)).toBeNull();
    await fireEvent.click(screen.getByText("Voir"));
    expect(calls("open_history")).toEqual([{ id: 7 }]);
    expect(screen.getByText("Échec de la transcription : fournisseur injoignable. Réessayez depuis l'historique.")).toBeTruthy();
  });

  it("a toast without dictation only has the close button", async () => {
    await mount();
    await fire("overlay", toast({ dictation_id: null }));
    expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual(["Fermer"]);
    await fireEvent.click(screen.getByLabelText("Fermer"));
    expect(screen.queryByText(COPIED)).toBeNull();
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("a toast closes itself after its timeout", async () => {
    vi.useFakeTimers();
    await mount();
    await fire("overlay", toast());
    vi.advanceTimersByTime(5999);
    await Promise.resolve();
    expect(screen.getByText(COPIED)).toBeTruthy();
    vi.advanceTimersByTime(1);
    await waitFor(() => expect(screen.queryByText(COPIED)).toBeNull());
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("shows the style badge while recording, locked, processing and on the toast", async () => {
    const { container } = await mount();
    const badge = () => container.querySelector<HTMLElement>(`.${styles.badge}`);
    await fire("overlay", { kind: "recording", locked: false, warning: null, style: null });
    expect(badge()).toBeNull();
    await fire("overlay", { kind: "recording", locked: false, warning: null, style: SLACK });
    expect(badge()!.textContent).toBe("Slack");
    expect(badge()!.title).toBe("Style : Slack");
    expect(badge()!.closest(`.${styles.pill}`)).toBeTruthy();
    await fire("overlay", { kind: "recording", locked: true, warning: null, style: SLACK });
    expect(badge()!.textContent).toBe("Slack");
    expect(screen.getByText("Verrouillé")).toBeTruthy();
    await fire("overlay", { kind: "processing", style: null });
    expect(badge()).toBeNull();
    await fire("overlay", { kind: "processing", style: { id: "mail", name: "Mail" } });
    expect(badge()!.textContent).toBe("Mail");
    await fire("overlay", toast({ style: SLACK }));
    expect(badge()!.closest(`.${styles.toast}`)).toBeTruthy();
    expect(badge()!.textContent).toBe("Slack");
    await fire("overlay", toast());
    expect(badge()).toBeNull();
  });

  it("a keyword-style paste ends on an « inserted » toast with its badge, kept while hovered", async () => {
    vi.useFakeTimers();
    const { container } = await mount();
    await fire("overlay", toast({ message: { code: "pasted" }, preview: null, style: SLACK }));
    expect(screen.getByText("Texte inséré")).toBeTruthy();
    expect(container.querySelector(`.${styles.badge}`)!.textContent).toBe("Slack");
    const root = container.querySelector(`.${styles.toast}`)!;
    await fireEvent.mouseEnter(root);
    vi.advanceTimersByTime(20000);
    await Promise.resolve();
    expect(screen.getByText("Texte inséré")).toBeTruthy();
    await fireEvent.mouseLeave(root);
    vi.advanceTimersByTime(999);
    await Promise.resolve();
    expect(screen.getByText("Texte inséré")).toBeTruthy();
    vi.advanceTimersByTime(1);
    await waitFor(() => expect(screen.queryByText("Texte inséré")).toBeNull());
    expect(calls("overlay_dismiss")).toHaveLength(1);
  });

  it("sends its buttons' areas when they change, and follows the native hover", async () => {
    vi.useFakeTimers();
    await mount();
    expect(calls("overlay_hit_areas")).toEqual([{ surface: null, buttons: [] }]);
    await fire("overlay", toast({ style: SLACK, message: { code: "pasted" } }));
    expect(calls("overlay_hit_areas")).toHaveLength(2);
    expect(calls("overlay_hit_areas")[1]!.surface).not.toBeNull();
    expect((calls("overlay_hit_areas")[1]!.buttons as unknown[]).length).toBe(screen.getAllByRole("button").length);
    await fire("audio-level", 0.5);
    expect(calls("overlay_hit_areas")).toHaveLength(2);
    await fire("overlay-pointer", { inside: true, button: 0 });
    expect(screen.getAllByRole("button")[0].hasAttribute("data-hover")).toBe(true);
    vi.advanceTimersByTime(20000);
    await Promise.resolve();
    expect(screen.getByText("Texte inséré")).toBeTruthy();
    await fire("overlay-pointer", { inside: false, button: null });
    expect(screen.getAllByRole("button")[0].hasAttribute("data-hover")).toBe(false);
    vi.advanceTimersByTime(1000);
    await waitFor(() => expect(screen.queryByText("Texte inséré")).toBeNull());
    expect(calls("overlay_hit_areas").at(-1)).toEqual({ surface: null, buttons: [] });
  });

  it("logs a failed hit-area update", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    commands({ overlay_hit_areas: () => Promise.reject(new Error("boom")) });
    await mount();
    await waitFor(() => expect(error).toHaveBeenCalledWith(new Error("boom")));
    error.mockRestore();
  });

  it("shows a notice's explanation on its own line under the title", async () => {
    await mount();
    await fire("overlay", toast({ level: "copied", message: { code: "not_pasted", correction: { code: "timeout" } } }));
    expect(screen.getByText("Non corrigé : fournisseur injoignable").className).toBe(styles.detail);
    expect(screen.getByText(/^Non inséré : texte copié/).tagName).toBe("STRONG");
  });

  it("a failed copy shows an error toast instead of an unhandled rejection", async () => {
    commands({ copy_dictation: () => Promise.reject("presse-papier occupé") });
    await mount();
    await fire("overlay", toast());
    await fireEvent.click(screen.getByText("Copier"));
    expect(await screen.findByText("Copie impossible : presse-papier occupé")).toBeTruthy();
    expect(calls("overlay_dismiss")).toHaveLength(0);
  });

  it("logs a failed dismiss instead of leaving an unhandled rejection", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      commands({ overlay_dismiss: () => Promise.reject("fenêtre fermée") });
      await mount();
      await fire("overlay", toast());
      await fireEvent.click(screen.getByLabelText("Fermer"));
      await waitFor(() => expect(logged).toHaveBeenCalledWith("fenêtre fermée"));
      expect(screen.queryByText(COPIED)).toBeNull();
    } finally {
      logged.mockRestore();
    }
  });

  it("speaks English, and follows a language change while shown", async () => {
    lang.value = "en";
    await mount();
    await fire("overlay", { kind: "recording", locked: true, warning: "shared_microphone", style: null });
    expect(screen.getByRole("status", { name: "Recording" })).toBeTruthy();
    expect(screen.getByText("Locked")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toBe("Microphone not exclusive: other apps can hear you");
    await fire("overlay", toast({ style: SLACK }));
    expect(screen.getByText("Text copied to the clipboard")).toBeTruthy();
    expect(screen.getByTitle("Style: Slack")).toBeTruthy();
    expect(screen.getByText("Copy")).toBeTruthy();
    await fire("language", "fr");
    expect(await screen.findByText(COPIED)).toBeTruthy();
    expect(screen.getByText("Voir")).toBeTruthy();
  });

  it("starts in the backend's language", async () => {
    commands({ language: () => "en" });
    await mount();
    await fire("overlay", { kind: "processing", style: null });
    expect(await screen.findByText("Transcribing…")).toBeTruthy();
  });

  it("stops listening when unmounted", async () => {
    const { unmount } = await mount();
    unmount();
    await waitFor(() => expect(emit("overlay", { kind: "processing", style: null }) + emit("audio-level", 0.5) + emit("overlay-pointer", { inside: false, button: null })).toBe(0));
  });
});
