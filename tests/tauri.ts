// Test doubles for the two Tauri modules the UI uses. In a component test:
//   vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
//   vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));
// then `commands({...})` answers invoke() per command and `emit(name, payload)` fires listen() handlers.
import { vi } from "vitest";

type Handler = (args: Record<string, unknown> | undefined) => unknown;

let routes: Record<string, Handler> = {};

/** Every invoke() call, in order: `[command, args]`. */
export const invoke = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
  const route = routes[cmd];
  if (!route) throw new Error(`unexpected command ${cmd}`);
  return route(args);
});

/** Answers each command with its handler (a value is returned, a thrown error or rejected promise rejects). */
export function commands(r: Record<string, Handler>) {
  routes = { ...routes, ...r };
}

const handlers = new Map<string, Set<(e: { event: string; id: number; payload: unknown }) => void>>();

export const listen = vi.fn(async (name: string, h: (e: { event: string; id: number; payload: unknown }) => void) => {
  if (!handlers.has(name)) handlers.set(name, new Set());
  handlers.get(name)!.add(h);
  return () => {
    handlers.get(name)!.delete(h);
  };
});

/** Fires `name` to its current listeners; returns how many received it. */
export function emit(name: string, payload: unknown = null): number {
  const hs = [...(handlers.get(name) ?? [])];
  for (const h of hs) h({ event: name, id: 0, payload });
  return hs.length;
}

/** Calls of one command, as their args. */
export function calls(cmd: string): (Record<string, unknown> | undefined)[] {
  return invoke.mock.calls.filter(([c]) => c === cmd).map(([, a]) => a);
}

export function resetTauri() {
  routes = {};
  handlers.clear();
  invoke.mockClear();
  listen.mockClear();
}

export const coreModule = { invoke };
export const eventModule = { listen };
