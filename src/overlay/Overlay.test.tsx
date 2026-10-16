import { afterEach, beforeEach, describe, expect, it, vi } from "bun:test";
import { fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { styleCatalog } from "../../tests/fixtures";
import { calls, commands, emit, resetTauri } from "../../tests/tauri";
import { advanceTimers } from "../../tests/timers";
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
  commands({ overlay_dismiss: () => undefined, copy_dictation: () => undefined, open_history: () => undefined, overlay_hit_areas: () => undefined, style_catalog: () => styleCatalog() });
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
    await fire("overlay", { kind: "recording", locked: false, style: null });
    const bars = () => [...container.querySelectorAll<HTMLElement>(`.${styles.bars} span`)].map((b) => b.style.height);
    expect(bars()).toHaveLength(12);
    expect(screen.queryByText("Verrouillé")).toBeNull();
    await fire("audio-level", 1);
    expect(bars().at(-1)).toBe("28px");
    await fire("overlay", { kind: "recording", locked: true, style: null });
    expect(screen.getByText("Verrouillé")).toBeTruthy();
    expect(bars().at(-1)).toBe("28px"); // locking keeps the meter
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
    await fire("overlay", { kind: "recording", locked: false, style: null });
    expect(badge()).toBeNull();
    await fire("overlay", { kind: "recording", locked: false, style: SLACK });
    expect(badge()!.textContent).toBe("Slack");
    expect(badge()!.title).toBe("Style : Slack");
    expect(badge()!.closest(`.${styles.pill}`)).toBeTruthy();
    await fire("overlay", { kind: "recording", locked: true, style: SLACK });
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
    await advanceTimers(0);
    // Once the catalog arrives, the badge becomes the style switch button.
    expect(calls("overlay_hit_areas")).toHaveLength(3);
    expect(calls("overlay_hit_areas")[2]!.surface).not.toBeNull();
    expect((calls("overlay_hit_areas")[2]!.buttons as unknown[]).length).toBe(screen.getAllByRole("button").length);
    await fire("audio-level", 0.5);
    expect(calls("overlay_hit_areas")).toHaveLength(3);
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
    await fire("overlay", { kind: "recording", locked: true, style: null });
    expect(screen.getByRole("status", { name: "Recording" })).toBeTruthy();
    expect(screen.getByText("Locked")).toBeTruthy();
    await fire("overlay", toast({ style: SLACK }));
    expect(screen.getByText("Text copied to the clipboard")).toBeTruthy();
    expect(await screen.findByTitle("Change style")).toBeTruthy();
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

  describe("style switch", () => {
    const PASTED = toast({ message: { code: "pasted" }, preview: null, style: SLACK });
    const chips = () => screen.getByRole("group", { name: "Styles" });

    async function picker() {
      const r = await mount();
      await fire("overlay", PASTED);
      await fireEvent.click(await screen.findByTitle("Changer de style"));
      return r;
    }

    it("opens one scrollable row of style chips in the toast, the current style marked", async () => {
      const { container } = await picker();
      const row = chips();
      expect(row.className).toBe(styles.chips);
      // happy-dom applies no stylesheet: the row's own rule makes it scroll sideways.
      expect(await Bun.file(new URL("./Overlay.module.css", import.meta.url)).text()).toMatch(/\.chips \{[^}]*overflow-x: auto/);
      expect(row.closest("[data-surface]")).toBe(container.querySelector(`.${styles.toast}`));
      const buttons = [...row.querySelectorAll("button")];
      expect(buttons.map((b) => b.textContent)).toEqual(["Par défaut", "Propre", "Slack", "Mail"]);
      const current = screen.getByRole("button", { name: "Slack" }) as HTMLButtonElement;
      expect(current.getAttribute("aria-pressed")).toBe("true");
      expect(current.disabled).toBe(true);
      expect(current.className).toBe(styles.current);
      expect(screen.getByRole("button", { name: "Mail" }).getAttribute("aria-pressed")).toBe("false");
      expect(screen.queryByText("Copier")).toBeNull();
      expect(screen.queryByTitle("Changer de style")).toBeNull();
      expect(screen.getByLabelText("Fermer")).toBeTruthy();
      const enabled = screen.getAllByRole("button").filter((b) => !(b as HTMLButtonElement).disabled);
      expect((calls("overlay_hit_areas").at(-1)!.buttons as unknown[]).length).toBe(enabled.length);
    });

    it("rewrites with the chip clicked, then shows the backend's result", async () => {
      commands({ restyle_dictation: () => undefined });
      await picker();
      await fireEvent.click(screen.getByRole("button", { name: "Mail" }));
      expect(calls("restyle_dictation")).toEqual([{ id: 7, styleId: "mail" }]);
      expect(screen.getByText("Réécriture en style Mail…")).toBeTruthy();
      expect(screen.queryByRole("group", { name: "Styles" })).toBeNull();
      expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual(["Fermer"]);
      await fire("overlay", toast({ message: { code: "restyled" }, preview: null, style: { id: "mail", name: "Mail" } }));
      expect(screen.getByText("Texte remplacé")).toBeTruthy();
      expect(await screen.findByTitle("Changer de style")).toBeTruthy();
    });

    it("a refused restyle shows an error toast", async () => {
      commands({ restyle_dictation: () => Promise.reject({ code: "style_not_found" }) });
      await picker();
      await fireEvent.click(screen.getByRole("button", { name: "Mail" }));
      expect(await screen.findByText("Réécriture impossible : ce style n'existe plus")).toBeTruthy();
      expect(screen.queryByTitle("Changer de style")).toBeNull();
    });

    it("the picker stays open past the toast's delay", async () => {
      vi.useFakeTimers();
      await mount();
      await fire("overlay", PASTED);
      await advanceTimers(0);
      await fireEvent.click(screen.getByTitle("Changer de style"));
      await advanceTimers(30000);
      expect(chips()).toBeTruthy();
      expect(calls("overlay_dismiss")).toHaveLength(0);
    });

    it("no style switch when correction is off", async () => {
      commands({ style_catalog: () => styleCatalog({ correction_enabled: false }) });
      await mount();
      await fire("overlay", PASTED);
      await waitFor(() => expect(calls("style_catalog")).toHaveLength(1));
      await Promise.resolve();
      expect(screen.queryByTitle("Changer de style")).toBeNull();
      expect(screen.getByText("Slack").tagName).toBe("SPAN");
    });

    it("no style switch on a toast without a badge", async () => {
      await mount();
      await fire("overlay", PASTED);
      expect(await screen.findByTitle("Changer de style")).toBeTruthy();
      await fire("overlay", toast({ style: null }));
      expect(screen.getByText("Texte copié dans le presse-papier")).toBeTruthy();
      expect(screen.queryByTitle("Changer de style")).toBeNull();
    });

    it("a vertical wheel scrolls the chips sideways", async () => {
      await picker();
      const row = chips();
      Object.defineProperty(row, "scrollWidth", { value: 600 });
      Object.defineProperty(row, "clientWidth", { value: 300 });
      const wheel = new WheelEvent("wheel", { deltaY: 40, cancelable: true });
      row.dispatchEvent(wheel);
      expect(row.scrollLeft).toBe(40);
      expect(wheel.defaultPrevented).toBe(true);
    });

    it("sends the chips' areas again when the row scrolls", async () => {
      await picker();
      const row = chips();
      const sent = calls("overlay_hit_areas").length;
      const mail = screen.getByRole("button", { name: "Mail" });
      mail.getBoundingClientRect = () => ({ x: 1, y: 2, width: 3, height: 4 }) as DOMRect;
      row.getBoundingClientRect = () => ({ x: 0, y: 0, width: 400, height: 30 }) as DOMRect;
      await fireEvent.scroll(row);
      expect(calls("overlay_hit_areas")).toHaveLength(sent + 1);
      expect(calls("overlay_hit_areas").at(-1)!.buttons).toContainEqual([1, 2, 3, 4]);
    });

    it("renames the chips when the language changes", async () => {
      await picker();
      commands({ style_catalog: () => styleCatalog({ styles: [{ id: "default", name: "Default" }, SLACK] }) });
      await fire("language", "en");
      expect(await screen.findByRole("button", { name: "Default" })).toBeTruthy();
      expect(screen.getByRole("group", { name: "Styles" })).toBeTruthy();
    });
  });
});
