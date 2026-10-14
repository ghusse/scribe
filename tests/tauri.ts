// Test doubles for the two Tauri modules the UI uses, installed for every test by tests/setup.ts (and reset before
// each one): `commands({...})` answers invoke() per command and `emit(name, payload)` fires listen() handlers.
import { vi } from "bun:test";

type Handler = (args: Record<string, unknown> | undefined) => unknown;

let routes: Record<string, Handler> = {};

async function route(cmd: string, args?: Record<string, unknown>) {
  const handler = routes[cmd];
  if (!handler) throw new Error(`unexpected command ${cmd}`);
  return handler(args);
}

/** Every invoke() call, in order: `[command, args]`. */
export const invoke = vi.fn(route);

/** Answers each command with its handler (a value is returned, a thrown error or rejected promise rejects). */
export function commands(r: Record<string, Handler>) {
  routes = { ...routes, ...r };
}

const handlers = new Map<string, Set<(e: { event: string; id: number; payload: unknown }) => void>>();

async function subscribe(name: string, h: (e: { event: string; id: number; payload: unknown }) => void) {
  if (!handlers.has(name)) handlers.set(name, new Set());
  handlers.get(name)!.add(h);
  return () => {
    handlers.get(name)?.delete(h);
  };
}

export const listen = vi.fn(subscribe);

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
  // Implementations too: a test overriding them (mockImplementation) would leak into the next files of the process.
  invoke.mockReset().mockImplementation(route);
  listen.mockReset().mockImplementation(subscribe);
}

export const coreModule = { invoke };
export const eventModule = { listen };
