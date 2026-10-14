import type { Language, LanguagePref, Level } from "./api";
import { t } from "./i18n";
import { OS, type Os } from "./platform";

/** « Réflexion du modèle » choices; the values sent to the backend stay low / medium / high. */
export function effortLabel(effort: string): string {
  return (t.value.settings.models.efforts as Record<string, string>)[effort] ?? effort;
}

const LEVELS: Level[] = ["raw", "clean", "formatted"];

/** « Niveau de correction » choices, in display order. */
export function levelOptions(): { value: Level; label: string }[] {
  return LEVELS.map((value) => ({ value, label: t.value.settings.correction.levels[value] }));
}

/** Each language in its own name, whatever the UI language. */
const LANGUAGE_NAMES: Record<Language, string> = { en: "English", fr: "Français" };

export function languageOptions(): { value: LanguagePref; label: string }[] {
  return [
    { value: "system", label: t.value.settings.language.system },
    ...(Object.entries(LANGUAGE_NAMES) as [Language, string][]).map(([value, label]) => ({ value, label })),
  ];
}

export function keysHelp(os: Os = OS): string {
  return t.value.settings.apiKeys.help[os];
}
