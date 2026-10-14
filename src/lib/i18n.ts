import { computed, effect, signal } from "@preact/signals";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "preact/hooks";
import { en, type Messages } from "../locales/en";
import { fr } from "../locales/fr";
import { api, type Language } from "./api";

export type { Messages };

const CATALOGS: Record<Language, Messages> = { en, fr };

/** First supported language among BCP 47 tags, else English. Same rule as `locale::system_language` (Rust). */
export function pickLanguage(tags: readonly string[]): Language {
  for (const tag of tags) {
    const base = tag.toLowerCase().split(/[-_]/)[0];
    if (base === "fr" || base === "en") return base;
  }
  return "en";
}

/** Until the backend answers: the webview's languages, so the first paint is rarely in the wrong one. */
export const lang = signal<Language>(pickLanguage(navigator.languages ?? [navigator.language]));

/** The current catalog: reading it in a component re-renders it when the language changes. */
export const t = computed(() => CATALOGS[lang.value]);

effect(() => {
  document.documentElement.lang = lang.value;
});

/** Follows the backend's language (saved choice, else the system's) in this window. */
export function useLanguage() {
  useEffect(() => {
    api
      .language()
      .then((l) => (lang.value = l))
      .catch(() => {});
    const un = listen<Language>("language", (e) => (lang.value = e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
}
