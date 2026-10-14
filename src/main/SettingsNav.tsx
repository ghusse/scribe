import { t } from "../lib/i18n";
import styles from "./Settings.module.css";

export type Category = "general" | "dictation" | "models" | "advanced";
export const CATEGORIES: Category[] = ["general", "dictation", "models", "advanced"];

export interface SettingsNavProps {
  current: Category;
  /** Categories holding something to fix (a missing key, an invalid number). */
  attention: Partial<Record<Category, boolean>>;
  onSelect: (next: Category) => void;
}

export default function SettingsNav({ current, attention, onSelect }: SettingsNavProps) {
  const m = t.value.settings.nav;
  return (
    <nav class={styles.nav} aria-label={m.label}>
      {CATEGORIES.map((c) => (
        <button key={c} type="button" aria-current={c === current ? "page" : undefined} onClick={() => onSelect(c)}>
          {m[c]}
          {attention[c] && <span class={styles.dot} role="img" aria-label={m.attention} title={m.attention}></span>}
        </button>
      ))}
    </nav>
  );
}
