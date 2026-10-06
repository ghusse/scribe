import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { calls, commands, resetTauri } from "../../tests/tauri";
import type { PermissionStatus } from "../lib/api";
import Permissions from "./Permissions.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));

let statuses: PermissionStatus[];

beforeEach(() => {
  resetTauri();
  statuses = [
    { permission: "accessibility", state: "not_determined" },
    { permission: "microphone", state: "denied" },
  ];
  commands({
    permissions: () => statuses,
    request_permission: () => statuses,
    open_permission_settings: () => undefined,
  });
});

afterEach(() => {
  vi.useRealTimers();
});

const panel = () => screen.queryByRole("region", { name: "Autorisations manquantes" });

describe("Permissions", () => {
  it("shows nothing when every permission is granted (or on Windows, which asks for none)", async () => {
    statuses = [];
    render(Permissions);
    await vi.waitFor(() => expect(calls("permissions")).toHaveLength(1));
    expect(panel()).toBeNull();
  });

  it("explains each missing permission with its actions", async () => {
    render(Permissions);
    expect(await screen.findByText("Accessibilité")).toBeTruthy();
    expect(screen.getByText("Micro")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Autoriser" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "Ouvrir les Réglages Système" })).toHaveLength(2);
  });

  it("asks the OS, opens its settings, and hides once granted", async () => {
    render(Permissions);
    await fireEvent.click(await screen.findByRole("button", { name: "Autoriser" }));
    expect(calls("request_permission")).toEqual([{ permission: "accessibility" }]);
    await fireEvent.click(screen.getAllByRole("button", { name: "Ouvrir les Réglages Système" })[1]);
    expect(calls("open_permission_settings")).toEqual([{ permission: "microphone" }]);
    statuses = [{ permission: "accessibility", state: "granted" }, { permission: "microphone", state: "granted" }];
    await fireEvent.click(screen.getByRole("button", { name: "Autoriser" }));
    await vi.waitFor(() => expect(panel()).toBeNull());
  });

  it("polls while something is missing, so a grant in System Settings shows up; a failed read keeps the state", async () => {
    vi.useFakeTimers();
    render(Permissions);
    await vi.advanceTimersByTimeAsync(0);
    expect(calls("permissions")).toHaveLength(1);
    commands({ permissions: () => { throw new Error("ipc"); } });
    await vi.advanceTimersByTimeAsync(1500);
    expect(calls("permissions")).toHaveLength(2);
    expect(panel()).not.toBeNull();
    commands({ permissions: () => [] });
    await vi.advanceTimersByTimeAsync(1500);
    expect(panel()).toBeNull();
    await vi.advanceTimersByTimeAsync(3000);
    expect(calls("permissions")).toHaveLength(3);
  });

  it("reports failed actions", async () => {
    render(Permissions);
    await screen.findByText("Accessibilité");
    commands({
      request_permission: () => { throw new Error("refus"); },
      open_permission_settings: () => { throw new Error("open introuvable"); },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Autoriser" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Demande impossible : Error: refus");
    await fireEvent.click(screen.getAllByRole("button", { name: "Ouvrir les Réglages Système" })[0]);
    expect((await screen.findByRole("alert")).textContent).toBe("Ouverture des Réglages impossible : Error: open introuvable");
    expect(panel()).not.toBeNull();
  });
});
