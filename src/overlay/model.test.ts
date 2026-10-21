import { describe, expect, it, vi } from "bun:test";
import type { OverlayEvent, StyleCatalog } from "../lib/api";
import { styleCatalog } from "../../tests/fixtures";
import { barHeight, createOverlay, emptyLevels, hitAreas, initialView, markHovered, LEAVE_GRACE_MS, LEVEL_BARS, pushLevel, reduce, toastActions, toastTimeout, wheelSideways, type OverlayView } from "./model";

type Toast = Extract<OverlayEvent, { kind: "toast" }>;
const SLACK = { id: "slack", name: "Slack" };
const toast = (over: Partial<Toast> = {}): Toast => ({
  kind: "toast", level: "info", message: { code: "copied", correction: null }, preview: null, dictation_id: 7, style: null, ...over,
});

describe("pure helpers", () => {
  it("keeps error toasts longest, then badged toasts", () => {
    expect(toastTimeout(toast({ level: "error" }))).toBe(10000);
    expect(toastTimeout(toast({ level: "error", style: SLACK }))).toBe(10000);
    for (const level of ["info", "uncertain", "copied"] as const) {
      expect(toastTimeout(toast({ level }))).toBe(6000);
      expect(toastTimeout(toast({ level, style: SLACK }))).toBe(8000);
    }
    expect(toastTimeout(toast({ message: { code: "pasted" }, style: SLACK }))).toBe(8000);
  });

  it("lists the visible surface and the buttons' areas", () => {
    const root = document.createElement("div");
    root.innerHTML = "<div data-surface><strong>Texte</strong><button>Copier</button><span><button>Voir</button></span></div><p data-surface>!</p>";
    const rect = (el: Element, x: number, y: number, width: number, height: number) =>
      (el.getBoundingClientRect = () => ({ x, y, width, height }) as DOMRect);
    const [copy, view] = root.querySelectorAll("button");
    const [pill, below] = root.querySelectorAll("[data-surface]");
    rect(copy, 10, 5, 40, 24);
    rect(view, 56, 5, 30, 24);
    rect(pill, 0, 0, 100, 34);
    rect(below, 20, 40, 120, 20);
    expect(hitAreas(root)).toEqual({ surface: [0, 0, 140, 60], buttons: [[10, 5, 40, 24], [56, 5, 30, 24]] });
    expect(hitAreas(document.createElement("div"))).toEqual({ surface: null, buttons: [] });
  });

  it("marks only the hovered button", () => {
    const root = document.createElement("div");
    root.innerHTML = "<button>Copier</button><button>Voir</button>";
    const marked = () => [...root.querySelectorAll("button")].map((b) => b.hasAttribute("data-hover"));
    markHovered(root, 1);
    expect(marked()).toEqual([false, true]);
    markHovered(root, 0);
    expect(marked()).toEqual([true, false]);
    markHovered(root, null);
    expect(marked()).toEqual([false, false]);
  });

  it("leaves disabled buttons out of the hit areas and of the hover indexes", () => {
    const root = document.createElement("div");
    root.innerHTML = "<button>A</button><button disabled>B</button><button>C</button>";
    const [a, b, c] = root.querySelectorAll("button");
    for (const [el, x] of [[a, 0], [b, 50], [c, 100]] as const) el.getBoundingClientRect = () => ({ x, y: 0, width: 40, height: 20 }) as DOMRect;
    expect(hitAreas(root).buttons).toEqual([[0, 0, 40, 20], [100, 0, 40, 20]]);
    markHovered(root, 1);
    expect([a, b, c].map((el) => el.hasAttribute("data-hover"))).toEqual([false, false, true]);
    markHovered(root, 5);
    expect([a, b, c].some((el) => el.hasAttribute("data-hover"))).toBe(false);
  });

  it("turns a vertical wheel into a sideways scroll of an overflowing row", () => {
    const row = { scrollLeft: 0, scrollWidth: 600, clientWidth: 300 };
    expect(wheelSideways(row, 0, 40)).toBe(true);
    expect(row.scrollLeft).toBe(40);
    expect(wheelSideways(row, 30, 10)).toBe(false);
    expect(wheelSideways({ ...row, scrollWidth: 300 }, 0, 40)).toBe(false);
    expect(row.scrollLeft).toBe(40);
  });

  it("scrolls the meter, amplifies and caps the level", () => {
    const start = emptyLevels();
    expect(start).toHaveLength(LEVEL_BARS);
    const one = pushLevel(start, 0.1);
    expect(one).toHaveLength(LEVEL_BARS);
    expect(one[LEVEL_BARS - 1]).toBeCloseTo(0.6);
    expect(one.slice(0, -1).every((l) => l === 0)).toBe(true);
    expect(pushLevel(one, 5)[LEVEL_BARS - 1]).toBe(1);
    expect(pushLevel(one, 5)[LEVEL_BARS - 2]).toBeCloseTo(0.6);
    expect(pushLevel(one, -1)[LEVEL_BARS - 1]).toBe(0);
    expect(start.every((l) => l === 0)).toBe(true); // not mutated
  });

  it("maps a level to a bar height", () => {
    expect(barHeight(0)).toBe(4);
    expect(barHeight(1)).toBe(28);
  });

  it("offers Copier and Voir only for a dictation, and no Copier on errors", () => {
    expect(toastActions({ kind: "idle" })).toEqual({ copy: false, view: false, restyle: false });
    expect(toastActions({ kind: "processing", style: null })).toEqual({ copy: false, view: false, restyle: false });
    expect(toastActions(toast({ dictation_id: null }))).toEqual({ copy: false, view: false, restyle: false });
    expect(toastActions(toast())).toEqual({ copy: true, view: true, restyle: false });
    expect(toastActions(toast({ level: "copied" }))).toEqual({ copy: true, view: true, restyle: false });
    expect(toastActions(toast({ level: "error" }))).toEqual({ copy: false, view: true, restyle: false });
  });

  it("offers a style switch only on a badged dictation toast with a transcript, correction on", () => {
    for (const message of [
      { code: "pasted" },
      { code: "pasted_uncertain", correction: null },
      { code: "pasted_without_correction", correction: { code: "timeout" } },
      { code: "copied", correction: null },
      { code: "not_pasted", correction: null },
      { code: "restyled" },
      { code: "restyle_copied" },
    ] as const) {
      expect(toastActions(toast({ message, style: SLACK }), true).restyle).toBe(true);
    }
    expect(toastActions(toast({ style: SLACK }), false).restyle).toBe(false);
    expect(toastActions(toast({ style: SLACK })).restyle).toBe(false);
    expect(toastActions(toast(), true).restyle).toBe(false);
    expect(toastActions(toast({ style: SLACK, dictation_id: null }), true).restyle).toBe(false);
    for (const message of [
      { code: "transcription_failed", error: { code: "timeout" } },
      { code: "clipboard_unavailable", correction: null },
      { code: "restyle_failed", error: { code: "timeout" } },
      { code: "restyle_refused", detail: "x" },
      { code: "copy_failed", detail: "x" },
    ] as const) {
      expect(toastActions(toast({ level: "error", message, style: SLACK }), true).restyle).toBe(false);
    }
  });

  it("keeps only the visible part of a button scrolled out of its row", () => {
    const root = document.createElement("div");
    root.innerHTML = "<div data-surface><div data-clip><button>A</button><button>B</button><button>C</button></div><button>x</button></div>";
    const rect = (el: Element, x: number, y: number, width: number, height: number) =>
      (el.getBoundingClientRect = () => ({ x, y, width, height }) as DOMRect);
    const [a, b, c, close] = root.querySelectorAll("button");
    rect(root.querySelector("[data-surface]")!, 0, 0, 300, 60);
    rect(root.querySelector("[data-clip]")!, 10, 30, 200, 22);
    rect(a, 10, 30, 80, 22);
    rect(b, 150, 30, 80, 22);
    rect(c, 240, 30, 80, 22);
    rect(close, 260, 10, 24, 24);
    expect(hitAreas(root).buttons).toEqual([[10, 30, 80, 22], [150, 30, 60, 22], [240, 30, 0, 22], [260, 10, 24, 24]]);
  });

  it("closes the picker and the rewrite state on any event, keeps the catalog", () => {
    const catalog = styleCatalog();
    const view: OverlayView = { ...initialView(), ov: toast({ style: SLACK }), catalog, picker: true, rewriting: SLACK };
    const next = reduce(view, toast({ message: { code: "restyled" }, style: SLACK })).view;
    expect(next).toMatchObject({ picker: false, rewriting: null });
    expect(next.catalog).toBe(catalog);
  });

  it("resets the meter on a new recording, keeps it when locking, schedules toasts", () => {
    const view: OverlayView = { ...initialView(), levels: pushLevel(emptyLevels(), 0.1) };
    const rec = reduce(view, { kind: "recording", locked: false, style: null });
    expect(rec.view.levels).toEqual(emptyLevels());
    expect(rec.timeout).toBeNull();
    const locked = reduce(view, { kind: "recording", locked: true, style: null });
    expect(locked.view.levels).toBe(view.levels);
    expect(locked.view.ov).toEqual({ kind: "recording", locked: true, style: null });
    expect(reduce(view, toast({ level: "error" })).timeout).toBe(10000);
    expect(reduce(view, toast({ message: { code: "pasted" }, style: SLACK })).timeout).toBe(8000);
    expect(reduce(view, { kind: "processing", style: null }).timeout).toBeNull();
  });
});

/** A controller on fake timers and a fake backend; `views` records every render. */
function setup() {
  const timers: { fn: () => void; ms: number; cancelled: boolean }[] = [];
  let resolveCopy: (() => void) | undefined;
  let rejectCopy: ((e: unknown) => void) | undefined;
  let resolveRestyle: (() => void) | undefined;
  let rejectRestyle: ((e: unknown) => void) | undefined;
  const catalogs: { resolve: (c: StyleCatalog) => void; reject: (e: unknown) => void }[] = [];
  const deps = {
    dismiss: vi.fn(() => Promise.resolve()),
    copy: vi.fn(() => new Promise<void>((res, rej) => { resolveCopy = res; rejectCopy = rej; })),
    openHistory: vi.fn(() => Promise.resolve()),
    styleCatalog: vi.fn(() => new Promise<StyleCatalog>((resolve, reject) => catalogs.push({ resolve, reject }))),
    restyle: vi.fn((_id: number, _styleId: string) => new Promise<void>((res, rej) => { resolveRestyle = res; rejectRestyle = rej; })),
    schedule: vi.fn((fn: () => void, ms: number) => { const t = { fn, ms, cancelled: false }; timers.push(t); return t; }),
    cancel: vi.fn((h: unknown) => { (h as { cancelled: boolean }).cancelled = true; }),
    report: vi.fn(),
  };
  const views: OverlayView[] = [];
  const ctl = createOverlay(deps, (v) => views.push(v));
  return {
    ctl, deps, views, timers, catalogs,
    resolveCopy: () => resolveCopy!(),
    rejectCopy: (e: unknown) => rejectCopy!(e),
    resolveRestyle: () => resolveRestyle!(),
    rejectRestyle: (e: unknown) => rejectRestyle!(e),
  };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("createOverlay", () => {
  it("starts idle and renders each event", () => {
    const { ctl, views } = setup();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    ctl.event({ kind: "processing", style: null });
    expect(views.at(-1)!.ov).toEqual({ kind: "processing", style: null });
  });

  it("feeds the level meter", () => {
    const { ctl, views } = setup();
    ctl.event({ kind: "recording", locked: false, style: null });
    ctl.level(0.1);
    expect(views.at(-1)!.levels[LEVEL_BARS - 1]).toBeCloseTo(0.6);
    expect(views.at(-1)!.ov).toEqual({ kind: "recording", locked: false, style: null });
  });

  it("dismisses a toast on its own after its timeout and tells the backend", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    expect(timers).toHaveLength(1);
    expect(timers[0].ms).toBe(6000);
    timers[0].fn();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    expect(deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("a toast whose delay runs out under the pointer waits for it to leave, then goes shortly after", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    ctl.hover(true);
    timers[0].fn();
    expect(ctl.view().ov.kind).toBe("toast");
    ctl.hover(false);
    expect(timers.map((t) => t.ms)).toEqual([6000, LEAVE_GRACE_MS]);
    ctl.hover(true);
    expect(timers[1].cancelled).toBe(true);
    ctl.hover(false);
    timers[2].fn();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    expect(deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("a toast closed under the pointer does not leave the next one hovered", () => {
    const { ctl, timers } = setup();
    ctl.event(toast());
    ctl.hover(true);
    ctl.dismiss();
    ctl.event(toast({ style: SLACK }));
    timers.at(-1)!.fn();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
  });

  it("a toast replaced by another view under the pointer does not leave a later toast hovered", () => {
    const { ctl, timers } = setup();
    ctl.event(toast());
    ctl.hover(true);
    ctl.event({ kind: "recording", locked: false, style: null });
    ctl.event(toast());
    timers.at(-1)!.fn();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
  });

  it("a toast replacing a hovered toast stays hovered", () => {
    const { ctl, timers } = setup();
    ctl.event(toast());
    ctl.hover(true);
    ctl.event(toast({ style: SLACK }));
    timers.at(-1)!.fn();
    expect(ctl.view().ov.kind).toBe("toast");
  });

  it("hovering before the delay runs out changes nothing", () => {
    const { ctl, timers } = setup();
    ctl.event(toast({ style: SLACK }));
    ctl.hover(true);
    ctl.hover(false);
    expect(timers).toHaveLength(1);
    expect(timers[0].cancelled).toBe(false);
    timers[0].fn();
    expect(ctl.view().ov).toEqual({ kind: "idle" });
  });

  it("a new event clears an expired toast's wait", () => {
    const { ctl, timers } = setup();
    ctl.event(toast());
    ctl.hover(true);
    timers[0].fn();
    ctl.event({ kind: "processing", style: null });
    ctl.hover(false);
    expect(timers).toHaveLength(1);
    expect(ctl.view().ov.kind).toBe("processing");
  });

  it("a new event cancels the pending toast timer", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    ctl.event({ kind: "recording", locked: false, style: null });
    expect(timers[0].cancelled).toBe(true);
    expect(deps.cancel).toHaveBeenCalledTimes(1);
  });

  it("closing by hand cancels the timer", () => {
    const { ctl, deps, timers } = setup();
    ctl.event(toast());
    ctl.dismiss();
    expect(timers[0].cancelled).toBe(true);
    expect(ctl.view().ov).toEqual({ kind: "idle" });
    expect(deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("destroy cancels the timer without rendering", () => {
    const { ctl, timers, views } = setup();
    ctl.event(toast());
    const n = views.length;
    ctl.destroy();
    expect(timers[0].cancelled).toBe(true);
    expect(views).toHaveLength(n);
  });

  it("reports a failed dismiss instead of leaving an unhandled rejection", async () => {
    const { ctl, deps } = setup();
    deps.dismiss.mockReturnValueOnce(Promise.reject("ipc down"));
    ctl.dismiss();
    await flush();
    expect(deps.report).toHaveBeenCalledWith("ipc down");
  });

  it("copies then dismisses", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    expect(s.deps.copy).toHaveBeenCalledWith(7);
    s.resolveCopy();
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "idle" });
    expect(s.deps.dismiss).toHaveBeenCalledTimes(1);
  });

  it("a recording that starts while copying is not hidden by the copy's dismiss", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.ctl.event({ kind: "recording", locked: false, style: null });
    s.resolveCopy();
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "recording", locked: false, style: null });
    expect(s.deps.dismiss).not.toHaveBeenCalled();
  });

  it("a failed copy turns into an error toast (no unhandled rejection), with Voir still offered", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.rejectCopy("presse-papier occupé");
    await expect(done).resolves.toBeUndefined();
    const ov = s.ctl.view().ov;
    expect(ov).toEqual({ kind: "toast", level: "error", message: { code: "copy_failed", detail: "presse-papier occupé" }, preview: null, dictation_id: 7, style: null });
    expect(toastActions(ov)).toEqual({ copy: false, view: true, restyle: false });
    expect(s.timers.at(-1)!.ms).toBe(10000);
    expect(s.deps.dismiss).not.toHaveBeenCalled();
  });

  it("a failed copy keeps the toast's style badge", async () => {
    const s = setup();
    s.ctl.event(toast({ style: SLACK }));
    const done = s.ctl.copy();
    s.rejectCopy("x");
    await done;
    expect(s.ctl.view().ov).toMatchObject({ level: "error", message: { code: "copy_failed" }, style: SLACK });
  });

  it("a copy refused by the backend keeps its untranslated detail", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.rejectCopy({ code: "system", detail: "clipboard locked" });
    await done;
    expect(s.ctl.view().ov).toMatchObject({ message: { code: "copy_failed", detail: "clipboard locked" } });
  });

  it("a failed copy does not cover an event that arrived meanwhile", async () => {
    const s = setup();
    s.ctl.event(toast());
    const done = s.ctl.copy();
    s.ctl.event({ kind: "processing", style: null });
    s.rejectCopy("x");
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "processing", style: null });
  });

  it("copy and Voir do nothing without a dictation", async () => {
    const s = setup();
    await s.ctl.copy();
    s.ctl.openHistory();
    s.ctl.event(toast({ dictation_id: null }));
    await s.ctl.copy();
    s.ctl.openHistory();
    expect(s.deps.copy).not.toHaveBeenCalled();
    expect(s.deps.openHistory).not.toHaveBeenCalled();
  });

  it("Voir opens the dictation, keeps the toast and reports failures", async () => {
    const s = setup();
    s.ctl.event(toast({ dictation_id: 3 }));
    s.ctl.openHistory();
    expect(s.deps.openHistory).toHaveBeenCalledWith(3);
    expect(s.ctl.view().ov.kind).toBe("toast");
    s.deps.openHistory.mockReturnValueOnce(Promise.reject("no window"));
    s.ctl.openHistory();
    await flush();
    expect(s.deps.report).toHaveBeenCalledWith("no window");
  });

  describe("style switch", () => {
    const MAIL = { id: "mail", name: "Mail" };
    /** A badged toast with its catalog loaded. */
    async function ready(over: Partial<StyleCatalog> = {}) {
      const s = setup();
      s.ctl.event(toast({ message: { code: "pasted" }, style: SLACK }));
      s.catalogs.at(-1)!.resolve(styleCatalog(over));
      await flush();
      return s;
    }

    it("fetches the catalog for each badged dictation toast only", async () => {
      const s = setup();
      s.ctl.event(toast());
      s.ctl.event(toast({ style: SLACK, dictation_id: null }));
      s.ctl.event({ kind: "processing", style: SLACK });
      expect(s.deps.styleCatalog).not.toHaveBeenCalled();
      s.ctl.event(toast({ style: SLACK }));
      s.catalogs[0].resolve(styleCatalog());
      await flush();
      expect(s.ctl.view().catalog).toEqual(styleCatalog());
      s.ctl.event(toast({ style: SLACK }));
      expect(s.deps.styleCatalog).toHaveBeenCalledTimes(2);
    });

    it("keeps the latest catalog asked for, and reports a failed fetch", async () => {
      const s = setup();
      s.ctl.event(toast({ style: SLACK }));
      s.ctl.event(toast({ style: SLACK }));
      const newest = styleCatalog({ styles: [MAIL] });
      s.catalogs[1].resolve(newest);
      s.catalogs[0].resolve(styleCatalog());
      await flush();
      expect(s.ctl.view().catalog).toEqual(newest);
      s.ctl.event(toast({ style: SLACK }));
      s.catalogs[2].reject("ipc down");
      await flush();
      expect(s.deps.report).toHaveBeenCalledWith("ipc down");
      expect(s.ctl.view().catalog).toEqual(newest);
    });

    it("fetches the catalog again on a language change once it was needed", async () => {
      const s = setup();
      s.ctl.languageChanged();
      expect(s.deps.styleCatalog).not.toHaveBeenCalled();
      const r = await ready();
      r.ctl.languageChanged();
      const english = styleCatalog({ styles: [{ id: "default", name: "Default" }] });
      r.catalogs.at(-1)!.resolve(english);
      await flush();
      expect(r.ctl.view().catalog).toEqual(english);
    });

    it("opening the picker cancels the auto-dismiss", async () => {
      const s = await ready();
      s.ctl.openPicker();
      expect(s.ctl.view().picker).toBe(true);
      expect(s.timers[0].cancelled).toBe(true);
      s.ctl.hover(true);
      s.ctl.hover(false);
      expect(s.timers).toHaveLength(1);
      expect(s.ctl.view().ov.kind).toBe("toast");
    });

    it("opening the picker under an expired toast clears its pending leave", async () => {
      const s = await ready();
      s.ctl.hover(true);
      s.timers[0].fn();
      s.ctl.openPicker();
      s.ctl.hover(false);
      expect(s.timers).toHaveLength(1);
      expect(s.ctl.view().ov.kind).toBe("toast");
    });

    it("no picker nor restyle without a badge, a catalog or correction", async () => {
      const off = await ready({ correction_enabled: false });
      off.ctl.openPicker();
      await off.ctl.restyle("mail");
      expect(off.ctl.view().picker).toBe(false);
      expect(off.deps.restyle).not.toHaveBeenCalled();

      const noCatalog = setup();
      noCatalog.ctl.event(toast({ style: SLACK }));
      noCatalog.ctl.openPicker();
      await noCatalog.ctl.restyle("mail");
      expect(noCatalog.ctl.view().picker).toBe(false);

      const failed = await ready();
      failed.ctl.event(toast({ level: "error", message: { code: "transcription_failed", error: { code: "timeout" } }, style: SLACK }));
      failed.ctl.openPicker();
      expect(failed.ctl.view().picker).toBe(false);
      failed.ctl.dismiss();
      await failed.ctl.restyle("mail");
      expect(failed.deps.restyle).not.toHaveBeenCalled();
    });

    it("an unknown style does nothing", async () => {
      const s = await ready();
      s.ctl.openPicker();
      await s.ctl.restyle("gone");
      expect(s.deps.restyle).not.toHaveBeenCalled();
      expect(s.ctl.view().picker).toBe(true);
    });

    it("rewriting shows the style asked for until the backend's toast arrives", async () => {
      const s = await ready();
      s.ctl.openPicker();
      const done = s.ctl.restyle("mail");
      expect(s.deps.restyle).toHaveBeenCalledWith(7, "mail");
      expect(s.ctl.view()).toMatchObject({ picker: false, rewriting: MAIL });
      s.resolveRestyle();
      await done;
      expect(s.ctl.view().rewriting).toEqual(MAIL);
      s.ctl.event(toast({ message: { code: "restyled" }, style: MAIL }));
      expect(s.ctl.view()).toMatchObject({ rewriting: null, ov: { message: { code: "restyled" }, style: MAIL } });
      expect(s.timers.at(-1)!.ms).toBe(8000);
    });

    it("a refused restyle turns into an error toast keeping the badge", async () => {
      const s = await ready();
      s.ctl.openPicker();
      const done = s.ctl.restyle("mail");
      s.rejectRestyle({ code: "system", detail: "db locked" });
      await expect(done).resolves.toBeUndefined();
      expect(s.ctl.view()).toMatchObject({
        rewriting: null,
        ov: { kind: "toast", level: "error", message: { code: "restyle_refused", detail: "db locked" }, preview: null, dictation_id: 7, style: SLACK },
      });
      expect(s.timers.at(-1)!.ms).toBe(10000);
    });

    it("a refused restyle does not cover an event that arrived meanwhile, nor reopen a closed toast", async () => {
      const s = await ready();
      s.ctl.openPicker();
      const late = s.ctl.restyle("mail");
      s.ctl.event({ kind: "recording", locked: false, style: null });
      s.rejectRestyle("x");
      await late;
      expect(s.ctl.view().ov.kind).toBe("recording");

      const closed = await ready();
      closed.ctl.openPicker();
      const done = closed.ctl.restyle("mail");
      closed.ctl.dismiss();
      closed.rejectRestyle("x");
      await done;
      expect(closed.ctl.view()).toMatchObject({ ov: { kind: "idle" }, rewriting: null, picker: false });
    });
  });
});
