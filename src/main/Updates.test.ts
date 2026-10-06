import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { calls, commands, resetTauri } from "../../tests/tauri";
import Updates from "./Updates.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));
vi.mock("@tauri-apps/api/event", () => import("../../tests/tauri").then((m) => m.eventModule));

beforeEach(() => {
  resetTauri();
  commands({
    check_update: () => ({ current: "0.1.0", available: null }),
    install_update: () => undefined,
  });
});

const button = (name: RegExp) => screen.getByRole("button", { name }) as HTMLButtonElement;

describe("Updates", () => {
  it("never checks on its own: opening the settings stays offline", () => {
    render(Updates);
    expect(calls("check_update")).toHaveLength(0);
    expect(button(/Rechercher des mises à jour/).disabled).toBe(false);
  });

  it("says when Scribe is up to date", async () => {
    let answer!: (v: unknown) => void;
    commands({ check_update: () => new Promise((r) => (answer = r)) });
    render(Updates);
    await fireEvent.click(button(/Rechercher des mises à jour/));
    expect(button(/Recherche…/).disabled).toBe(true);
    answer({ current: "0.1.0", available: null });
    expect((await screen.findByRole("status")).textContent).toBe("Scribe 0.1.0 est à jour.");
    expect(button(/Rechercher des mises à jour/).disabled).toBe(false);
  });

  it("offers a newer version with its notes and installs it", async () => {
    commands({ check_update: () => ({ current: "0.1.0", available: { version: "0.2.0", notes: "Corrections du collage" } }) });
    render(Updates);
    await fireEvent.click(button(/Rechercher des mises à jour/));
    expect((await screen.findByRole("status")).textContent).toContain("Scribe 0.2.0 est disponible (version actuelle : 0.1.0).");
    expect(screen.getByText("Corrections du collage")).toBeTruthy();
    let finish!: () => void;
    commands({ install_update: () => new Promise<void>((r) => (finish = r)) });
    await fireEvent.click(button(/Installer et redémarrer/));
    expect(calls("install_update")).toHaveLength(1);
    expect(button(/Installation… Scribe va redémarrer/).disabled).toBe(true);
    expect(button(/Rechercher des mises à jour/).disabled).toBe(true);
    finish();
  });

  it("shows a release without notes", async () => {
    commands({ check_update: () => ({ current: "0.1.0", available: { version: "0.2.0", notes: null } }) });
    const { container } = render(Updates);
    await fireEvent.click(button(/Rechercher des mises à jour/));
    await screen.findByRole("button", { name: /Installer et redémarrer/ });
    expect(container.querySelector(".notes")).toBeNull();
  });

  it("reports a failed check or install and lets the user try again", async () => {
    commands({ check_update: () => Promise.reject("réseau indisponible") });
    render(Updates);
    await fireEvent.click(button(/Rechercher des mises à jour/));
    expect((await screen.findByRole("alert")).textContent).toBe("Vérification impossible : réseau indisponible");
    commands({
      check_update: () => ({ current: "0.1.0", available: { version: "0.2.0", notes: null } }),
      install_update: () => Promise.reject("signature invalide"),
    });
    await fireEvent.click(button(/Rechercher des mises à jour/));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    await fireEvent.click(await screen.findByRole("button", { name: /Installer et redémarrer/ }));
    expect((await screen.findByRole("alert")).textContent).toBe("Installation impossible : signature invalide");
    expect(button(/Installer et redémarrer/).disabled).toBe(false);
  });
});
