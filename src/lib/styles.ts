import type { Language, Preset, PresetView, Style } from "./api";
import { t } from "./i18n";

export const DEFAULT_STYLE_ID = "default";
/** Mirror of `style::MAX_STYLES`. */
export const MAX_STYLES = 10;

/** A user style: no preset, so it needs a name, a keyword and a task before it can be saved. */
export function newStyle(id: string): Style {
  return { id, preset: null, name: null, keyword: "", aliases: [], custom_task: null };
}

function presetOf(style: Style, presets: PresetView[]): PresetView | undefined {
  return presets.find((p) => p.preset === style.preset);
}

/** Name shown for the style at `index`: its name, else its preset's, else its keyword, else « Style n ». */
export function styleName(style: Style | undefined, presets: PresetView[], index: number): string {
  const name = style?.name?.trim();
  if (name) return name;
  const preset = style && presetOf(style, presets);
  if (preset) return preset.name;
  const keyword = style?.keyword?.trim();
  return keyword || t.value.styles.number(index + 1);
}

/** The name field's placeholder: the preset's name for a built-in. */
export function namePlaceholder(style: Style, presets: PresetView[]): string {
  return presetOf(style, presets)?.name ?? "";
}

export interface TaskView {
  text: string;
  /** null: a built-in text, which follows the app language. */
  language: Language | null;
}

export function taskView(style: Style, presets: PresetView[]): TaskView {
  if (style.custom_task) return { text: style.custom_task.text, language: style.custom_task.language };
  return { text: presetOf(style, presets)?.task ?? "", language: null };
}

/** The task once `text` is committed: it becomes custom, in `lang`, unless it is the built-in text unchanged. */
export function editTask(style: Style, text: string, lang: Language, presets: PresetView[]): Style {
  if (!style.custom_task && text === taskView(style, presets).text) return style;
  return { ...style, custom_task: { text, language: lang } };
}

export function resetTask(style: Style): Style {
  return { ...style, custom_task: null };
}

export function canReset(style: Style): boolean {
  return style.preset !== null && style.custom_task !== null;
}

export function canDelete(style: Style): boolean {
  return style.id !== DEFAULT_STYLE_ID;
}

/** Indexes of `styles` in display order: the default style first, the others as saved. */
export function displayOrder(styles: Style[]): number[] {
  const indexes = styles.map((_, i) => i);
  return [...indexes.filter((i) => styles[i].id === DEFAULT_STYLE_ID), ...indexes.filter((i) => styles[i].id !== DEFAULT_STYLE_ID)];
}

/** Built-in presets with no style of that id (catalog ids are the preset names). */
export function missingBuiltins(styles: Style[], presets: PresetView[]): Preset[] {
  return presets.map((p) => p.preset).filter((id) => !styles.some((s) => s.id === id));
}

/** Index of the last style whose keyword or alias is `trigger` as typed (what `duplicate_trigger` reports). */
export function triggerOwner(styles: Style[], trigger: string): number | null {
  for (let i = styles.length - 1; i >= 0; i--) {
    const s = styles[i];
    if (s.keyword === trigger || s.aliases.includes(trigger)) return i;
  }
  return null;
}
