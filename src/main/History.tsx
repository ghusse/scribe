import { useSignal } from "@preact/signals";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "preact/hooks";
import { api, type Dictation } from "../lib/api";
import {
  canCopy,
  canRetranscribe,
  COPY_FEEDBACK_MS,
  displayText,
  editDraft,
  editPayload,
  latestOnly,
  STALE,
  timingLine,
} from "../lib/history";
import { errorText, storedErrorText } from "../lib/errors";
import { lang, t } from "../lib/i18n";
import { cx, useTick } from "../lib/ui";
import styles from "./History.module.css";

export type HistoryProps = { focus?: { id: number } | null; onFocused?: () => void };

const PAGE = 50;

export default function History({ focus = null, onFocused }: HistoryProps) {
  const items = useSignal<Dictation[]>([]);
  const query = useSignal("");
  const hasMore = useSignal(false);
  const error = useSignal<string | null>(null);
  const editing = useSignal<number | null>(null);
  const draft = useSignal("");
  const expanded = useSignal<number | null>(null);
  const confirmDelete = useSignal<number | null>(null);
  const busy = useSignal<number | null>(null);
  const highlighted = useSignal<number | null>(null);
  /** The card whose text was just copied: its button reads « ✓ Copié » and the card flashes. */
  const copied = useSignal<number | null>(null);
  const timers = useRef<{ search?: ReturnType<typeof setTimeout>; copied?: ReturnType<typeof setTimeout> }>({});
  // Search results can come back out of order: only the latest request's page is shown.
  const latest = useRef(latestOnly()).current;
  const tick = useTick();
  const onFocusedRef = useRef(onFocused);
  onFocusedRef.current = onFocused;

  async function load(reset = true) {
    try {
      const page = await latest(api.listDictations(query.value || null, PAGE, reset ? 0 : items.value.length));
      if (page === STALE) return;
      items.value = reset ? page : [...items.value, ...page];
      hasMore.value = page.length === PAGE;
      error.value = null;
    } catch (e) {
      error.value = errorText(e);
    }
  }

  function onSearch(e: Event) {
    query.value = (e.currentTarget as HTMLInputElement).value;
    clearTimeout(timers.current.search);
    timers.current.search = setTimeout(() => load(true), 250);
  }

  async function run(id: number, action: () => Promise<void>): Promise<boolean> {
    busy.value = id;
    try {
      await action();
      error.value = null;
      return true;
    } catch (e) {
      error.value = errorText(e);
      return false;
    } finally {
      busy.value = null;
    }
  }

  async function copy(id: number) {
    if (!(await run(id, () => api.copyDictation(id)))) return;
    clearTimeout(timers.current.copied);
    // Off then on again, so copying the same card twice replays the flash.
    copied.value = null;
    await tick();
    copied.value = id;
    timers.current.copied = setTimeout(() => (copied.value = null), COPY_FEEDBACK_MS);
  }

  function startEdit(d: Dictation) {
    editing.value = d.id;
    draft.value = editDraft(d);
  }

  async function saveEdit(d: Dictation) {
    // On failure keep the editor open (and the draft) so the error stays visible.
    if (!(await run(d.id, () => api.saveEditedText(d.id, editPayload(draft.value))))) return;
    editing.value = null;
    await load(true);
  }

  async function focusOn(id: number) {
    query.value = "";
    await load(true);
    highlighted.value = id;
    await tick();
    document.getElementById(`d-${id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    onFocusedRef.current?.();
  }

  // The focus-dictation event is received by App (always mounted) and passed down here,
  // including when this tab is mounted because of it.
  useEffect(() => {
    if (focus) focusOn(focus.id);
  }, [focus]);

  useEffect(() => {
    if (!focus) load(true);
    const unChanged = listen("history-changed", () => load(true));
    return () => {
      clearTimeout(timers.current.search);
      clearTimeout(timers.current.copied);
      unChanged.then((f) => f());
    };
  }, []);

  const m = t.value.history;
  return (
    <>
      <input class={styles.search} placeholder={m.search} value={query.value} onInput={onSearch} />
      {error.value && <p class={styles.error}>{error.value}</p>}
      <p class={styles.srOnly} aria-live="polite">
        {copied.value !== null ? m.copiedAnnouncement : ""}
      </p>
      {items.value.length === 0 && (
        <p class={styles.muted}>{m.empty}</p>
      )}

      {items.value.map((d) => (
        <article
          key={d.id}
          id={`d-${d.id}`}
          class={cx(styles.card, highlighted.value === d.id && styles.highlighted, copied.value === d.id && styles.justCopied)}
        >
          <header>
            <span class={cx(styles.badge, styles[d.outcome])}>{m.outcome[d.outcome]}</span>
            <span class={styles.muted}>{new Date(d.created_at).toLocaleString(lang.value)}</span>
            {d.app_name && <span class={styles.muted}>· {d.app_name}</span>}
            <span class={cx(styles.muted, styles.right)}>{timingLine(d, lang.value)}</span>
          </header>

          {editing.value === d.id ? (
            <>
              <textarea
                rows={4}
                value={draft.value}
                onInput={(e) => (draft.value = (e.currentTarget as HTMLTextAreaElement).value)}
              />
              <div class={styles.row}>
                <button class="primary" onClick={() => saveEdit(d)}>
                  {m.saveEdit}
                </button>
                <button onClick={() => (editing.value = null)}>{m.cancel}</button>
              </div>
            </>
          ) : (
            <p class={styles.text}>{displayText(d)}</p>
          )}
          {d.error && <p class={cx(styles.error, styles.small)}>{storedErrorText(d.error)}</p>}

          {expanded.value === d.id && (
            <dl>
              <dt>{m.raw}</dt>
              <dd>{d.raw_text ?? "—"}</dd>
              <dt>{m.corrected}</dt>
              <dd>{d.final_text ?? "—"}</dd>
              {d.edited_text && (
                <>
                  <dt>{m.yourCorrection}</dt>
                  <dd>{d.edited_text}</dd>
                </>
              )}
              <dt>{m.models}</dt>
              <dd>
                {d.transcriber ?? "—"} / {d.corrector ?? "—"}
              </dd>
            </dl>
          )}

          <div class={styles.row}>
            <button
              class={cx(styles.copy, copied.value === d.id && styles.copied)}
              onClick={() => copy(d.id)}
              disabled={!canCopy(d)}
            >
              {copied.value === d.id ? m.copied : m.copy}
            </button>
            <button onClick={() => startEdit(d)} disabled={!canCopy(d)}>
              {m.edit}
            </button>
            <button onClick={() => run(d.id, () => api.retranscribe(d.id))} disabled={!canRetranscribe(d, busy.value)}>
              {busy.value === d.id ? "…" : m.retranscribe}
            </button>
            <button onClick={() => (expanded.value = expanded.value === d.id ? null : d.id)}>
              {expanded.value === d.id ? m.less : m.details}
            </button>
            {confirmDelete.value === d.id ? (
              <>
                <button
                  class="danger"
                  onClick={() =>
                    run(d.id, async () => {
                      await api.deleteDictation(d.id);
                      confirmDelete.value = null;
                      await load(true);
                    })
                  }
                >
                  {m.confirmDelete}
                </button>
                <button onClick={() => (confirmDelete.value = null)}>{m.cancel}</button>
              </>
            ) : (
              <button class="danger" onClick={() => (confirmDelete.value = d.id)}>
                {m.delete}
              </button>
            )}
          </div>
        </article>
      ))}
      {hasMore.value && <button onClick={() => load(false)}>{m.more}</button>}
    </>
  );
}
