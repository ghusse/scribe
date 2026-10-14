import { untracked, useComputed, useSignal } from "@preact/signals";
import { Fragment, type ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useState } from "preact/hooks";
import { api, effortLevels, type LanguagePref, type Provider, type ProviderTest, type Settings as SettingsT } from "../lib/api";
import { chordName, keyName } from "../lib/keys";
import { editAction, SAVED_FADE_MS, staleGuard, statusView, testSignature, type SaveState } from "../lib/autosave";
import { errorText } from "../lib/errors";
import { t } from "../lib/i18n";
import { checkNumbers, fieldText, retentionOptions, type NumberFieldId } from "../lib/validation";
import {
  assignHotkey,
  captureOutcome,
  normalizeLock,
  pendingSummary,
  swallowsKey,
  type Assignment,
  type HotkeyRole,
} from "../lib/hotkeys";
import {
  canSaveKey,
  keyPlaceholder,
  keyState,
  keyStateLabel,
  keyUsage,
  listParts,
  missingKeys,
  missingKeyWarning,
  roleLabel,
  rolesText,
  setupBannerParts,
  splitProviders,
  unsavedDraftMessage,
  unsavedDrafts,
  type KeyRole,
} from "../lib/apiKeys";
import { testFailure, testLine } from "../lib/providerTest";
import { effortLabel, keysHelp, languageOptions, levelOptions } from "../lib/settingsCopy";
import ModelPicker from "./ModelPicker";
import NumberField from "./NumberField";
import SaveStatusBar from "./SaveStatusBar";
import SettingsNav, { type Category } from "./SettingsNav";
import Startup from "./Startup";
import Updates from "./Updates";
import { cx, useSyncEffect, useTick } from "../lib/ui";
import styles from "./Settings.module.css";

/** App keeps this view mounted while another tab is shown (so nothing typed here is lost) and reads `status` to flag unsaved settings on the tab. */
export interface SettingsProps {
  active?: boolean;
  status?: SaveState;
  onStatusChange?: (next: SaveState) => void;
}

/** `noteOf` re-runs the assignment so its note is rendered in the current language. */
type Accepted = Extract<Assignment, { ok: true }> & { noteOf: () => string | null };
type KeyFeedback = { done: "saved" | "deleted" } | { error: unknown };
type TestError = { kind: "unsaved" } | { kind: "failed"; error: unknown };

// Stable identity: a new function each render would refocus the element on every render.
const autofocus = (el: HTMLElement | null) => el?.focus();

export default function Settings({ active = true, onStatusChange }: SettingsProps) {
  const s = useSignal<SettingsT | null>(null);
  const loadError = useSignal<unknown>(null);
  const providers = useSignal<Provider[]>([]);
  const keyStatus = useSignal<Record<string, boolean>>({});
  const keyDrafts = useSignal<Record<string, string>>({});
  // One category shown at a time; the others stay mounted so drafts and the autosave go on.
  const category = useSignal<Category>("general");

  // Autosave: every change is persisted ~500 ms after the last edit; the bar shows the outcome.
  // Number fields and the hand-typed model id only change `s` when committed (blur or Entrée).
  const saveState = useSignal<SaveState>("saved");
  // Why the last save failed (the rejection, translated when shown).
  const saveError = useSignal<unknown>(null);
  const savedFlash = useSignal(0);
  // « ✓ Réglages enregistrés » fades out after SAVED_FADE_MS; errors stay until fixed.
  const flashing = useSignal(false);

  const keyBusy = useSignal<Record<string, boolean>>({});
  const keyFeedback = useSignal<Record<string, KeyFeedback>>({});
  const confirmDelete = useSignal<string | null>(null);
  const othersOpen = useSignal(false);

  const test = useSignal<ProviderTest | null>(null);
  const testing = useSignal(false);
  // Messages kept as thunks are translated at render, so they follow a language change.
  const testBlock = useSignal<(() => string) | null>(null);
  const testError = useSignal<TestError | null>(null);

  const capturing = useSignal<HotkeyRole | null>(null);
  const hotkeyNote = useSignal<(() => string | null) | null>(null);
  const pendingAssign = useSignal<Accepted | null>(null);

  const [m] = useState(() => ({
    lastSaved: "",
    lastAttempted: "",
    inFlight: null as Promise<void> | null,
    timer: undefined as ReturnType<typeof setTimeout> | undefined,
    flashTimer: undefined as ReturnType<typeof setTimeout> | undefined,
    feedbackTimers: {} as Record<string, ReturnType<typeof setTimeout>>,
    testGuard: staleGuard(),
    captureCancelled: false,
    // The captured key also reaches the focused « Annuler » button: Space/Enter must not activate it
    // (that would cancel the capture or start another one). Its keyup is swallowed too.
    captureEnded: null as { role: HotkeyRole; at: number } | null,
    swallowedCode: null as string | null,
    keyInputs: {} as Record<string, HTMLInputElement | null>,
    deleteButtons: {} as Record<string, HTMLButtonElement | null>,
  }));
  const tick = useTick();

  const providerOf = (id: string | undefined) => providers.value.find((p) => p.id === id);
  const sttProviders = useComputed(() => providers.value.filter((p) => p.stt_models.length > 0));
  const llmProviders = useComputed(() => providers.value.filter((p) => p.llm_api !== null));
  const sttProvider = useComputed(() => providerOf(s.value?.stt_provider));
  const llmProvider = useComputed(() => providerOf(s.value?.llm_provider));
  const efforts = useComputed(() => (s.value ? effortLevels(llmProvider.value, s.value.llm_model) : []));
  // Keys the current providers and level need (for the missing-key banner and the per-key status).
  const usage = useComputed(() => (s.value ? keyUsage(s.value) : {}));
  const missing = useComputed(() => (s.value ? missingKeys(s.value, keyStatus.value) : []));
  const keyRows = useComputed(() => splitProviders(providers.value, usage.value));
  const labelOf = (id: string) => providerOf(id)?.label ?? id;
  const banner = useComputed(() => setupBannerParts(missing.value, labelOf));
  const raw = useComputed(() => s.value?.level === "raw");
  const numberIssue = useComputed(() => (s.value ? checkNumbers(s.value) : null));
  const bar = useComputed(() => {
    const st = saveState.value;
    const why = st === "failed" ? errorText(saveError.value) : st === "invalid" ? (numberIssue.value?.message ?? null) : null;
    return statusView(st, why, flashing.value);
  });
  const attention = useComputed(() => ({ models: missing.value.length > 0, advanced: numberIssue.value !== null }));
  const testSig = useComputed(() => (s.value ? testSignature(s.value) : ""));

  useSyncEffect(() => {
    const st = saveState.value;
    untracked(() => onStatusChange?.(st));
  });

  /** Applies `change` to a copy of the settings; an edit that changes nothing is not one. */
  function edit(change: (draft: SettingsT) => void) {
    const cur = s.peek();
    if (!cur) return;
    const next = structuredClone(cur);
    change(next);
    if (JSON.stringify(next) !== JSON.stringify(cur)) s.value = next;
  }

  async function load() {
    loadError.value = null;
    try {
      const [st, pr, ks] = await Promise.all([api.getSettings(), api.providers(), api.keyStatus()]);
      const shown = normalizeLock(st); // before lastSaved: showing it is not an edit
      m.lastSaved = m.lastAttempted = JSON.stringify(shown);
      providers.value = pr;
      keyStatus.value = ks;
      s.value = shown;
      // First launch: start where the missing keys are added.
      if (missingKeys(shown, ks).length > 0) category.value = "models";
    } catch (e) {
      loadError.value = e;
    }
  }

  useEffect(() => {
    load();
  }, []);

  // Safety net (App keeps the view mounted): never drop the last edit. A layout cleanup runs at unmount, a passive one only after paint.
  useLayoutEffect(
    () => () => {
      if (saveState.peek() === "pending") flush();
      if (capturing.peek()) cancelCapture();
    },
    [],
  );

  useSyncEffect(() => {
    const cur = s.value;
    if (!cur) return;
    const action = editAction(JSON.stringify(cur), m.lastSaved, m.inFlight !== null);
    if (action === "ignore") return;
    clearTimeout(m.timer);
    if (action === "revert") {
      saveState.value = "saved";
      saveError.value = null;
      return;
    }
    saveState.value = "pending";
    m.timer = setTimeout(flush, 500);
  });

  // Switching provider selects its newest model.
  function onSttProvider(id: string) {
    edit((d) => {
      d.stt_provider = id;
      const p = providerOf(id);
      if (p) d.stt_model = p.stt_models[0];
    });
  }

  function onLlmProvider(id: string) {
    edit((d) => {
      d.llm_provider = id;
      const p = providerOf(id);
      if (p) d.llm_model = p.llm_models[0].id;
      syncEffortOf(d);
    });
  }

  function syncEffortOf(d: SettingsT) {
    const levels = effortLevels(providerOf(d.llm_provider), d.llm_model);
    if (levels.length > 0 && !levels.includes(d.llm_effort)) d.llm_effort = levels[0];
  }

  function flush(): Promise<void> {
    clearTimeout(m.timer);
    const cur = s.peek();
    if (!cur) return Promise.resolve();
    if (m.inFlight) return m.inFlight; // its follow-up saves what changed meanwhile
    const json = JSON.stringify(cur);
    if (json === m.lastSaved) {
      saveState.value = "saved";
      saveError.value = null;
      return Promise.resolve();
    }
    const snap: SettingsT = {
      ...cur,
      max_recording_ms: Math.round(cur.max_recording_ms),
      llm_timeout_base_ms: Math.round(cur.llm_timeout_base_ms),
    };
    if (checkNumbers(snap)) {
      // The bar quotes `numberIssue`, which follows the language.
      saveState.value = "invalid";
      saveError.value = null;
      return Promise.resolve();
    }
    m.lastAttempted = json;
    saveState.value = "saving";
    const run = (async () => {
      try {
        await api.saveSettings(snap);
        m.lastSaved = json;
        saveState.value = "saved";
        saveError.value = null;
        savedFlash.value++;
        flashing.value = true;
        clearTimeout(m.flashTimer);
        m.flashTimer = setTimeout(() => (flashing.value = false), SAVED_FADE_MS);
      } catch (e) {
        saveState.value = "failed";
        saveError.value = e;
      } finally {
        m.inFlight = null;
      }
      // Edits made while the request was in flight: save them too (but never retry the same failing payload).
      const now = s.peek();
      if (now && JSON.stringify(now) !== m.lastAttempted) await flush();
    })();
    m.inFlight = run;
    return run;
  }

  // « Réessayer »: send again the payload that failed (flush never retries it by itself).
  function retrySave() {
    m.lastAttempted = "";
    flush();
  }

  // ---- API keys: saved explicitly, one row at a time. ----
  async function writeKey(id: string, key: string, done: "saved" | "deleted"): Promise<boolean> {
    keyBusy.value = { ...keyBusy.value, [id]: true };
    test.value = null;
    m.testGuard.invalidate(); // a test still running used the old key
    clearTimeout(m.feedbackTimers[id]);
    try {
      await api.setApiKey(id, key);
      keyDrafts.value = { ...keyDrafts.value, [id]: "" };
      keyStatus.value = await api.keyStatus();
      keyFeedback.value = { ...keyFeedback.value, [id]: { done } };
      m.feedbackTimers[id] = setTimeout(() => {
        const { [id]: _, ...rest } = keyFeedback.value;
        keyFeedback.value = rest;
      }, 2000);
      return true;
    } catch (e) {
      keyFeedback.value = { ...keyFeedback.value, [id]: { error: e } };
      return false;
    } finally {
      keyBusy.value = { ...keyBusy.value, [id]: false };
    }
  }

  // Never sends an empty key: that is what deleting does, and only after a confirmation.
  async function saveKey(id: string): Promise<boolean> {
    const draft = keyDrafts.value[id];
    if (!canSaveKey(draft) || keyBusy.value[id]) return false;
    return writeKey(id, draft, "saved");
  }

  async function deleteKey(id: string) {
    confirmDelete.value = null;
    await tick();
    m.keyInputs[id]?.focus();
    await writeKey(id, "", "deleted");
  }

  async function cancelDelete(id: string) {
    confirmDelete.value = null;
    await tick();
    m.deleteButtons[id]?.focus();
  }

  const escCancelsDelete = (id: string) => (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelDelete(id);
    }
  };

  // « Ajouter la clé » (banner / inline warning): bring the provider's key field into view.
  async function focusKey(id: string) {
    category.value = "models";
    if (keyRows.value.others.some((p) => p.id === id)) othersOpen.value = true;
    if (confirmDelete.value === id) confirmDelete.value = null;
    await tick();
    m.keyInputs[id]?.scrollIntoView({ block: "center", behavior: "smooth" });
    m.keyInputs[id]?.focus({ preventScroll: true });
  }

  // ---- Provider test: runs on what is displayed, once saved. ----
  useSyncEffect(() => {
    void testSig.value; // a result for other providers/models is stale
    test.value = null;
    m.testGuard.invalidate();
  });

  async function runTest() {
    test.value = null;
    testError.value = null;
    const drafts = unsavedDrafts(keyDrafts.value, providers.value);
    if (unsavedDraftMessage(drafts)) {
      testBlock.value = () => unsavedDraftMessage(drafts) ?? "";
      return;
    }
    testing.value = true;
    try {
      await flush();
      if (saveState.value === "invalid" || saveState.value === "failed") {
        testError.value = { kind: "unsaved" };
        return;
      }
      const token = m.testGuard.token();
      const result = await api.testProviders();
      if (m.testGuard.isCurrent(token)) test.value = result;
    } catch (e) {
      testError.value = { kind: "failed", error: e };
    } finally {
      testing.value = false;
    }
  }

  async function saveDraftsAndTest() {
    testBlock.value = null;
    for (const p of unsavedDrafts(keyDrafts.value, providers.value)) if (!(await saveKey(p.id))) return;
    await runTest();
  }

  // ---- Hotkey capture. ----
  async function capture(role: HotkeyRole) {
    if (!s.value || capturing.value) return;
    capturing.value = role;
    m.captureCancelled = false;
    hotkeyNote.value = null;
    pendingAssign.value = null;
    try {
      const keys = await api.captureKey();
      const outcome = captureOutcome(keys, m.captureCancelled);
      const cur = s.value;
      if (outcome === "timeout") hotkeyNote.value = () => t.value.settings.hotkey.captureTimeout;
      else if (outcome === "keys" && cur) {
        const prev = { trigger_keys: cur.trigger_keys, lock_vk: cur.lock_vk };
        const noteOf = () => assignHotkey(role, keys!, prev).note;
        const a = assignHotkey(role, keys!, prev);
        if (!a.ok) hotkeyNote.value = noteOf;
        else if (a.confirm) pendingAssign.value = { ...a, noteOf };
        else applyKeys({ ...a, noteOf });
      }
    } catch (e) {
      hotkeyNote.value = () => t.value.settings.hotkey.captureFailed(errorText(e));
    } finally {
      m.captureEnded = { role, at: performance.now() };
      capturing.value = null;
    }
  }

  function hotkeyKeydown(e: KeyboardEvent, role: HotkeyRole) {
    if (!swallowsKey(e.key, role, capturing.value, m.captureEnded, performance.now())) return;
    e.preventDefault();
    m.swallowedCode = e.code;
  }

  function hotkeyKeyup(e: KeyboardEvent) {
    if (m.swallowedCode === null || e.code !== m.swallowedCode) return;
    e.preventDefault();
    m.swallowedCode = null;
  }

  function applyKeys(a: Accepted) {
    edit((d) => {
      d.trigger_keys = a.keys.trigger_keys;
      d.lock_vk = a.keys.lock_vk;
    });
    hotkeyNote.value = a.note === null ? null : a.noteOf;
    pendingAssign.value = null;
  }

  async function cancelCapture() {
    m.captureCancelled = true;
    try {
      await api.cancelCapture();
    } catch {
      // The capture still ends on its own after 10 s.
    }
  }

  // A capture must not keep listening while its tab or its category is hidden.
  useEffect(() => {
    if ((!active || category.value !== "dictation") && capturing.value) cancelCapture();
  }, [active, category.value, capturing.value]);

  const tr = t.value.settings;
  const cur = s.value;
  if (!cur) {
    if (loadError.value !== null)
      return (
        <section class={cx(styles.section, styles.loadError)} role="alert">
          <p>{tr.loadError(errorText(loadError.value))}</p>
          <button onClick={load}>{tr.retry}</button>
        </section>
      );
    return (
      <p class={styles.loading} role="status">
        {tr.loading}
      </p>
    );
  }

  const testLines =
    test.value !== null
      ? [
          testLine(roleLabel("transcription"), sttProvider.value?.label ?? cur.stt_provider, cur.stt_model, test.value.stt),
          testLine(roleLabel("correction"), llmProvider.value?.label ?? cur.llm_provider, cur.llm_model, test.value.llm),
        ]
      : [];
  const failure = testError.value?.kind === "failed" ? testFailure(testError.value.error) : null;
  const locked = capturing.value !== null || pendingAssign.value !== null;

  const hotkeyButton = (role: HotkeyRole) => (
    <button
      onClick={() => (capturing.value === role ? cancelCapture() : capture(role))}
      aria-label={
        capturing.value === role ? tr.hotkey.cancelCapture : role === "trigger" ? tr.hotkey.changeTrigger : tr.hotkey.changeLock
      }
      onKeyDown={(e) => hotkeyKeydown(e, role)}
      onKeyUp={hotkeyKeyup}
      disabled={(capturing.value !== null && capturing.value !== role) || pendingAssign.value !== null}
    >
      {capturing.value === role ? tr.cancel : tr.hotkey.change}
    </button>
  );

  const keyLine = (id: string, role: KeyRole) =>
    keyStatus.value[id] ? (
      <p class={cx(styles.keyLine, styles.ok)}>{tr.models.keySaved(labelOf(id))}</p>
    ) : (
      <p class={cx(styles.keyLine, styles.warn)}>
        {missingKeyWarning(labelOf(id), role)}{" "}
        <button type="button" class={styles.link} onClick={() => focusKey(id)}>
          {tr.models.addKey}
        </button>
      </p>
    );

  const keyRow = (p: { id: string; label: string }) => {
    const st = keyState(p.id, cur, keyStatus.value);
    const draft = keyDrafts.value[p.id] ?? "";
    const feedback = keyFeedback.value[p.id];
    return (
      <form
        key={p.id}
        class={styles.key}
        onSubmit={(e) => {
          e.preventDefault();
          saveKey(p.id);
        }}
      >
        <span class={styles.who}>
          <span class={styles.label}>{p.label}</span>
          <span class={cx(styles.status, styles[st])}>{keyStateLabel(st)}</span>
          {usage.value[p.id] && <span class={styles.usage}>{tr.apiKeys.usedFor(rolesText(usage.value[p.id]))}</span>}
        </span>
        {confirmDelete.value === p.id ? (
          <Fragment key="confirm">
            <span class={styles.confirm}>{tr.apiKeys.deleteQuestion(p.label)}</span>
            <span class={styles.actions}>
              <button type="button" class="danger" onClick={() => deleteKey(p.id)} onKeyDown={escCancelsDelete(p.id)}>
                {tr.delete}
              </button>
              <button type="button" onClick={() => cancelDelete(p.id)} onKeyDown={escCancelsDelete(p.id)} ref={autofocus}>
                {tr.cancel}
              </button>
            </span>
          </Fragment>
        ) : (
          <Fragment key="edit">
            <input
              type="password"
              placeholder={keyPlaceholder(keyStatus.value[p.id])}
              aria-label={tr.apiKeys.field(p.label)}
              value={draft}
              onInput={(e) => (keyDrafts.value = { ...keyDrafts.value, [p.id]: e.currentTarget.value })}
              ref={(el) => {
                m.keyInputs[p.id] = el;
              }}
            />
            <span class={styles.actions}>
              <button type="submit" class={cx(canSaveKey(draft) && "primary")} disabled={!canSaveKey(draft) || keyBusy.value[p.id]}>
                {tr.save}
              </button>
              {keyStatus.value[p.id] && (
                <button
                  type="button"
                  class="danger"
                  disabled={keyBusy.value[p.id]}
                  onClick={() => (confirmDelete.value = p.id)}
                  ref={(el) => {
                    m.deleteButtons[p.id] = el;
                  }}
                >
                  {tr.delete}
                </button>
              )}
            </span>
          </Fragment>
        )}
        {feedback && (
          <span class={cx(styles.keyFeedback, "done" in feedback ? styles.ok : styles.error)} role="status">
            {"done" in feedback ? tr.apiKeys[feedback.done] : errorText(feedback.error)}
          </span>
        )}
      </form>
    );
  };

  const numberField = (id: NumberFieldId, value: number, set: (d: SettingsT, n: number) => void) => (
    <NumberField id={id} value={value} invalid={numberIssue.value?.field === id} onCommit={(n) => edit((d) => set(d, n))} />
  );

  const pane = (id: Category, children: ComponentChildren) => (
    <div class={styles.pane} hidden={category.value !== id} aria-labelledby={`settings-${id}`} role="region">
      <h2 class={styles.paneTitle} id={`settings-${id}`}>
        {tr.nav[id]}
      </h2>
      {children}
    </div>
  );

  const sum = pendingAssign.value
    ? pendingSummary(pendingAssign.value.keys, { trigger_keys: cur.trigger_keys, lock_vk: cur.lock_vk })
    : null;

  return (
    <>
      {banner.value.length > 0 && (
        <div class={styles.banner} role="note">
          {tr.banner.intro}{" "}
          {listParts(banner.value.map((_, i) => String(i))).map((part, i) => {
            if (part.type === "literal") return part.value;
            const n = Number(part.value);
            return (
              <Fragment key={i}>
                <button type="button" class={styles.link} onClick={() => focusKey(missing.value[n].provider)}>
                  {banner.value[n].label}
                </button>{" "}
                ({banner.value[n].roles})
              </Fragment>
            );
          })}
          .
        </div>
      )}

      <div class={styles.layout}>
        <SettingsNav current={category.value} attention={attention.value} onSelect={(c) => (category.value = c)} />

        {pane(
          "general",
          <>
            <section class={styles.section}>
              <h3 class={styles.heading}>{tr.language.heading}</h3>
              <label class={styles.row}>
                {tr.language.label}
                <select
                  class={styles.short}
                  value={cur.language}
                  onChange={(e) => edit((d) => (d.language = e.currentTarget.value as LanguagePref))}
                >
                  {languageOptions().map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            </section>
            <Startup />
            <Updates />
          </>,
        )}

        {pane(
          "dictation",
          <>
            <section class={styles.section}>
              <h3 class={styles.heading}>{tr.hotkey.heading}</h3>
              <p class={styles.help}>{tr.hotkey.help}</p>
              <div class={styles.hotkeys}>
                <div class={styles.hotkey}>
                  <span class={styles.label}>{tr.hotkey.trigger}</span>
                  <strong>{capturing.value === "trigger" ? tr.hotkey.pressCombination : chordName(cur.trigger_keys)}</strong>
                  {hotkeyButton("trigger")}
                </div>
                <label class={cx(styles.row, styles.check)}>
                  <input
                    type="checkbox"
                    checked={cur.gesture.double_tap_enabled}
                    onChange={(e) => edit((d) => (d.gesture.double_tap_enabled = e.currentTarget.checked))}
                  />{" "}
                  {tr.hotkey.doubleTap}
                </label>
                <div class={styles.hotkey}>
                  <span class={styles.label}>{tr.hotkey.lockRow}</span>
                  <strong>{capturing.value === "lock" ? tr.hotkey.pressKey : keyName(cur.lock_vk)}</strong>
                  {hotkeyButton("lock")}
                  <button
                    onClick={() => edit((d) => (d.lock_vk = 0))}
                    aria-label={tr.hotkey.disableLock}
                    disabled={cur.lock_vk === 0 || locked}
                  >
                    {tr.hotkey.disable}
                  </button>
                </div>
              </div>
              {pendingAssign.value && sum && (
                <p class={styles.inlineConfirm} role="status">
                  <span>
                    {tr.hotkey.trigger} = <strong>{sum.trigger}</strong>
                    {sum.lock && (
                      <>
                        , {tr.hotkey.lock} = <strong>{sum.lock}</strong>
                      </>
                    )}
                    . {tr.hotkey.typingKeyWarning}
                  </span>
                  <button class="primary" onClick={() => applyKeys(pendingAssign.value!)}>
                    {tr.hotkey.continue}
                  </button>
                  <button onClick={() => (pendingAssign.value = null)} ref={autofocus}>
                    {tr.cancel}
                  </button>
                </p>
              )}
              {hotkeyNote.value && (
                <p class={styles.note} role="status">
                  {hotkeyNote.value()}
                </p>
              )}
            </section>

            <section class={styles.section}>
              <h3 class={styles.heading}>{tr.sound.heading}</h3>
              <label class={cx(styles.row, styles.check)}>
                <input
                  type="checkbox"
                  checked={cur.mute_audio_during_dictation}
                  onChange={(e) => edit((d) => (d.mute_audio_during_dictation = e.currentTarget.checked))}
                />{" "}
                {tr.sound.mute}
              </label>
              <label class={cx(styles.row, styles.check)}>
                <input
                  type="checkbox"
                  checked={cur.exclusive_microphone_during_dictation}
                  onChange={(e) => edit((d) => (d.exclusive_microphone_during_dictation = e.currentTarget.checked))}
                  aria-describedby="exclusive-mic-note"
                />{" "}
                {tr.sound.exclusiveMic}
              </label>
              <p class={styles.note} id="exclusive-mic-note">
                {tr.sound.exclusiveMicNote}
              </p>
            </section>

            <section class={styles.section}>
              <h3 class={styles.heading}>{tr.correction.heading}</h3>
              <label class={styles.row}>
                {tr.correction.level}
                <select
                  value={cur.level}
                  onChange={(e) => edit((d) => (d.level = e.currentTarget.value as SettingsT["level"]))}
                >
                  {levelOptions().map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            </section>
          </>,
        )}

        {pane(
          "models",
          <>
            <section class={styles.section}>
              <h3 class={styles.heading}>{tr.models.heading}</h3>
              <h4 class={styles.subheading}>{tr.roles.transcription}</h4>
              <label class={styles.row}>
                {tr.models.provider}
                <select value={cur.stt_provider} onChange={(e) => onSttProvider(e.currentTarget.value)}>
                  {sttProviders.value.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </label>
              {keyLine(cur.stt_provider, "transcription")}
              <label class={styles.row} key={`stt-${cur.stt_provider}`}>
                {tr.models.model}{" "}
                <ModelPicker
                  models={sttProvider.value?.stt_models ?? []}
                  value={cur.stt_model}
                  onValueChange={(v) => edit((d) => (d.stt_model = v))}
                />
              </label>

              <h4 class={styles.subheading}>{tr.roles.correction}</h4>
              {raw.value && <p class={cx(styles.note, styles.indent)}>{tr.models.rawNote}</p>}
              <fieldset disabled={raw.value} class={cx(styles.fieldset, raw.value && styles.off)}>
                <label class={styles.row}>
                  {tr.models.provider}
                  <select value={cur.llm_provider} onChange={(e) => onLlmProvider(e.currentTarget.value)}>
                    {llmProviders.value.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.label}
                      </option>
                    ))}
                  </select>
                </label>
                {!raw.value && keyLine(cur.llm_provider, "correction")}
                <label class={styles.row} key={`llm-${cur.llm_provider}`}>
                  {tr.models.model}{" "}
                  <ModelPicker
                    models={llmProvider.value?.llm_models.map((x) => x.id) ?? []}
                    value={cur.llm_model}
                    onValueChange={(v) => edit((d) => (d.llm_model = v))}
                    onChange={() => edit(syncEffortOf)}
                  />
                </label>
                {efforts.value.length > 0 && (
                  <label class={styles.row}>
                    {tr.models.effort}
                    <select value={cur.llm_effort} onChange={(e) => edit((d) => (d.llm_effort = e.currentTarget.value))}>
                      {efforts.value.map((x) => (
                        <option key={x} value={x}>
                          {effortLabel(x)}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
              </fieldset>

              <div class={styles.test}>
                <button onClick={runTest} disabled={testing.value}>
                  {testing.value ? tr.test.running : raw.value ? tr.test.runTranscription : tr.test.run}
                </button>
                {testBlock.value && (
                  <p class={styles.inlineConfirm} role="status">
                    {testBlock.value()}
                    <button class="primary" onClick={saveDraftsAndTest}>
                      {tr.save}
                    </button>
                    <button onClick={() => (testBlock.value = null)}>{tr.cancel}</button>
                  </p>
                )}
                <div class={styles.testResult} role="status">
                  {testLines.map((l, i) => (
                    <Fragment key={i}>
                      <p class={l.ok ? styles.ok : styles.fail}>
                        <span class={styles.title}>{l.title}</span>
                        {tr.colon}
                        {l.text}
                      </p>
                      {l.raw && (
                        <details>
                          <summary>{tr.details}</summary>
                          <pre class={styles.pre}>{l.raw}</pre>
                        </details>
                      )}
                    </Fragment>
                  ))}
                  {testError.value?.kind === "unsaved" && <p class={styles.fail}>{tr.test.notSaved}</p>}
                  {failure && (
                    <>
                      <p class={styles.fail}>{failure.text}</p>
                      {failure.raw && (
                        <details>
                          <summary>{tr.details}</summary>
                          <pre class={styles.pre}>{failure.raw}</pre>
                        </details>
                      )}
                    </>
                  )}
                </div>
              </div>
            </section>

            <section class={styles.section}>
              <h3 class={styles.heading}>{tr.apiKeys.heading}</h3>
              <p class={styles.help}>{keysHelp()}</p>
              {keyRows.value.used.map(keyRow)}
              {keyRows.value.others.length > 0 && (
                <details
                  class={styles.others}
                  open={othersOpen.value}
                  onToggle={(e) => (othersOpen.value = (e.currentTarget as HTMLDetailsElement).open)}
                >
                  <summary>{tr.apiKeys.others}</summary>
                  {keyRows.value.others.map(keyRow)}
                </details>
              )}
            </section>
          </>,
        )}

        {pane(
          "advanced",
          <section class={styles.section}>
            {numberField("hold_threshold_ms", cur.gesture.hold_threshold_ms, (d, n) => (d.gesture.hold_threshold_ms = n))}
            {numberField("double_tap_window_ms", cur.gesture.double_tap_window_ms, (d, n) => (d.gesture.double_tap_window_ms = n))}
            {numberField("restore_delay_ms", cur.restore_delay_ms, (d, n) => (d.restore_delay_ms = n))}
            <label class={cx(styles.row, styles.field)}>
              <span>{fieldText("audio_retention_days").label}</span>
              <select
                class={styles.short}
                value={String(cur.audio_retention_days)}
                onChange={(e) => edit((d) => (d.audio_retention_days = Number(e.currentTarget.value)))}
              >
                {retentionOptions(cur.audio_retention_days).map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
            </label>
            {numberField("max_recording_min", cur.max_recording_ms / 60000, (d, n) => (d.max_recording_ms = n * 60000))}
            {numberField("llm_timeout_s", cur.llm_timeout_base_ms / 1000, (d, n) => (d.llm_timeout_base_ms = n * 1000))}
          </section>,
        )}
      </div>

      <SaveStatusBar view={bar.value} savedFlash={savedFlash.value} onRetry={retrySave} />
    </>
  );
}
