import { describe, expect, it } from "bun:test";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/preact";
import { useState } from "preact/hooks";
import type { PresetView, Settings as SettingsT, Style } from "../lib/api";
import { lang } from "../lib/i18n";
import { newStyle } from "../lib/styles";
import { calls, commands } from "../../tests/tauri";
import { PRESETS, settings, STYLES } from "../../tests/fixtures";
import Styles from "./Styles";

/** Renders the pane under a parent holding the settings, applying each edit like Settings does. */
function setup(over: Partial<SettingsT> = {}, presets: PresetView[] = PRESETS) {
  let cur = settings(over);
  function Parent() {
    const [s, setS] = useState(cur);
    const edit = (change: (d: SettingsT) => void) => {
      const next = structuredClone(cur);
      change(next);
      cur = next;
      setS(next);
    };
    return <Styles cur={s} presets={presets} edit={edit} />;
  }
  render(<Parent />);
  return { styles: () => cur.styles };
}

const card = (name: string) => screen.getByRole("region", { name });
const field = (name: string, label: string) => within(card(name)).getByLabelText(label) as HTMLInputElement & HTMLTextAreaElement;
const commit = (el: HTMLElement, value: string) => fireEvent.change(el, { target: { value } });
const titles = () => screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);

describe("Styles: cards", () => {
  it("one card per style, the default first", () => {
    const [def, ...others] = STYLES;
    setup({ styles: [...others, def] });
    expect(titles()).toEqual(["Par défaut", "Propre", "Slack", "Mail"]);
  });

  it("the default style has no keyword and cannot be deleted", () => {
    setup();
    const def = card("Par défaut");
    expect(within(def).getByText("S'applique quand aucun mot-clé n'est dit.")).toBeTruthy();
    expect(within(def).queryByLabelText("Mot-clé")).toBeNull();
    expect(within(def).queryByRole("button", { name: "Supprimer" })).toBeNull();
    expect(field("Mail", "Mot-clé").value).toBe("mail");
    expect(field("Mail", "Autres formes entendues").value).toBe("email, e-mail, courriel");
    expect(field("Mail", "Nom").placeholder).toBe("Mail");
  });

  it("a built-in task shows the preset text and follows the app language", () => {
    setup();
    expect(field("Slack", "Instructions").value).toBe("- Rédige un message Slack.");
    expect(within(card("Slack")).getByText("Suit la langue de l'application")).toBeTruthy();
    expect(within(card("Slack")).queryByRole("button", { name: "Rétablir l'original" })).toBeNull();
  });

  it("shows the preset text once the catalog arrives", () => {
    const { rerender } = render(<Styles cur={settings()} presets={[]} edit={() => {}} />);
    expect((within(card("Style 1")).getByLabelText("Instructions") as HTMLTextAreaElement).value).toBe("");
    rerender(<Styles cur={settings()} presets={PRESETS} edit={() => {}} />);
    expect(field("Par défaut", "Instructions").value).toBe("- Corrige et mets en forme.");
  });
});

describe("Styles: editing", () => {
  it("the first edit of a built-in task makes it custom, in the UI language; reset brings the built-in back", async () => {
    const { styles } = setup();
    await commit(field("Slack", "Instructions"), "- Messages très courts.");
    expect(styles()[2].custom_task).toEqual({ text: "- Messages très courts.", language: "fr" });
    expect(within(card("Slack")).getByText("Rédigées en français")).toBeTruthy();
    await fireEvent.click(within(card("Slack")).getByRole("button", { name: "Rétablir l'original" }));
    expect(styles()[2]).toEqual(STYLES[2]);
    expect(field("Slack", "Instructions").value).toBe("- Rédige un message Slack.");
  });

  it("an edited task keeps its language when the UI language changes", async () => {
    const custom: Style = { ...STYLES[2], custom_task: { text: "- Short.", language: "en" } };
    setup({ styles: [STYLES[0], custom] });
    expect(within(card("Slack")).getByText("Rédigées en anglais")).toBeTruthy();
    lang.value = "en";
    expect(await screen.findByText("Written in English")).toBeTruthy();
    expect(field("Slack", "Instructions").value).toBe("- Short.");
  });

  it("committing the built-in text unchanged keeps the task built-in", async () => {
    const { styles } = setup();
    await commit(field("Slack", "Instructions"), "- Rédige un message Slack.");
    expect(styles()[2].custom_task).toBeNull();
  });

  it("commits the name, keyword and spoken forms", async () => {
    const { styles } = setup();
    await commit(field("Mail", "Nom"), "Courriel pro");
    expect(styles()[3].name).toBe("Courriel pro");
    await commit(field("Courriel pro", "Nom"), "  ");
    expect(styles()[3].name).toBeNull();
    await commit(field("Mail", "Mot-clé"), "  pro ");
    expect(styles()[3].keyword).toBe("pro");
    await commit(field("Mail", "Autres formes entendues"), "mel, , mèl ");
    expect(styles()[3].aliases).toEqual(["mel", "mèl"]);
    expect(styles().slice(0, 3)).toEqual(STYLES.slice(0, 3));
  });

  it("an improved task is used as a custom task in the UI language", async () => {
    commands({ improve_style: () => "- Proposé." });
    const { styles } = setup();
    const c = card("Slack");
    await fireEvent.click(within(c).getByRole("button", { name: "Améliorer avec l'IA" }));
    await fireEvent.input(within(c).getByLabelText("Que doit faire ce style ?"), { target: { value: "plus court" } });
    await fireEvent.click(within(c).getByRole("button", { name: "Générer" }));
    expect(calls("improve_style")[0]).toEqual({ request: { name: "Slack", keyword: "slack", draft: "- Rédige un message Slack.", instructions: "plus court" } });
    await fireEvent.click(await within(c).findByRole("button", { name: "Utiliser" }));
    expect(styles()[2].custom_task).toEqual({ text: "- Proposé.", language: "fr" });
    expect(field("Slack", "Instructions").value).toBe("- Proposé.");
  });

  it("opens one improve panel at a time, and its button closes it", async () => {
    setup();
    const open = (name: string) => within(card(name)).getByRole("button", { name: "Améliorer avec l'IA" });
    const panels = () => screen.queryAllByLabelText("Que doit faire ce style ?");
    expect(panels()).toHaveLength(0);
    await fireEvent.click(open("Slack"));
    expect(open("Slack").getAttribute("aria-expanded")).toBe("true");
    expect(within(card("Slack")).getByLabelText("Que doit faire ce style ?")).toBeTruthy();
    await fireEvent.click(open("Mail"));
    expect(panels()).toHaveLength(1);
    expect(within(card("Mail")).getByLabelText("Que doit faire ce style ?")).toBeTruthy();
    await fireEvent.click(open("Mail"));
    expect(panels()).toHaveLength(0);
    await fireEvent.click(open("Slack"));
    await fireEvent.click(within(card("Slack")).getByRole("button", { name: "Annuler" }));
    expect(panels()).toHaveLength(0);
  });
});

describe("Styles: adding and deleting", () => {
  it("deletes a style", async () => {
    const { styles } = setup();
    await fireEvent.click(within(card("Slack")).getByRole("button", { name: "Supprimer" }));
    expect(styles().map((s) => s.id)).toEqual(["default", "clean", "mail"]);
    expect(titles()).toEqual(["Par défaut", "Propre", "Mail"]);
  });

  it("adds a user style with a fresh id", async () => {
    const { styles } = setup();
    await fireEvent.click(screen.getByRole("button", { name: "Ajouter un style" }));
    const added = styles()[4];
    expect(added).toEqual({ ...newStyle(added.id), id: added.id });
    expect(added.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(titles().at(-1)).toBe("Style 5");
    await commit(field("Style 5", "Mot-clé"), "cr");
    expect(titles().at(-1)).toBe("cr");
  });

  it("cannot add beyond the maximum", () => {
    const many = Array.from({ length: 6 }, (_, i) => ({ ...newStyle(`u${i}`), name: `S${i}`, keyword: `k${i}` }));
    setup({ styles: [...STYLES, ...many] });
    expect((screen.getByRole("button", { name: "Ajouter un style" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("10 styles au maximum.")).toBeTruthy();
  });
});

describe("Styles: restore built-in styles", () => {
  const restore = () => screen.getByRole("button", { name: "Rétablir les styles intégrés" }) as HTMLButtonElement;

  it("is disabled when no built-in is missing", () => {
    setup();
    expect(restore().disabled).toBe(true);
  });

  it("adds what the backend restored, keeping the edited styles", async () => {
    const edited: Style = { ...STYLES[2], name: "Équipe" };
    commands({ restore_builtin_styles: (a) => [...(a!.styles as Style[]), STYLES[1], STYLES[3]] });
    const { styles } = setup({ styles: [STYLES[0], edited] });
    await fireEvent.click(restore());
    await waitFor(() => expect(styles()).toHaveLength(4));
    expect(calls("restore_builtin_styles")).toEqual([{ styles: [STYLES[0], edited] }]);
    expect(styles()).toEqual([STYLES[0], edited, STYLES[1], STYLES[3]]);
    expect(restore().disabled).toBe(true);
  });

  it("says when it fails", async () => {
    commands({ restore_builtin_styles: () => Promise.reject({ code: "system", detail: "boom" }) });
    const { styles } = setup({ styles: [STYLES[0]] });
    await fireEvent.click(restore());
    expect((await screen.findByRole("alert")).textContent).toBe("Impossible de rétablir les styles intégrés : boom");
    expect(styles()).toEqual([STYLES[0]]);
    expect(restore().disabled).toBe(false);
  });
});

describe("Styles: correction off", () => {
  it("disables the pane and says keywords are still removed", () => {
    setup({ correction_enabled: false });
    expect(screen.getByText(/Un mot-clé dit en début de dictée est quand même retiré/)).toBeTruthy();
    expect((document.querySelector("fieldset") as HTMLFieldSetElement).disabled).toBe(true);
  });

  it("is enabled when correction is on", () => {
    setup();
    expect(screen.queryByText(/quand même retiré/)).toBeNull();
    expect((document.querySelector("fieldset") as HTMLFieldSetElement).disabled).toBe(false);
  });
});
