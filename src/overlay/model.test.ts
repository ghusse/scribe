import { describe, expect, it, vi } from "bun:test";
import type { OverlayEvent } from "../lib/api";
import { barHeight, createOverlay, emptyLevels, hitAreas, markHovered, LEAVE_GRACE_MS, LEVEL_BARS, pushLevel, reduce, toastActions, toastTimeout, type OverlayView } from "./model";

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
    const [pill, warning] = root.querySelectorAll("[data-surface]");
    rect(copy, 10, 5, 40, 24);
    rect(view, 56, 5, 30, 24);
    rect(pill, 0, 0, 100, 34);
    rect(warning, 20, 40, 120, 20);
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
    expect(toastActions({ kind: "idle" })).toEqual({ copy: false, view: false });
    expect(toastActions({ kind: "processing", style: null })).toEqual({ copy: false, view: false });
    expect(toastActions(toast({ dictation_id: null }))).toEqual({ copy: false, view: false });
    expect(toastActions(toast())).toEqual({ copy: true, view: true });
    expect(toastActions(toast({ level: "copied" }))).toEqual({ copy: true, view: true });
    expect(toastActions(toast({ level: "error" }))).toEqual({ copy: false, view: true });
  });

  it("resets the meter on a new recording, keeps it when locking, schedules toasts", () => {
    const view: OverlayView = { ov: { kind: "idle" }, levels: pushLevel(emptyLevels(), 0.1) };
    const rec = reduce(view, { kind: "recording", locked: false, warning: null, style: null });
    expect(rec.view.levels).toEqual(emptyLevels());
    expect(rec.timeout).toBeNull();
    const locked = reduce(view, { kind: "recording", locked: true, warning: null, style: null });
    expect(locked.view.levels).toBe(view.levels);
    expect(locked.view.ov).toEqual({ kind: "recording", locked: true, warning: null, style: null });
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
  const deps = {
    dismiss: vi.fn(() => Promise.resolve()),
    copy: vi.fn(() => new Promise<void>((res, rej) => { resolveCopy = res; rejectCopy = rej; })),
    openHistory: vi.fn(() => Promise.resolve()),
    schedule: vi.fn((fn: () => void, ms: number) => { const t = { fn, ms, cancelled: false }; timers.push(t); return t; }),
    cancel: vi.fn((h: unknown) => { (h as { cancelled: boolean }).cancelled = true; }),
    report: vi.fn(),
  };
  const views: OverlayView[] = [];
  const ctl = createOverlay(deps, (v) => views.push(v));
  return { ctl, deps, views, timers, resolveCopy: () => resolveCopy!(), rejectCopy: (e: unknown) => rejectCopy!(e) };
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
    ctl.event({ kind: "recording", locked: false, warning: null, style: null });
    ctl.level(0.1);
    expect(views.at(-1)!.levels[LEVEL_BARS - 1]).toBeCloseTo(0.6);
    expect(views.at(-1)!.ov).toEqual({ kind: "recording", locked: false, warning: null, style: null });
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
    ctl.event({ kind: "recording", locked: false, warning: null, style: null });
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
    ctl.event({ kind: "recording", locked: false, warning: null, style: null });
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
    s.ctl.event({ kind: "recording", locked: false, warning: null, style: null });
    s.resolveCopy();
    await done;
    expect(s.ctl.view().ov).toEqual({ kind: "recording", locked: false, warning: null, style: null });
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
    expect(toastActions(ov)).toEqual({ copy: false, view: true });
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
});
