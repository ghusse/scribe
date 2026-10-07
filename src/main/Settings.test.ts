import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import type { Settings as SettingsT } from "../lib/api";
import type { SaveState } from "../lib/autosave";
import { CAPTURE_TIMEOUT_MESSAGE } from "../lib/hotkeys";
import { calls, commands, invoke, resetTauri } from "../../tests/tauri";
import { PROVIDERS, settings } from "../../tests/fixtures";
import Settings from "./Settings.svelte";

vi.mock("@tauri-apps/api/core", () => import("../../tests/tauri").then((m) => m.coreModule));

let stored: Record<string, boolean>;

beforeEach(() => {
  resetTauri();
  Element.prototype.scrollIntoView = vi.fn();
  stored = { openai: true, anthropic: true };
  commands({
    get_settings: () => settings(),
    providers: () => PROVIDERS,
    key_status: () => ({ ...stored }),
    save_settings: () => undefined,
    set_api_key: (a) => {
      stored[a!.provider as string] = a!.key !== "";
    },
    test_providers: () => ({ stt: { Ok: 412.4 }, llm: { Ok: 300 } }),
    capture_key: () => null,
    cancel_capture: () => undefined,
    get_autostart: () => false,
  });
});

/** Mounts Settings with an observable `bind:status` (see ModelPicker.test.ts) and waits for the form. */
async function setup(extra: { active?: boolean } = {}) {
  let status: SaveState = "saved";
  const props = {
    ...extra,
    get status() {
      return status;
    },
    set status(v: SaveState) {
      status = v;
    },
  };
  const r = render(Settings, { props });
  await screen.findByText("Raccourci");
  return { ...r, props, status: () => status };
}

const saves = () => calls("save_settings").map((a) => a!.settings as SettingsT);
const lastSave = () => saves().at(-1)!;
const bar = () => document.querySelector(".statusbar") as HTMLElement;
const alertText = () => screen.getByRole("alert", { hidden: true }).textContent ?? "";
const select = (label: RegExp) => screen.getByLabelText(label, { selector: "select" }) as HTMLSelectElement;
const keyForm = (label: string) => (screen.getByLabelText(`Clé ${label}`) as HTMLInputElement).closest("form")!;
/** The provider select of the transcription or the correction section. */
const providerSelect = (role: "stt" | "llm") => screen.getAllByLabelText(/^Fournisseur/)[role === "stt" ? 0 : 1] as HTMLSelectElement;
const SAVE_WAIT = { timeout: 2000 };

async function setNumber(label: RegExp, value: string) {
  const input = screen.getByLabelText(label) as HTMLInputElement;
  await fireEvent.change(input, { target: { value } });
  return input;
}

describe("Settings: loading", () => {
  it("shows a loading state, then the form", async () => {
    render(Settings);
    expect(screen.getByText("Chargement…")).toBeTruthy();
    expect(await screen.findByText("Raccourci")).toBeTruthy();
    expect(screen.getByText("Alt droit (AltGr)")).toBeTruthy();
    expect(screen.getByText("Espace")).toBeTruthy();
  });

  it("shows a load error and retries", async () => {
    commands({ get_settings: () => Promise.reject("base verrouillée") });
    render(Settings);
    expect(await screen.findByText(/Impossible de charger les réglages.*base verrouillée/)).toBeTruthy();
    commands({ get_settings: () => settings() });
    await fireEvent.click(screen.getByText("Réessayer"));
    expect(await screen.findByText("Raccourci")).toBeTruthy();
  });

  it("shows settings saved with the old lock checkbox off as « no lock key », without saving", async () => {
    commands({ get_settings: () => settings({ gesture: { ...settings().gesture, lock_key_enabled: false } }) });
    await setup();
    expect(screen.getByText("Aucune")).toBeTruthy();
    expect((screen.getByLabelText("Désactiver la touche de verrouillage") as HTMLButtonElement).disabled).toBe(true);
    await new Promise((r) => setTimeout(r, 600));
    expect(saves()).toHaveLength(0);
  });
});

describe("Settings: autosave", () => {
  it("saves an edit after a pause, shows the progress then a success that fades out", async () => {
    const { status } = await setup();
    expect(bar().classList.contains("hidden")).toBe(true);
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    expect(status()).toBe("pending");
    expect(bar().textContent).toContain("Enregistrement…");
    expect(saves()).toHaveLength(0);
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().gesture.double_tap_enabled).toBe(false);
    await waitFor(() => expect(bar().textContent).toContain("Réglages enregistrés"));
    expect(status()).toBe("saved");
    expect(bar().classList.contains("hidden")).toBe(false);
    await waitFor(() => expect(bar().classList.contains("hidden")).toBe(true), { timeout: 3000 });
  });

  it("saves the « mute during dictation » checkbox", async () => {
    await setup();
    const box = screen.getByLabelText(/Couper le son de l'ordinateur pendant la dictée/) as HTMLInputElement;
    expect(box.checked).toBe(true);
    await fireEvent.click(box);
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().mute_audio_during_dictation).toBe(false);
  });

  it("an edit undone before the save is not saved", async () => {
    const { status } = await setup();
    const box = screen.getByLabelText(/Double-tap/);
    await fireEvent.click(box);
    await fireEvent.click(box);
    expect(status()).toBe("saved");
    await new Promise((r) => setTimeout(r, 600));
    expect(saves()).toHaveLength(0);
  });

  it("rejects an out-of-range number: alert, aria-invalid, nothing sent", async () => {
    const { status } = await setup();
    const input = await setNumber(/Seuil de maintien/, "50");
    await waitFor(() => expect(status()).toBe("invalid"), SAVE_WAIT);
    expect(alertText()).toBe("⚠ Non enregistré\u00a0: Seuil de maintien\u00a0: entre 100 et 2000 ms");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(saves()).toHaveLength(0);
    await setNumber(/Seuil de maintien/, "400");
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().gesture.hold_threshold_ms).toBe(400);
  });

  it("saves every number field, the retention and the max duration in ms", async () => {
    await setup();
    await setNumber(/Fenêtre de double-tap/, "400");
    await setNumber(/Restauration du presse-papier/, "200");
    await setNumber(/Durée max/, "2.5");
    await setNumber(/Délai max. de correction/, "6.5");
    await fireEvent.change(select(/Conservation de l'audio/), { target: { value: "90" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    const s = lastSave();
    expect([s.gesture.double_tap_window_ms, s.restore_delay_ms, s.max_recording_ms, s.audio_retention_days]).toEqual([400, 200, 150000, 90]);
    expect(s.llm_timeout_base_ms).toBe(6500);
  });

  it("a failed save stays visible with its details and can be retried", async () => {
    commands({ save_settings: () => Promise.reject("écriture impossible\nchemin: C:\\x") });
    const { status } = await setup();
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    await waitFor(() => expect(status()).toBe("failed"), SAVE_WAIT);
    expect(alertText()).toBe("⚠ Non enregistré\u00a0: écriture impossible");
    const details = screen.getByRole("button", { name: "Détails" });
    await fireEvent.click(details);
    expect(details.getAttribute("aria-expanded")).toBe("true");
    expect(document.querySelector(".statusbar pre")!.textContent).toBe("écriture impossible\nchemin: C:\\x");
    commands({ save_settings: () => undefined });
    await fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    await waitFor(() => expect(status()).toBe("saved"));
    expect(saves()).toHaveLength(2);
    expect(saves()[1]).toEqual(saves()[0]);
  });

  it("saves edits made while a save is in flight right after it", async () => {
    let finish!: () => void;
    commands({ save_settings: () => new Promise<void>((res) => (finish = res)) });
    await setup();
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    await fireEvent.change(select(/Niveau de correction/), { target: { value: "clean" } });
    await new Promise((r) => setTimeout(r, 600)); // the debounce fires while the first save is in flight
    expect(saves()).toHaveLength(1);
    commands({ save_settings: () => undefined });
    finish();
    await waitFor(() => expect(saves()).toHaveLength(2));
    expect(lastSave().level).toBe("clean");
    expect(lastSave().gesture.double_tap_enabled).toBe(false);
  });

  it("an edit reverted while a save is in flight is settled by that save", async () => {
    let finish!: () => void;
    commands({ save_settings: () => new Promise<void>((res) => (finish = res)) });
    const { status } = await setup();
    const box = screen.getByLabelText(/Double-tap/);
    await fireEvent.click(box);
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    await fireEvent.click(box); // back to the stored value while the first save runs
    await fireEvent.click(box); // and to what is being saved
    finish();
    await waitFor(() => expect(status()).toBe("saved"));
    await new Promise((r) => setTimeout(r, 600));
    expect(saves()).toHaveLength(1);
  });

  it("never drops a pending edit when unmounted", async () => {
    const { unmount } = await setup();
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    unmount();
    await waitFor(() => expect(saves()).toHaveLength(1));
    expect(lastSave().gesture.double_tap_enabled).toBe(false);
  });
});

describe("Settings: providers and models", () => {
  it("switching the transcription provider selects its newest model", async () => {
    await setup();
    await fireEvent.change(providerSelect("stt"), { target: { value: "mistral" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect([lastSave().stt_provider, lastSave().stt_model]).toEqual(["mistral", "voxtral"]);
    expect(screen.getByText(/Aucune clé Mistral/)).toBeTruthy();
  });

  it("switching the correction provider selects its first model and drops a meaningless effort", async () => {
    await setup();
    expect(select(/Réflexion du modèle/).value).toBe("low");
    await fireEvent.change(providerSelect("llm"), { target: { value: "openai" } });
    expect(screen.queryByLabelText(/Réflexion du modèle/)).toBeNull();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect([lastSave().llm_provider, lastSave().llm_model]).toEqual(["openai", "gpt-5"]);
  });

  it("picking another model keeps the effort valid", async () => {
    commands({ get_settings: () => settings({ llm_effort: "max" }) });
    await setup();
    const llmModel = document.querySelector("fieldset .picker select") as HTMLSelectElement;
    await fireEvent.change(llmModel, { target: { value: "claude-haiku-4-5" } });
    expect(screen.queryByLabelText(/Réflexion du modèle/)).toBeNull();
    await fireEvent.change(llmModel, { target: { value: "claude-sonnet-4-5" } });
    expect(select(/Réflexion du modèle/).value).toBe("low");
    await fireEvent.change(select(/Réflexion du modèle/), { target: { value: "high" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().llm_effort).toBe("high");
  });

  it("raw level: correction disabled, its key not required, the test is transcription only", async () => {
    stored = { openai: true };
    await setup();
    expect(screen.getByRole("note").textContent).toMatch(/Anthropic.*\(correction\)/);
    await fireEvent.change(select(/Niveau de correction/), { target: { value: "raw" } });
    expect(screen.getByText("Correction désactivée en mode brut")).toBeTruthy();
    expect((document.querySelector("fieldset") as HTMLFieldSetElement).disabled).toBe(true);
    expect(screen.queryByRole("note")).toBeNull();
    expect(screen.queryByText(/Aucune clé Anthropic/)).toBeNull();
    expect(screen.getByRole("button", { name: "Tester la transcription" })).toBeTruthy();
  });

  it("works with a provider the catalog does not know", async () => {
    commands({ get_settings: () => settings({ stt_provider: "gone", llm_provider: "gone2" }), key_status: () => ({ gone: true, gone2: true }) });
    await setup();
    expect(screen.getByText("✓ Clé gone enregistrée")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/Transcription · gone \/ gpt-4o-transcribe/)).toBeTruthy();
    expect(screen.getByText(/Correction · gone2 \/ claude-sonnet-4-5/)).toBeTruthy();
  });
});

describe("Settings: API keys", () => {
  it("lists the keys in use first, the others folded, with their status", async () => {
    stored = { openai: true };
    await setup();
    const used = [...document.querySelectorAll("section > form.key .label")].map((e) => e.textContent);
    expect(used).toEqual(["OpenAI", "Anthropic"]);
    const others = [...document.querySelectorAll("details.others form.key .label")].map((e) => e.textContent);
    expect(others).toEqual(["Mistral", "Groq"]);
    expect(within(keyForm("OpenAI")).getByText("✓ enregistrée")).toBeTruthy();
    expect(within(keyForm("Anthropic")).getByText("✗ requise")).toBeTruthy();
    expect(within(keyForm("Anthropic")).getByText(/utilisée pour.*Correction/)).toBeTruthy();
    expect(within(keyForm("Mistral")).getByText("— non configurée")).toBeTruthy();
    expect((screen.getByLabelText("Clé OpenAI") as HTMLInputElement).placeholder).toBe("Remplacer la clé…");
    expect((screen.getByLabelText("Clé Anthropic") as HTMLInputElement).placeholder).toBe("Coller la clé API…");
  });

  it("saves a typed key, refreshes the status and confirms briefly", async () => {
    stored = { openai: true };
    await setup();
    const form = keyForm("Anthropic");
    const save = within(form).getByRole("button", { name: "Enregistrer" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    const input = screen.getByLabelText("Clé Anthropic") as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "sk-ant" } });
    expect(save.disabled).toBe(false);
    await fireEvent.submit(form);
    expect(await within(keyForm("Anthropic")).findByText("✓ Clé enregistrée")).toBeTruthy();
    expect(calls("set_api_key")).toEqual([{ provider: "anthropic", key: "sk-ant" }]);
    expect(input.value).toBe("");
    expect(within(keyForm("Anthropic")).getByText("✓ enregistrée")).toBeTruthy();
    expect(screen.queryByRole("note")).toBeNull();
    await waitFor(() => expect(within(keyForm("Anthropic")).queryByText("✓ Clé enregistrée")).toBeNull(), { timeout: 3000 });
  });

  it("never sends an empty key from the form", async () => {
    await setup();
    await fireEvent.input(screen.getByLabelText("Clé OpenAI"), { target: { value: "   " } });
    await fireEvent.submit(keyForm("OpenAI"));
    expect(calls("set_api_key")).toHaveLength(0);
  });

  it("shows why a key could not be saved", async () => {
    commands({ set_api_key: () => Promise.reject("coffre indisponible") });
    await setup();
    await fireEvent.input(screen.getByLabelText("Clé OpenAI"), { target: { value: "sk" } });
    await fireEvent.submit(keyForm("OpenAI"));
    expect(await screen.findByText("coffre indisponible")).toBeTruthy();
    expect((screen.getByLabelText("Clé OpenAI") as HTMLInputElement).value).toBe("sk");
  });

  it("deletes a key only after confirmation", async () => {
    await setup();
    await fireEvent.click(within(keyForm("OpenAI")).getByRole("button", { name: "Supprimer" }));
    expect(screen.getByText("Supprimer la clé OpenAI ?")).toBeTruthy();
    const cancel = screen.getByRole("button", { name: "Annuler" });
    await waitFor(() => expect(document.activeElement).toBe(cancel));
    await fireEvent.click(cancel);
    expect(screen.queryByText("Supprimer la clé OpenAI ?")).toBeNull();
    const del = within(keyForm("OpenAI")).getByRole("button", { name: "Supprimer" });
    await waitFor(() => expect(document.activeElement).toBe(del));
    expect(calls("set_api_key")).toHaveLength(0);

    await fireEvent.click(del);
    const confirm = screen.getByText("Supprimer la clé OpenAI ?").closest("form")!;
    await fireEvent.click(within(confirm).getByRole("button", { name: "Supprimer" }));
    expect(await screen.findByText("✓ Clé supprimée")).toBeTruthy();
    expect(calls("set_api_key")).toEqual([{ provider: "openai", key: "" }]);
    expect(document.activeElement).toBe(screen.getByLabelText("Clé OpenAI"));
    expect(screen.getByRole("note").textContent).toMatch(/OpenAI.*\(transcription\)/);
  });

  it("Échap cancels the delete confirmation, other keys do nothing", async () => {
    await setup();
    await fireEvent.click(within(keyForm("OpenAI")).getByRole("button", { name: "Supprimer" }));
    const confirm = screen.getByText("Supprimer la clé OpenAI ?").closest("form")!;
    const del = within(confirm).getByRole("button", { name: "Supprimer" });
    await fireEvent.keyDown(del, { key: "a" });
    expect(screen.getByText("Supprimer la clé OpenAI ?")).toBeTruthy();
    expect(await fireEvent.keyDown(del, { key: "Escape" })).toBe(false); // default prevented
    await waitFor(() => expect(screen.queryByText("Supprimer la clé OpenAI ?")).toBeNull());
    expect(calls("set_api_key")).toHaveLength(0);
  });

  it("« Ajouter la clé » brings the missing key's field into view, unfolding the others", async () => {
    commands({ get_settings: () => settings({ stt_provider: "mistral" }) });
    stored = { anthropic: true };
    await setup();
    expect(document.querySelector("details.others")!.textContent).not.toContain("Mistral"); // in use: shown first
    await fireEvent.click(screen.getByRole("button", { name: "Ajouter la clé" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Mistral")));
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });

  it("the banner link focuses the key and closes a pending delete confirmation on it", async () => {
    stored = { openai: true };
    await setup();
    await fireEvent.click(within(screen.getByRole("note")).getByRole("button", { name: "Anthropic" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Anthropic")));
  });

  it("names every missing key in the banner", async () => {
    stored = {};
    commands({ get_settings: () => settings({ llm_provider: "mistral", llm_model: "mistral-large" }) });
    await setup();
    expect(screen.getByRole("note").textContent!.replace(/\s+/g, " ")).toBe(
      "Pour commencer, ajoutez la clé OpenAI (transcription) et Mistral (correction).",
    );
  });

  it("focusing a folded provider's key unfolds the others", async () => {
    // A key line for a provider that is not in use cannot be clicked, so go through a provider switch:
    // the inline warning of the new provider points at a row that was folded a moment ago.
    stored = { openai: true, anthropic: true };
    await setup();
    const details = document.querySelector("details.others") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    await fireEvent.click(within(keyForm("OpenAI")).getByRole("button", { name: "Supprimer" }));
    await fireEvent.change(providerSelect("stt"), { target: { value: "groq" } });
    await fireEvent.click(screen.getByRole("button", { name: "Ajouter la clé" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Groq")));
  });
});

describe("Settings: provider test", () => {
  it("shows one line per role with the timing or the translated error", async () => {
    commands({ test_providers: () => ({ stt: { Ok: 412.4 }, llm: { Err: "HTTP 401 Unauthorized" } }) });
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: "Tester transcription et correction" }));
    expect(await screen.findByText(/✓ fonctionne \(412 ms\)/)).toBeTruthy();
    expect(screen.getByText(/✗ clé refusée par Anthropic, vérifiez la clé/)).toBeTruthy();
    expect(screen.getByText("HTTP 401 Unauthorized")).toBeTruthy();
    expect(document.querySelectorAll(".test-result p.ok, .test-result p.fail")).toHaveLength(2);
  });

  it("a raw-level test says correction is not used", async () => {
    commands({ get_settings: () => settings({ level: "raw" }), test_providers: () => ({ stt: { Ok: 10 }, llm: null }) });
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: "Tester la transcription" }));
    expect(await screen.findByText(/non utilisée \(niveau brut\)/)).toBeTruthy();
  });

  it("saves pending edits before testing", async () => {
    await setup();
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await screen.findAllByText(/✓ fonctionne/);
    expect(invoke.mock.calls.map(([c]) => c).filter((c) => c === "save_settings" || c === "test_providers")).toEqual(["save_settings", "test_providers"]);
  });

  it("refuses to test settings that cannot be saved", async () => {
    await setup();
    await setNumber(/Seuil de maintien/, "5");
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/corrigez l'erreur avant de tester/)).toBeTruthy();
    expect(calls("test_providers")).toHaveLength(0);
  });

  it("explains a test that could not run, with the raw error in the details", async () => {
    commands({ test_providers: () => Promise.reject("clé API manquante pour « OpenAI »") });
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/✗ Test impossible.*aucune clé OpenAI enregistrée/)).toBeTruthy();
    expect(screen.getByText("clé API manquante pour « OpenAI »")).toBeTruthy();
  });

  it("shows an untranslatable error as is", async () => {
    commands({ test_providers: () => Promise.reject("panique") });
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText("panique")).toBeTruthy();
    expect(document.querySelector(".test-result details")).toBeNull();
  });

  it("shows the button busy while testing", async () => {
    let done!: (v: unknown) => void;
    commands({ test_providers: () => new Promise((res) => (done = res)) });
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    const busy = (await screen.findByRole("button", { name: "Test en cours…" })) as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
    done({ stt: { Ok: 1 }, llm: { Ok: 1 } });
    expect(await screen.findByRole("button", { name: "Tester transcription et correction" })).toBeTruthy();
  });

  it("a result for other settings is dropped", async () => {
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await screen.findByText(/✓ fonctionne \(412 ms\)/);
    await fireEvent.change(select(/Niveau de correction/), { target: { value: "clean" } });
    expect(screen.queryByText(/✓ fonctionne/)).toBeNull();
  });

  it("a key saved while a test runs makes its result stale", async () => {
    let done!: (v: unknown) => void;
    commands({ test_providers: () => new Promise((res) => (done = res)) });
    await setup();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await waitFor(() => expect(calls("test_providers")).toHaveLength(1));
    await fireEvent.input(screen.getByLabelText("Clé OpenAI"), { target: { value: "sk-new" } });
    await fireEvent.submit(keyForm("OpenAI"));
    await screen.findByText("✓ Clé enregistrée");
    done({ stt: { Ok: 1 }, llm: { Ok: 1 } });
    await screen.findByRole("button", { name: "Tester transcription et correction" });
    expect(screen.queryByText(/✓ fonctionne/)).toBeNull();
  });

  it("asks before testing with a typed but unsaved key, then saves it and tests", async () => {
    await setup();
    await fireEvent.input(screen.getByLabelText("Clé Anthropic"), { target: { value: "sk-ant" } });
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(screen.getByText("Clé Anthropic saisie mais non enregistrée. Enregistrer ?")).toBeTruthy();
    expect(calls("test_providers")).toHaveLength(0);
    const prompt = screen.getByText(/saisie mais non enregistrée/);
    await fireEvent.click(within(prompt).getByRole("button", { name: "Enregistrer" }));
    expect(await screen.findAllByText(/✓ fonctionne/)).toHaveLength(2);
    expect(calls("set_api_key")).toEqual([{ provider: "anthropic", key: "sk-ant" }]);
  });

  it("the unsaved-key question can be dismissed; a failed key save stops the test", async () => {
    commands({ set_api_key: () => Promise.reject("coffre") });
    await setup();
    await fireEvent.input(screen.getByLabelText("Clé Anthropic"), { target: { value: "sk-ant" } });
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await fireEvent.click(within(screen.getByText(/saisie mais non enregistrée/)).getByRole("button", { name: "Annuler" }));
    expect(screen.queryByText(/saisie mais non enregistrée/)).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await fireEvent.click(within(screen.getByText(/saisie mais non enregistrée/)).getByRole("button", { name: "Enregistrer" }));
    expect(await screen.findByText("coffre")).toBeTruthy();
    expect(calls("test_providers")).toHaveLength(0);
  });
});

describe("Settings: hotkeys", () => {
  const change = (role: "déclenchement" | "verrouillage") =>
    screen.getByRole("button", { name: role === "déclenchement" ? "Changer le raccourci de déclenchement" : "Changer la touche de verrouillage" });

  it("captures a new trigger key and saves it", async () => {
    commands({ capture_key: () => [0x70] });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText("F1")).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().trigger_keys).toEqual([0x70]);
  });

  it("shows the capture in progress and can cancel it", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)) });
    await setup();
    await fireEvent.click(change("verrouillage"));
    expect(screen.getByText("Appuyez sur une touche… (Échap pour annuler)")).toBeTruthy();
    expect((change("déclenchement") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByLabelText("Désactiver la touche de verrouillage") as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(screen.getByRole("button", { name: "Annuler la capture" }));
    expect(calls("cancel_capture")).toHaveLength(1);
    answer([0x41]); // a key that arrives after the cancel is ignored
    await waitFor(() => expect(screen.getByText("Espace")).toBeTruthy());
    expect(screen.queryByText(/saisie normale/)).toBeNull();
    await new Promise((r) => setTimeout(r, 600));
    expect(saves()).toHaveLength(0);
  });

  it("a cancel that fails still lets the capture end", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)), cancel_capture: () => Promise.reject("x") });
    await setup();
    await fireEvent.click(change("déclenchement"));
    await fireEvent.click(screen.getByRole("button", { name: "Annuler la capture" }));
    answer(null);
    expect(await screen.findByRole("button", { name: "Changer le raccourci de déclenchement" })).toBeTruthy();
    expect(screen.queryByText(CAPTURE_TIMEOUT_MESSAGE)).toBeNull();
  });

  it("says when no key was pressed in time", async () => {
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(CAPTURE_TIMEOUT_MESSAGE)).toBeTruthy();
  });

  it("says when the capture fails", async () => {
    commands({ capture_key: () => Promise.reject("hook absent") });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/Capture impossible.*hook absent/)).toBeTruthy();
  });

  it("refuses the trigger key as the lock key when there is no lock to swap", async () => {
    commands({ get_settings: () => settings({ lock_vk: 0 }), capture_key: () => [0xa5] });
    await setup();
    await fireEvent.click(change("verrouillage"));
    expect(await screen.findByText(/Alt droit \(AltGr\) est déjà la touche de déclenchement/)).toBeTruthy();
  });

  it("swaps the keys when the trigger takes the lock key", async () => {
    commands({ capture_key: () => [0x14] });
    commands({ get_settings: () => settings({ lock_vk: 0x14 }) });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/Touches échangées/)).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect([lastSave().trigger_keys, lastSave().lock_vk]).toEqual([[0x14], 0xa5]);
  });

  it("asks before making a typing key the trigger", async () => {
    commands({ capture_key: () => [0x41] });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/Ce raccourci gênera la saisie normale/)).toBeTruthy();
    expect(screen.getByText("A")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    expect(screen.queryByText(/saisie normale/)).toBeNull();
    expect(screen.getByText("Alt droit (AltGr)")).toBeTruthy();

    await fireEvent.click(change("déclenchement"));
    await screen.findByText(/saisie normale/);
    await fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().trigger_keys).toEqual([0x41]);
  });

  it("the confirmation also names the lock key when the keys are swapped", async () => {
    commands({ get_settings: () => settings({ lock_vk: 0x41 }), capture_key: () => [0x41] });
    await setup();
    await fireEvent.click(change("déclenchement"));
    const confirm = (await screen.findByText(/saisie normale/)).closest("p")!;
    expect(confirm.textContent).toMatch(/Déclenchement = A, Verrouillage = Alt droit \(AltGr\)\./);
  });

  it("captures a combination, shows it and saves it", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)) });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(screen.getByText("Appuyez sur la touche ou la combinaison, puis relâchez… (Échap pour annuler)")).toBeTruthy();
    answer([0xa2, 0xa0, 0x41]);
    expect(await screen.findByText("Ctrl + Maj + A")).toBeTruthy();
    expect(screen.queryByText(/saisie normale/)).toBeNull();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().trigger_keys).toEqual([0xa2, 0xa0, 0x41]);
  });

  it("Échap during the capture changes nothing", async () => {
    commands({ capture_key: () => [] });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByRole("button", { name: "Changer le raccourci de déclenchement" })).toBeTruthy();
    expect(screen.getByText("Alt droit (AltGr)")).toBeTruthy();
    expect(screen.queryByText(CAPTURE_TIMEOUT_MESSAGE)).toBeNull();
    await new Promise((r) => setTimeout(r, 600));
    expect(saves()).toHaveLength(0);
  });

  it("warns about a combination Windows uses but keeps it", async () => {
    commands({ capture_key: () => [0xa0, 0xa4] });
    await setup();
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/changer la langue du clavier/)).toBeTruthy();
    expect(screen.getByText("Maj + Alt")).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
  });

  it("refuses a combination as the lock key", async () => {
    commands({ capture_key: () => [0xa2, 0x41] });
    await setup();
    await fireEvent.click(change("verrouillage"));
    expect(await screen.findByText("Le verrouillage se fait avec une seule touche.")).toBeTruthy();
    expect(screen.getByText("Espace")).toBeTruthy();
  });

  it("disables the lock key", async () => {
    await setup();
    await fireEvent.click(screen.getByLabelText("Désactiver la touche de verrouillage"));
    expect(screen.getByText("Aucune")).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().lock_vk).toBe(0);
  });

  it("the captured Space/Enter never activates the button being captured", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)) });
    await setup();
    const button = change("déclenchement");
    await fireEvent.click(button);
    const capturing = screen.getByRole("button", { name: "Annuler la capture" });
    expect(await fireEvent.keyDown(capturing, { key: " ", code: "Space" })).toBe(false);
    expect(await fireEvent.keyDown(capturing, { key: "Tab", code: "Tab" })).toBe(true);
    expect(await fireEvent.keyUp(capturing, { key: "Enter", code: "Enter" })).toBe(true); // not the swallowed key
    expect(await fireEvent.keyUp(capturing, { key: " ", code: "Space" })).toBe(false);
    expect(await fireEvent.keyUp(capturing, { key: " ", code: "Space" })).toBe(true); // swallowed once
    answer([0x70]);
    const after = await screen.findByRole("button", { name: "Changer le raccourci de déclenchement" });
    expect(await fireEvent.keyDown(after, { key: "Enter", code: "Enter" })).toBe(false); // just after the capture
    expect(await fireEvent.keyDown(change("verrouillage"), { key: "Enter", code: "Enter" })).toBe(true);
  });

  it("a capture stops when its tab is hidden, and when unmounted", async () => {
    commands({ capture_key: () => new Promise(() => {}) });
    const { rerender, unmount } = await setup();
    await fireEvent.click(change("déclenchement"));
    await rerender({ active: false });
    await waitFor(() => expect(calls("cancel_capture")).toHaveLength(1));
    unmount();
    expect(calls("cancel_capture")).toHaveLength(2);
  });

  it("ignores a second capture while one runs", async () => {
    commands({ capture_key: () => new Promise(() => {}) });
    await setup();
    const button = change("déclenchement");
    await fireEvent.click(button);
    expect(calls("capture_key")).toHaveLength(1);
  });
});
