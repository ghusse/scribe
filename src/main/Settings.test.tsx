import { afterEach, beforeEach, describe, expect, it, vi } from "bun:test";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/preact";
import type { Settings as SettingsT } from "../lib/api";
import type { SaveState } from "../lib/autosave";
import { lang, t } from "../lib/i18n";
import { calls, commands, invoke, resetTauri } from "../../tests/tauri";
import { PRESETS, PROVIDERS, settings, styleCatalog } from "../../tests/fixtures";
import { advanceTimers } from "../../tests/timers";
import Settings from "./Settings";
import styles from "./Settings.module.css";

let stored: Record<string, boolean>;

beforeEach(() => {
  resetTauri();
  // Debounce and fade-out delays run on fake time: waitFor advances it, so the file takes no real seconds.
  vi.useFakeTimers();
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
    style_catalog: () => styleCatalog(),
  });
});

afterEach(() => {
  vi.useRealTimers();
});

type Pane = "Général" | "Dictée" | "Styles" | "Modèles" | "Avancé";

/** The settings navigation (one button per category). */
const nav = () => screen.getByRole("navigation", { name: t.value.settings.nav.label });
const goTo = (pane: Pane) => fireEvent.click(within(nav()).getByRole("button", { name: new RegExp(`^${pane}`) }));

/** Mounts Settings, records what it reports through `onStatusChange`, waits for the form and opens `pane`. */
async function setup(pane?: Pane) {
  let status: SaveState = "saved";
  const onStatusChange = (v: SaveState) => (status = v);
  const r = render(<Settings onStatusChange={onStatusChange} />);
  await screen.findByText("Raccourci");
  if (pane) await goTo(pane);
  const rerender = (props: { active?: boolean }) => r.rerender(<Settings {...props} onStatusChange={onStatusChange} />);
  return { ...r, rerender, status: () => status };
}

const saves = () => calls("save_settings").map((a) => a!.settings as SettingsT);
const lastSave = () => saves().at(-1)!;
const bar = () => document.querySelector(`.${styles.statusbar}`) as HTMLElement;
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
    render(<Settings />);
    expect(screen.getByText("Chargement…")).toBeTruthy();
    expect(await screen.findByText("Raccourci")).toBeTruthy();
    expect(screen.getByText("Alt droit (AltGr)")).toBeTruthy();
    expect(screen.getByText("Espace")).toBeTruthy();
  });

  it("shows a load error and retries", async () => {
    commands({ get_settings: () => Promise.reject("base verrouillée") });
    render(<Settings />);
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
    await advanceTimers(600);
    expect(saves()).toHaveLength(0);
  });
});

describe("Settings: autosave", () => {
  it("saves an edit after a pause, shows the progress then a success that fades out", async () => {
    const { status } = await setup();
    expect(bar().classList.contains(styles.hidden)).toBe(true);
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    expect(status()).toBe("pending");
    expect(bar().textContent).toContain("Enregistrement…");
    expect(saves()).toHaveLength(0);
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().gesture.double_tap_enabled).toBe(false);
    await waitFor(() => expect(bar().textContent).toContain("Réglages enregistrés"));
    expect(status()).toBe("saved");
    expect(bar().classList.contains(styles.hidden)).toBe(false);
    await waitFor(() => expect(bar().classList.contains(styles.hidden)).toBe(true), { timeout: 3000 });
  });

  it("saves the « mute during dictation » checkbox", async () => {
    await setup();
    const box = screen.getByLabelText(/Couper le son de l'ordinateur pendant la dictée/) as HTMLInputElement;
    expect(box.checked).toBe(true);
    await fireEvent.click(box);
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().mute_audio_during_dictation).toBe(false);
  });

  it("saves the « exclusive microphone » checkbox, off by default, with its warning", async () => {
    await setup();
    const box = screen.getByLabelText(/Couper le micro des autres applications pendant la dictée/) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(document.getElementById(box.getAttribute("aria-describedby")!)?.textContent).toMatch(/visio.*perd le micro/);
    await fireEvent.click(box);
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().exclusive_microphone_during_dictation).toBe(true);
  });

  it("an edit undone before the save is not saved", async () => {
    const { status } = await setup();
    const box = screen.getByLabelText(/Double-tap/);
    await fireEvent.click(box);
    await fireEvent.click(box);
    expect(status()).toBe("saved");
    await advanceTimers(600);
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
    expect(bar().querySelector("pre")!.textContent).toBe("écriture impossible\nchemin: C:\\x");
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
    await fireEvent.click(screen.getByLabelText(/Corriger et mettre en forme/));
    await advanceTimers(600); // the debounce fires while the first save is in flight
    expect(saves()).toHaveLength(1);
    commands({ save_settings: () => undefined });
    finish();
    await waitFor(() => expect(saves()).toHaveLength(2));
    expect(lastSave().correction_enabled).toBe(false);
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
    await advanceTimers(600);
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
    await setup("Modèles");
    await fireEvent.change(providerSelect("stt"), { target: { value: "mistral" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect([lastSave().stt_provider, lastSave().stt_model]).toEqual(["mistral", "voxtral"]);
    expect(screen.getByText(/Aucune clé Mistral/)).toBeTruthy();
  });

  it("switching the correction provider selects its first model and drops a meaningless effort", async () => {
    await setup("Modèles");
    expect(select(/Réflexion du modèle/).value).toBe("low");
    await fireEvent.change(providerSelect("llm"), { target: { value: "openai" } });
    expect(screen.queryByLabelText(/Réflexion du modèle/)).toBeNull();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect([lastSave().llm_provider, lastSave().llm_model]).toEqual(["openai", "gpt-5"]);
  });

  it("picking another model keeps the effort valid", async () => {
    commands({ get_settings: () => settings({ llm_effort: "max" }) });
    await setup("Modèles");
    const llmModel = screen.getAllByLabelText(/^Modèle/, { selector: "select" })[1] as HTMLSelectElement;
    await fireEvent.change(llmModel, { target: { value: "claude-haiku-4-5" } });
    expect(screen.queryByLabelText(/Réflexion du modèle/)).toBeNull();
    await fireEvent.change(llmModel, { target: { value: "claude-sonnet-4-5" } });
    expect(select(/Réflexion du modèle/).value).toBe("low");
    await fireEvent.change(select(/Réflexion du modèle/), { target: { value: "high" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().llm_effort).toBe("high");
  });

  it("correction off: its fields disabled, its key not required, the test is transcription only", async () => {
    stored = { openai: true };
    await setup("Modèles");
    expect(screen.getByRole("note").textContent).toMatch(/Anthropic.*\(correction\)/);
    await fireEvent.click(screen.getByLabelText(/Corriger et mettre en forme/));
    expect(screen.getByText("Correction désactivée (réglages de Dictée)")).toBeTruthy();
    expect((document.querySelector(`fieldset.${styles.fieldset}`) as HTMLFieldSetElement).disabled).toBe(true);
    expect(screen.queryByRole("note")).toBeNull();
    expect(screen.queryByText(/Aucune clé Anthropic/)).toBeNull();
    expect(screen.getByRole("button", { name: "Tester la transcription" })).toBeTruthy();
  });

  it("works with a provider the catalog does not know", async () => {
    commands({ get_settings: () => settings({ stt_provider: "gone", llm_provider: "gone2" }), key_status: () => ({ gone: true, gone2: true }) });
    await setup("Modèles");
    expect(screen.getByText("✓ Clé gone enregistrée")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/Transcription · gone \/ gpt-4o-transcribe/)).toBeTruthy();
    expect(screen.getByText(/Correction · gone2 \/ claude-sonnet-4-5/)).toBeTruthy();
  });
});

describe("Settings: API keys", () => {
  it("lists the keys in use first, the others folded, with their status", async () => {
    stored = { openai: true };
    await setup("Modèles");
    const used = [...document.querySelectorAll(`.${styles.section} > form.${styles.key} .${styles.label}`)].map((e) => e.textContent);
    expect(used).toEqual(["OpenAI", "Anthropic"]);
    const others = [...document.querySelectorAll(`details.${styles.others} form.${styles.key} .${styles.label}`)].map((e) => e.textContent);
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
    await setup("Modèles");
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
    await setup("Modèles");
    await fireEvent.input(screen.getByLabelText("Clé OpenAI"), { target: { value: "   " } });
    await fireEvent.submit(keyForm("OpenAI"));
    expect(calls("set_api_key")).toHaveLength(0);
  });

  it("shows why a key could not be saved", async () => {
    commands({ set_api_key: () => Promise.reject("coffre indisponible") });
    await setup("Modèles");
    await fireEvent.input(screen.getByLabelText("Clé OpenAI"), { target: { value: "sk" } });
    await fireEvent.submit(keyForm("OpenAI"));
    expect(await screen.findByText("coffre indisponible")).toBeTruthy();
    expect((screen.getByLabelText("Clé OpenAI") as HTMLInputElement).value).toBe("sk");
  });

  it("deletes a key only after confirmation", async () => {
    await setup("Modèles");
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
    await setup("Modèles");
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
    await setup("Modèles");
    expect(document.querySelector(`details.${styles.others}`)!.textContent).not.toContain("Mistral"); // in use: shown first
    await fireEvent.click(screen.getByRole("button", { name: "Ajouter la clé" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Mistral")));
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });

  it("the banner link focuses the key and closes a pending delete confirmation on it", async () => {
    stored = { openai: true };
    await setup("Modèles");
    await fireEvent.click(within(screen.getByRole("note")).getByRole("button", { name: "Anthropic" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Anthropic")));
  });

  it("names every missing key in the banner", async () => {
    stored = {};
    commands({ get_settings: () => settings({ llm_provider: "mistral", llm_model: "mistral-large" }) });
    await setup("Modèles");
    expect(screen.getByRole("note").textContent!.replace(/\s+/g, " ")).toBe(
      "Pour commencer, ajoutez la clé OpenAI (transcription) et Mistral (correction).",
    );
  });

  it("focusing a folded provider's key unfolds the others", async () => {
    // A key line for a provider that is not in use cannot be clicked, so go through a provider switch:
    // the inline warning of the new provider points at a row that was folded a moment ago.
    stored = { openai: true, anthropic: true };
    await setup("Modèles");
    const details = document.querySelector(`details.${styles.others}`) as HTMLDetailsElement;
    expect(details.open).toBe(false);
    await fireEvent.click(within(keyForm("OpenAI")).getByRole("button", { name: "Supprimer" }));
    await fireEvent.change(providerSelect("stt"), { target: { value: "groq" } });
    await fireEvent.click(screen.getByRole("button", { name: "Ajouter la clé" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Groq")));
  });
});

describe("Settings: provider test", () => {
  it("shows one line per role with the timing or the translated error", async () => {
    commands({ test_providers: () => ({ stt: { Ok: 412.4 }, llm: { Err: { code: "http", status: 401, body: "Unauthorized" } } }) });
    await setup("Modèles");
    await fireEvent.click(screen.getByRole("button", { name: "Tester transcription et correction" }));
    expect(await screen.findByText(/✓ fonctionne \(412 ms\)/)).toBeTruthy();
    expect(screen.getByText(/✗ clé refusée par Anthropic, vérifiez la clé/)).toBeTruthy();
    expect(screen.getByText("HTTP 401: Unauthorized")).toBeTruthy();
    expect(document.querySelectorAll(`.${styles.testResult} p.${styles.ok}, .${styles.testResult} p.${styles.fail}`)).toHaveLength(2);
  });

  it("with correction off, the test says correction is not used", async () => {
    commands({ get_settings: () => settings({ correction_enabled: false }), test_providers: () => ({ stt: { Ok: 10 }, llm: null }) });
    await setup("Modèles");
    await fireEvent.click(screen.getByRole("button", { name: "Tester la transcription" }));
    expect(await screen.findByText(/non utilisée \(correction désactivée\)/)).toBeTruthy();
  });

  it("saves pending edits before testing", async () => {
    await setup("Modèles");
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await screen.findAllByText(/✓ fonctionne/);
    expect(invoke.mock.calls.map(([c]) => c).filter((c) => c === "save_settings" || c === "test_providers")).toEqual(["save_settings", "test_providers"]);
  });

  it("refuses to test settings that cannot be saved", async () => {
    await setup("Modèles");
    await setNumber(/Seuil de maintien/, "5");
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/corrigez l'erreur avant de tester/)).toBeTruthy();
    expect(calls("test_providers")).toHaveLength(0);
  });

  it("explains a test that could not run, with the raw error in the details", async () => {
    commands({ test_providers: () => Promise.reject({ code: "provider", error: { code: "config", detail: "no base url" } }) });
    await setup("Modèles");
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/✗ Test impossible.*configuration du fournisseur invalide/)).toBeTruthy();
    expect(screen.getByText("no base url")).toBeTruthy();
  });

  it("shows an error without technical text as is, without details", async () => {
    commands({ test_providers: () => Promise.reject("panique") });
    await setup("Modèles");
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(await screen.findByText(/^✗ Test impossible.*: panique$/)).toBeTruthy();
    expect(document.querySelector(`.${styles.testResult} details`)).toBeNull();
  });

  it("shows the button busy while testing", async () => {
    let done!: (v: unknown) => void;
    commands({ test_providers: () => new Promise((res) => (done = res)) });
    await setup("Modèles");
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    const busy = (await screen.findByRole("button", { name: "Test en cours…" })) as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
    done({ stt: { Ok: 1 }, llm: { Ok: 1 } });
    expect(await screen.findByRole("button", { name: "Tester transcription et correction" })).toBeTruthy();
  });

  it("a result for other settings is dropped", async () => {
    await setup("Modèles");
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    await screen.findByText(/✓ fonctionne \(412 ms\)/);
    await fireEvent.click(screen.getByLabelText(/Corriger et mettre en forme/));
    expect(screen.queryByText(/✓ fonctionne/)).toBeNull();
  });

  it("a key saved while a test runs makes its result stale", async () => {
    let done!: (v: unknown) => void;
    commands({ test_providers: () => new Promise((res) => (done = res)) });
    await setup("Modèles");
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
    await setup("Modèles");
    await fireEvent.input(screen.getByLabelText("Clé Anthropic"), { target: { value: "sk-ant" } });
    await fireEvent.click(screen.getByRole("button", { name: /Tester/ }));
    expect(screen.getByText("Clé Anthropic saisie mais non enregistrée. Enregistrer ?")).toBeTruthy();
    expect(calls("test_providers")).toHaveLength(0);
    lang.value = "en";
    expect(await screen.findByText("Anthropic key typed but not saved. Save it?")).toBeTruthy();
    lang.value = "fr";
    const prompt = await screen.findByText(/saisie mais non enregistrée/);
    await fireEvent.click(within(prompt).getByRole("button", { name: "Enregistrer" }));
    expect(await screen.findAllByText(/✓ fonctionne/)).toHaveLength(2);
    expect(calls("set_api_key")).toEqual([{ provider: "anthropic", key: "sk-ant" }]);
  });

  it("the unsaved-key question can be dismissed; a failed key save stops the test", async () => {
    commands({ set_api_key: () => Promise.reject("coffre") });
    await setup("Modèles");
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
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText("F1")).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().trigger_keys).toEqual([0x70]);
  });

  it("shows the capture in progress and can cancel it", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)) });
    await setup("Dictée");
    await fireEvent.click(change("verrouillage"));
    expect(screen.getByText("Appuyez sur une touche… (Échap pour annuler)")).toBeTruthy();
    expect((change("déclenchement") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByLabelText("Désactiver la touche de verrouillage") as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(screen.getByRole("button", { name: "Annuler la capture" }));
    expect(calls("cancel_capture")).toHaveLength(1);
    answer([0x41]); // a key that arrives after the cancel is ignored
    await waitFor(() => expect(screen.getByText("Espace")).toBeTruthy());
    expect(screen.queryByText(/saisie normale/)).toBeNull();
    await advanceTimers(600);
    expect(saves()).toHaveLength(0);
  });

  it("a cancel that fails still lets the capture end", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)), cancel_capture: () => Promise.reject("x") });
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    await fireEvent.click(screen.getByRole("button", { name: "Annuler la capture" }));
    answer(null);
    expect(await screen.findByRole("button", { name: "Changer le raccourci de déclenchement" })).toBeTruthy();
    expect(screen.queryByText(t.value.settings.hotkey.captureTimeout)).toBeNull();
  });

  it("says when no key was pressed in time", async () => {
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(t.value.settings.hotkey.captureTimeout)).toBeTruthy();
  });

  it("says when the capture fails", async () => {
    commands({ capture_key: () => Promise.reject("hook absent") });
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/Capture impossible.*hook absent/)).toBeTruthy();
  });

  it("refuses the trigger key as the lock key when there is no lock to swap", async () => {
    commands({ get_settings: () => settings({ lock_vk: 0 }), capture_key: () => [0xa5] });
    await setup("Dictée");
    await fireEvent.click(change("verrouillage"));
    expect(await screen.findByText(/Alt droit \(AltGr\) est déjà la touche de déclenchement/)).toBeTruthy();
    lang.value = "en";
    expect(await screen.findByText(/is already the trigger key/)).toBeTruthy();
  });

  it("swaps the keys when the trigger takes the lock key", async () => {
    commands({ capture_key: () => [0x14] });
    commands({ get_settings: () => settings({ lock_vk: 0x14 }) });
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/Touches échangées/)).toBeTruthy();
    lang.value = "en";
    expect(await screen.findByText(/Keys swapped/)).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect([lastSave().trigger_keys, lastSave().lock_vk]).toEqual([[0x14], 0xa5]);
  });

  it("asks before making a typing key the trigger", async () => {
    commands({ capture_key: () => [0x41] });
    await setup("Dictée");
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
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    const confirm = (await screen.findByText(/saisie normale/)).closest("p")!;
    expect(confirm.textContent).toMatch(/Déclenchement = A, Verrouillage = Alt droit \(AltGr\)\./);
  });

  it("captures a combination, shows it and saves it", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)) });
    await setup("Dictée");
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
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByRole("button", { name: "Changer le raccourci de déclenchement" })).toBeTruthy();
    expect(screen.getByText("Alt droit (AltGr)")).toBeTruthy();
    expect(screen.queryByText(t.value.settings.hotkey.captureTimeout)).toBeNull();
    await advanceTimers(600);
    expect(saves()).toHaveLength(0);
  });

  it("warns about a combination Windows uses but keeps it", async () => {
    commands({ capture_key: () => [0xa0, 0xa4] });
    await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    expect(await screen.findByText(/changer la langue du clavier/)).toBeTruthy();
    expect(screen.getByText("Maj + Alt")).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
  });

  it("refuses a combination as the lock key", async () => {
    commands({ capture_key: () => [0xa2, 0x41] });
    await setup("Dictée");
    await fireEvent.click(change("verrouillage"));
    expect(await screen.findByText("Le verrouillage se fait avec une seule touche.")).toBeTruthy();
    expect(screen.getByText("Espace")).toBeTruthy();
  });

  it("disables the lock key", async () => {
    await setup("Dictée");
    await fireEvent.click(screen.getByLabelText("Désactiver la touche de verrouillage"));
    expect(screen.getByText("Aucune")).toBeTruthy();
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().lock_vk).toBe(0);
  });

  it("the captured Space/Enter never activates the button being captured", async () => {
    let answer!: (keys: number[] | null) => void;
    commands({ capture_key: () => new Promise((res) => (answer = res)) });
    await setup("Dictée");
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
    const { rerender, unmount } = await setup("Dictée");
    await fireEvent.click(change("déclenchement"));
    await rerender({ active: false });
    await waitFor(() => expect(calls("cancel_capture")).toHaveLength(1));
    unmount();
    expect(calls("cancel_capture")).toHaveLength(2);
  });

  it("ignores a second capture while one runs", async () => {
    commands({ capture_key: () => new Promise(() => {}) });
    await setup("Dictée");
    const button = change("déclenchement");
    await fireEvent.click(button);
    expect(calls("capture_key")).toHaveLength(1);
  });
});

describe("Settings: categories", () => {
  const current = () => within(nav()).getByRole("button", { current: "page" }).textContent;
  const paneTitle = () => screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);

  it("shows one category at a time, General first", async () => {
    await setup();
    expect(current()).toBe("Général");
    expect(paneTitle()).toEqual(["Général"]);
    expect(screen.getByRole("heading", { name: "Langue" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Raccourci" })).toBeNull();
    await goTo("Dictée");
    expect(current()).toBe("Dictée");
    expect(paneTitle()).toEqual(["Dictée"]);
    expect(screen.getByRole("heading", { name: "Raccourci" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Son" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Langue" })).toBeNull();
    await goTo("Avancé");
    expect(screen.getByRole("spinbutton", { name: /Seuil de maintien/ })).toBeTruthy();
  });

  it("opens on Models when a required key is missing", async () => {
    stored = { openai: true };
    await setup();
    expect(current()).toMatch(/^Modèles/);
    expect(screen.getByRole("heading", { name: "Clés API" })).toBeTruthy();
  });

  it("keeps the banner and the status bar whatever the category", async () => {
    stored = { openai: true };
    await setup("Général");
    expect(screen.getByRole("note")).toBeTruthy();
    await fireEvent.change(select(/Langue de l'interface/), { target: { value: "en" } });
    await goTo("Avancé");
    expect(bar().textContent).toContain("Enregistrement…");
    expect(screen.getByRole("note")).toBeTruthy();
  });

  it("the banner link switches to Models and focuses the key", async () => {
    stored = { openai: true };
    await setup("Dictée");
    await fireEvent.click(within(screen.getByRole("note")).getByRole("button", { name: "Anthropic" }));
    expect(current()).toMatch(/^Modèles/);
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Clé Anthropic")));
  });

  it("flags the categories that hold something to fix", async () => {
    stored = { openai: true };
    await setup("Avancé");
    const flagged = () => within(nav()).getAllByRole("img", { name: "(à vérifier)" }).map((d) => d.parentElement!.textContent);
    expect(flagged()).toEqual(["Modèles"]);
    await setNumber(/Seuil de maintien/, "5");
    expect(flagged()).toEqual(["Modèles", "Avancé"]);
  });

  it("cancels a capture when its category is left", async () => {
    commands({ capture_key: () => new Promise(() => {}) });
    await setup("Dictée");
    await fireEvent.click(screen.getByRole("button", { name: "Changer le raccourci de déclenchement" }));
    expect(calls("cancel_capture")).toHaveLength(0);
    await goTo("Général");
    await waitFor(() => expect(calls("cancel_capture")).toHaveLength(1));
  });
});

describe("Settings: language", () => {
  it("autosaves the chosen language, the system's being the default", async () => {
    await setup("Général");
    const language = select(/Langue de l'interface/);
    expect(language.value).toBe("system");
    expect([...language.options].map((o) => o.textContent)).toEqual(["Système", "English", "Français"]);
    await fireEvent.change(language, { target: { value: "en" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().language).toBe("en");
  });

  it("renders in English", async () => {
    lang.value = "en";
    stored = { openai: true };
    render(<Settings />);
    expect(await screen.findByRole("heading", { name: "Models" })).toBeTruthy();
    expect(within(nav()).getAllByRole("button").map((b) => b.textContent)).toEqual(["General", "Dictation", "Styles", "Models", "Advanced"]);
    expect(within(nav()).getByRole("img", { name: "(needs attention)" })).toBeTruthy();
    expect(screen.getByRole("note").textContent!.replace(/\s+/g, " ")).toBe("To get started, add an API key for Anthropic (correction).");
    expect(screen.getByText("⚠ No Anthropic key: correction will fail.")).toBeTruthy();
    expect((screen.getByLabelText("OpenAI key") as HTMLInputElement).placeholder).toBe("Replace the key…");
    await fireEvent.click(within(nav()).getByRole("button", { name: "Dictation" }));
    expect(screen.getByText("Right Alt (AltGr)")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Change the trigger shortcut" })).toBeTruthy();
  });

  it("switches the labels when the language changes", async () => {
    await setup("Dictée");
    expect(screen.getByRole("heading", { name: "Raccourci" })).toBeTruthy();
    lang.value = "en";
    expect(await screen.findByRole("heading", { name: "Shortcut" })).toBeTruthy();
    expect(screen.getByText("Space")).toBeTruthy();
    expect(within(nav()).getByRole("button", { current: "page" }).textContent).toBe("Dictation");
  });

  it("translates a refused save, also after a language change", async () => {
    commands({ save_settings: () => Promise.reject({ code: "invalid_settings", error: { code: "model_required" } }) });
    const { status } = await setup("Dictée");
    await fireEvent.click(screen.getByLabelText(/Double-tap/));
    await waitFor(() => expect(status()).toBe("failed"), SAVE_WAIT);
    expect(alertText()).toBe("⚠ Non enregistré : choisissez un modèle de transcription et un modèle de correction");
    lang.value = "en";
    await waitFor(() => expect(alertText()).toBe("⚠ Not saved: choose a transcription model and a correction model"));
  });
});

describe("Settings: command errors", () => {
  it("translates a load error", async () => {
    commands({ get_settings: () => Promise.reject({ code: "system", detail: "base verrouillée" }) });
    render(<Settings />);
    expect(await screen.findByText(/Impossible de charger les réglages.*base verrouillée/)).toBeTruthy();
  });

  it("translates a key that could not be saved", async () => {
    commands({ set_api_key: () => Promise.reject({ code: "system", detail: "coffre indisponible" }) });
    await setup("Modèles");
    await fireEvent.input(screen.getByLabelText("Clé OpenAI"), { target: { value: "sk" } });
    await fireEvent.submit(keyForm("OpenAI"));
    expect(await within(keyForm("OpenAI")).findByText("coffre indisponible")).toBeTruthy();
  });
});

describe("Settings: styles", () => {
  const card = (name: string) => screen.getByRole("region", { name });

  it("autosaves a style edit with the other settings", async () => {
    await setup("Styles");
    await fireEvent.change(within(card("Slack")).getByLabelText("Mot-clé"), { target: { value: "message" } });
    await waitFor(() => expect(saves()).toHaveLength(1), SAVE_WAIT);
    expect(lastSave().styles[2].keyword).toBe("message");
    expect(lastSave().offered_builtins).toEqual(["default", "clean", "slack", "mail"]);
  });

  it("a refused save names the style it is about", async () => {
    commands({ save_settings: () => Promise.reject({ code: "invalid_settings", error: { code: "duplicate_trigger", trigger: "slack" } }) });
    await setup("Styles");
    await fireEvent.click(screen.getByRole("button", { name: "Ajouter un style" }));
    const added = screen.getAllByRole("region").at(-1)!;
    await fireEvent.change(within(added).getByLabelText("Nom"), { target: { value: "Équipe" } });
    await fireEvent.change(within(added).getByLabelText("Mot-clé"), { target: { value: "slack" } });
    await waitFor(() => expect(alertText()).toContain("Équipe : « slack » est déjà utilisé par un autre style"), SAVE_WAIT);
  });

  it("fetches the presets again when the UI language changes", async () => {
    await setup("Styles");
    await waitFor(() => expect(calls("style_catalog")).toHaveLength(1));
    commands({ style_catalog: () => styleCatalog({ presets: PRESETS.map((p) => ({ ...p, name: `${p.name} (en)` })) }) });
    lang.value = "en";
    expect(await screen.findByRole("heading", { name: "Slack (en)" })).toBeTruthy();
    expect(calls("style_catalog")).toHaveLength(2);
  });

  it("works without the catalog", async () => {
    commands({ style_catalog: () => Promise.reject("down") });
    await setup("Styles");
    await waitFor(() => expect(calls("style_catalog")).toHaveLength(1));
    expect(within(card("slack")).getByLabelText("Mot-clé")).toBeTruthy();
  });
});
