import { useSignal } from "@preact/signals";
import { useLayoutEffect, useRef } from "preact/hooks";
import { api, type PresetView, type Settings as SettingsT, type Style } from "../lib/api";
import { errorText } from "../lib/errors";
import { joinVariants, splitVariants } from "../lib/glossary";
import { lang, t } from "../lib/i18n";
import {
  canDelete,
  canReset,
  DEFAULT_STYLE_ID,
  displayOrder,
  editTask,
  MAX_STYLES,
  missingBuiltins,
  namePlaceholder,
  newStyle,
  resetTask,
  styleName,
  taskView,
} from "../lib/styles";
import { cx } from "../lib/ui";
import { Plus, Sparkles, Trash, Undo } from "./icons";
import ImproveStyle from "./ImproveStyle";
import base from "./Settings.module.css";
import styles from "./Styles.module.css";

export interface StylesProps {
  cur: SettingsT;
  /** Built-in presets in the UI language (`style_catalog`). */
  presets: PresetView[];
  edit: (change: (draft: SettingsT) => void) => void;
}

interface TextFieldProps {
  label: string;
  value: string;
  placeholder?: string;
  multiline?: boolean;
  onCommit: (text: string) => void;
}

/** Committed on change (blur or Enter), never per keystroke: a half-typed keyword must not be saved. */
function TextField({ label, value, placeholder, multiline, onCommit }: TextFieldProps) {
  const ref = useRef<HTMLInputElement & HTMLTextAreaElement>(null);
  // Written only when the value changes: a re-render must not wipe what is being typed.
  useLayoutEffect(() => {
    ref.current!.value = value;
  }, [value]);
  const commit = (e: Event) => onCommit((e.currentTarget as HTMLInputElement).value);
  if (multiline)
    return (
      <label class={styles.task}>
        <span>{label}</span>
        <textarea ref={ref} placeholder={placeholder} onChange={commit} />
      </label>
    );
  return (
    <label class={base.row}>
      {label}
      <input ref={ref} placeholder={placeholder} onChange={commit} />
    </label>
  );
}

/** The Styles pane: one card per style, the default first, edited through the settings autosave. */
export default function Styles({ cur, presets, edit }: StylesProps) {
  const restoring = useSignal(false);
  const restoreError = useSignal<unknown>(null);
  const improving = useSignal<string | null>(null);

  const update = (id: string, change: (s: Style) => Style) =>
    edit((d) => {
      d.styles = d.styles.map((s) => (s.id === id ? change(s) : s));
    });

  async function restore() {
    restoring.value = true;
    restoreError.value = null;
    try {
      const restored = await api.restoreBuiltinStyles(cur.styles);
      edit((d) => {
        d.styles = restored;
      });
    } catch (e) {
      restoreError.value = e;
    } finally {
      restoring.value = false;
    }
  }

  const m = t.value.styles;
  const off = !cur.correction_enabled;
  const full = cur.styles.length >= MAX_STYLES;

  const card = (index: number) => {
    const s = cur.styles[index];
    const name = styleName(s, presets, index);
    const task = taskView(s, presets);
    const isDefault = s.id === DEFAULT_STYLE_ID;
    return (
      <section key={s.id} class={base.section} aria-label={name}>
        <div class={styles.head}>
          <h3 class={base.heading}>{name}</h3>
          {canDelete(s) && (
            <button
              type="button"
              class="ghost danger"
              onClick={() => edit((d) => (d.styles = d.styles.filter((x) => x.id !== s.id)))}
            >
              <Trash />
              {m.delete}
            </button>
          )}
        </div>
        {isDefault && <p class={base.help}>{m.defaultHint}</p>}
        <TextField
          label={m.name}
          value={s.name ?? ""}
          placeholder={namePlaceholder(s, presets)}
          onCommit={(v) => update(s.id, (x) => ({ ...x, name: v.trim() === "" ? null : v }))}
        />
        {!isDefault && (
          <>
            <TextField label={m.keyword} value={s.keyword ?? ""} onCommit={(v) => update(s.id, (x) => ({ ...x, keyword: v.trim() }))} />
            <TextField
              label={m.aliases}
              value={joinVariants(s.aliases)}
              placeholder={m.aliasesPlaceholder}
              onCommit={(v) => update(s.id, (x) => ({ ...x, aliases: splitVariants(v) }))}
            />
          </>
        )}
        <TextField
          label={m.task}
          value={task.text}
          multiline
          onCommit={(v) => update(s.id, (x) => editTask(x, v, lang.value, presets))}
        />
        <div class={styles.taskFoot}>
          <span class={styles.language}>
            {task.language === null ? m.followsLanguage : m.writtenIn(m.languages[task.language])}
          </span>
          {canReset(s) && (
            <button type="button" class="ghost" onClick={() => update(s.id, resetTask)}>
              <Undo />
              {m.reset}
            </button>
          )}
          <button
            type="button"
            class="accent"
            aria-expanded={improving.value === s.id}
            aria-controls={`improve-${s.id}`}
            onClick={() => (improving.value = improving.value === s.id ? null : s.id)}
          >
            <Sparkles />
            {m.improve.open}
          </button>
        </div>
        {improving.value === s.id && (
          <ImproveStyle
            id={`improve-${s.id}`}
            name={name}
            keyword={s.keyword}
            draft={task.text}
            onAccept={(text) => update(s.id, (x) => ({ ...x, custom_task: { text, language: lang.value } }))}
            onClose={() => (improving.value = null)}
          />
        )}
      </section>
    );
  };

  return (
    <>
      <p class={base.help}>{m.help}</p>
      {off && <p class={base.note}>{m.correctionOff}</p>}
      <fieldset disabled={off} class={cx(styles.fieldset, off && styles.off)}>
        {displayOrder(cur.styles).map(card)}
        <div class={styles.footer}>
          <button type="button" class="primary" onClick={() => edit((d) => d.styles.push(newStyle(crypto.randomUUID())))} disabled={full}>
            <Plus />
            {m.add}
          </button>
          <button type="button" class="ghost" onClick={restore} disabled={restoring.value || missingBuiltins(cur.styles, presets).length === 0}>
            {m.restore}
          </button>
          {full && <span class={base.note}>{m.max(MAX_STYLES)}</span>}
        </div>
        {restoreError.value !== null && (
          <p class={styles.error} role="alert">
            {m.restoreFailed(errorText(restoreError.value))}
          </p>
        )}
      </fieldset>
    </>
  );
}
