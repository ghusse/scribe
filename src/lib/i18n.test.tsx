import { describe, expect, test } from "bun:test";
import { render, screen } from "@testing-library/preact";
import { en } from "../locales/en";
import { fr } from "../locales/fr";
import { lang, pickLanguage, t, useLanguage } from "./i18n";
import { commands, emit } from "../../tests/tauri";

/** Every leaf of a catalog, functions called with placeholder arguments: `path → text`. */
function leaves(node: unknown, path = ""): Map<string, string> {
  const out = new Map<string, string>();
  if (typeof node === "string") out.set(path, node);
  else if (typeof node === "function") out.set(path, String(node(...Array(node.length).fill(7))));
  else for (const [k, v] of Object.entries(node as object)) for (const [p, s] of leaves(v, `${path}.${k}`)) out.set(p, s);
  return out;
}

describe("catalogs", () => {
  test("French has exactly the English keys, each with a non-empty text", () => {
    const e = leaves(en);
    const f = leaves(fr);
    expect([...f.keys()].sort()).toEqual([...e.keys()].sort());
    for (const [path, text] of [...e, ...f]) expect({ path, empty: text.trim() === "" }).toEqual({ path, empty: false });
  });
});

describe("pickLanguage", () => {
  test("takes the first supported tag, regional or not, else English", () => {
    expect(pickLanguage(["fr-FR", "en-US"])).toBe("fr");
    expect(pickLanguage(["de-DE", "en_GB", "fr"])).toBe("en");
    expect(pickLanguage(["FR"])).toBe("fr");
    expect(pickLanguage(["de", "it"])).toBe("en");
    expect(pickLanguage([])).toBe("en");
  });
});

function Probe() {
  useLanguage();
  return <p>{t.value.errors.app.noText}</p>;
}

describe("useLanguage", () => {
  test("follows the backend's language, then each language event", async () => {
    commands({ language: () => "en" });
    render(<Probe />);
    expect(await screen.findByText("this dictation has no text")).toBeTruthy();
    expect(document.documentElement.lang).toBe("en");
    emit("language", "fr");
    expect(await screen.findByText("cette dictée n'a pas de texte")).toBeTruthy();
    expect(lang.value).toBe("fr");
  });

  test("keeps the current language when the backend cannot answer", async () => {
    commands({ language: () => Promise.reject("down") });
    render(<Probe />);
    await Promise.resolve();
    expect(screen.getByText("cette dictée n'a pas de texte")).toBeTruthy();
  });
});
