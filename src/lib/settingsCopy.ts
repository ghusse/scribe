import type { Level } from "./api";

/** « Réflexion du modèle » choices; the values sent to the backend stay low / medium / high. */
export const EFFORT_LABELS: Record<string, string> = {
  low: "Faible (rapide, recommandé)",
  medium: "Moyenne",
  high: "Élevée (plus lent, plus cher)",
};

export function effortLabel(effort: string): string {
  return EFFORT_LABELS[effort] ?? effort;
}

/** « Niveau de correction » choices, in display order. */
export const LEVEL_OPTIONS: { value: Level; label: string }[] = [
  { value: "raw", label: "Aucune (texte brut)" },
  { value: "clean", label: "Nettoyage (ponctuation, vocabulaire, hésitations)" },
  { value: "formatted", label: "Nettoyage + mise en forme selon l'application" },
];

export const RAW_LEVEL_NOTE = "Correction désactivée en mode brut";

export const HOTKEY_HELP =
  "Maintenez pour dicter, relâchez pour envoyer. Double-tap (ou maintien + touche de verrouillage) pour dicter mains libres ; appuyez à nouveau pour arrêter.";

export const KEYS_HELP =
  "Stockées dans le coffre Windows. Chaque clé s'enregistre avec son bouton ; les autres réglages s'enregistrent automatiquement.";
