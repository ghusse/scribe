import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { calls, commands, resetTauri } from "../../tests/tauri";
import Startup from "./Startup.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));

let osState: boolean;

beforeEach(() => {
  resetTauri();
  osState = false;
  commands({
    get_autostart: () => osState,
    set_autostart: (a) => {
      osState = a!.enabled as boolean;
      return osState;
    },
  });
});

const box = () => screen.getByRole("checkbox", { name: /Lancer Scribe à l'ouverture de session/ }) as HTMLInputElement;

describe("Startup", () => {
  it("shows the state in place in the OS, disabled until it is known", async () => {
    let answer!: (v: boolean) => void;
    commands({ get_autostart: () => new Promise<boolean>((r) => (answer = r)) });
    render(Startup);
    expect(box().disabled).toBe(true);
    answer(true);
    await waitFor(() => expect(box().disabled).toBe(false));
    expect(box().checked).toBe(true);
  });

  it("switches launch at login on and off", async () => {
    render(Startup);
    await waitFor(() => expect(box().disabled).toBe(false));
    await fireEvent.click(box());
    await waitFor(() => expect(calls("set_autostart")).toEqual([{ enabled: true }]));
    await waitFor(() => expect(box().checked).toBe(true));
    await fireEvent.click(box());
    await waitFor(() => expect(calls("set_autostart")).toEqual([{ enabled: true }, { enabled: false }]));
    expect(osState).toBe(false);
    expect(box().checked).toBe(false);
  });

  it("keeps the box on the real state when switching fails, and says why", async () => {
    commands({ set_autostart: () => Promise.reject("accès au registre refusé") });
    render(Startup);
    await waitFor(() => expect(box().disabled).toBe(false));
    await fireEvent.click(box());
    expect((await screen.findByRole("alert")).textContent).toBe("Démarrage automatique indisponible : accès au registre refusé");
    expect(box().checked).toBe(false);
    expect(box().disabled).toBe(false);
    commands({ set_autostart: () => true });
    await fireEvent.click(box());
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(box().checked).toBe(true);
  });

  it("reports a state that cannot be read and leaves the box disabled", async () => {
    commands({ get_autostart: () => Promise.reject("plugin indisponible") });
    render(Startup);
    expect((await screen.findByRole("alert")).textContent).toContain("plugin indisponible");
    expect(box().disabled).toBe(true);
  });
});
