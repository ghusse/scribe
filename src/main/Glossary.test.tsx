import { beforeEach, describe, expect, it } from "bun:test";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/preact";
import type { Term } from "../lib/api";
import { calls, commands, resetTauri } from "../../tests/tauri";
import { lang } from "../lib/i18n";
import Glossary from "./Glossary";

const term = (id: number, t: string, variants: string[], note: string | null = null): Term => ({
  id, term: t, variants, note, source: "manual", use_count: id * 2, last_used_at: null, created_at: "2026-10-01T00:00:00Z",
});

let terms: Term[];

beforeEach(() => {
  resetTauri();
  terms = [term(1, "Kubernetes", ["cube", "ku ber netes"], "infra")];
  commands({
    list_terms: () => terms,
    add_term: (a) => {
      terms = [...terms, term(9, a!.term as string, a!.variants as string[], a!.note as string | null)];
      return 9;
    },
    update_term: () => undefined,
    delete_term: () => undefined,
  });
});

const rows = () => screen.getAllByRole("row").slice(1);
const type = (el: HTMLElement, value: string) => fireEvent.input(el, { target: { value } });

describe("Glossary", () => {
  it("lists the terms with their variants, note and usage count", async () => {
    render(<Glossary />);
    expect(await screen.findByText("Kubernetes")).toBeTruthy();
    const cells = within(rows()[0]).getAllByRole("cell").map((c) => c.textContent);
    expect(cells.slice(0, 4)).toEqual(["Kubernetes", "cube, ku ber netes", "infra", "2"]);
  });

  it("shows the empty state", async () => {
    terms = [];
    render(<Glossary />);
    expect(await screen.findByText(/Glossaire vide/)).toBeTruthy();
  });

  it("shows a load error", async () => {
    commands({ list_terms: () => Promise.reject("base verrouillée") });
    render(<Glossary />);
    expect(await screen.findByText("base verrouillée")).toBeTruthy();
  });

  it("adds a term with split variants and no note, clears the form and reloads", async () => {
    render(<Glossary />);
    await screen.findByText("Kubernetes");
    const termInput = screen.getByPlaceholderText("Terme (ex. Kubernetes)") as HTMLInputElement;
    const variantsInput = screen.getByPlaceholderText(/Mal entendu/) as HTMLInputElement;
    await type(termInput, "Svelte");
    await type(variantsInput, " svelt , , velte");
    await fireEvent.submit(termInput.form!);
    expect(await screen.findByText("Svelte")).toBeTruthy();
    expect(calls("add_term")).toEqual([{ term: "Svelte", variants: ["svelt", "velte"], note: null }]);
    expect(termInput.value).toBe("");
    expect(variantsInput.value).toBe("");
    expect(calls("list_terms")).toHaveLength(2);
  });

  it("keeps the form and shows the error when adding fails, clears it on the next success", async () => {
    commands({ add_term: () => Promise.reject({ code: "duplicate_term", term: "Kubernetes" }) });
    render(<Glossary />);
    await screen.findByText("Kubernetes");
    const termInput = screen.getByPlaceholderText("Terme (ex. Kubernetes)") as HTMLInputElement;
    await type(termInput, "Kubernetes");
    await type(screen.getByPlaceholderText(/Note/), "n");
    await fireEvent.submit(termInput.form!);
    expect(await screen.findByText("terme déjà présent dans le glossaire : Kubernetes")).toBeTruthy();
    expect(termInput.value).toBe("Kubernetes");
    expect(calls("add_term")[0]!.note).toBe("n");

    commands({ add_term: () => 10 });
    await fireEvent.submit(termInput.form!);
    await waitFor(() => expect(screen.queryByText("terme déjà présent dans le glossaire : Kubernetes")).toBeNull());
  });

  it("edits a term in place and saves it", async () => {
    render(<Glossary />);
    await screen.findByText("Kubernetes");
    await fireEvent.click(screen.getByText("Modifier"));
    const [t, v, n] = within(rows()[0]).getAllByRole("textbox") as HTMLInputElement[];
    expect([t.value, v.value, n.value]).toEqual(["Kubernetes", "cube, ku ber netes", "infra"]);
    await type(t, "K8s");
    await type(v, "cube,");
    await type(n, "");
    await fireEvent.click(screen.getByText("OK"));
    await waitFor(() => expect(screen.getByText("Modifier")).toBeTruthy());
    expect(calls("update_term")).toEqual([{ id: 1, term: "K8s", variants: ["cube"], note: null }]);
  });

  it("edits a term without note and cancels", async () => {
    terms = [term(1, "Rust", [])];
    render(<Glossary />);
    await screen.findByText("Rust");
    await fireEvent.click(screen.getByText("Modifier"));
    const [, , n] = within(rows()[0]).getAllByRole("textbox") as HTMLInputElement[];
    expect(n.value).toBe("");
    await fireEvent.click(screen.getByText("Annuler"));
    expect(screen.getByText("Rust")).toBeTruthy();
    expect(calls("update_term")).toHaveLength(0);
  });

  it("keeps the editor open when saving fails", async () => {
    commands({ update_term: () => Promise.reject("terme vide") });
    render(<Glossary />);
    await screen.findByText("Kubernetes");
    await fireEvent.click(screen.getByText("Modifier"));
    await fireEvent.click(screen.getByText("OK"));
    expect(await screen.findByText("terme vide")).toBeTruthy();
    expect(screen.getByText("OK")).toBeTruthy();
  });

  it("deletes a term and reports a failed delete", async () => {
    render(<Glossary />);
    await screen.findByText("Kubernetes");
    commands({ delete_term: () => Promise.reject("introuvable") });
    await fireEvent.click(screen.getByText("Supprimer"));
    expect(await screen.findByText("introuvable")).toBeTruthy();

    commands({ delete_term: () => { terms = []; } });
    await fireEvent.click(screen.getByText("Supprimer"));
    expect(await screen.findByText(/Glossaire vide/)).toBeTruthy();
    expect(screen.queryByText("introuvable")).toBeNull();
    expect(calls("delete_term")).toEqual([{ id: 1 }, { id: 1 }]);
  });

  it("speaks English and re-renders on a language change", async () => {
    lang.value = "en";
    terms = [];
    render(<Glossary />);
    expect(await screen.findByText("Empty glossary: add the proper names and jargon of your fields.")).toBeTruthy();
    expect(screen.getByPlaceholderText("Term (e.g. Kubernetes)")).toBeTruthy();
    expect(screen.getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["Term", "Heard", "Note", "Uses", ""]);
    lang.value = "fr";
    expect(await screen.findByText(/Glossaire vide/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ajouter" })).toBeTruthy();
  });
});
