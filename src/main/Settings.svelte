<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, effortLevels, type Provider, type ProviderTest, type Settings } from "../lib/api";
  import { keyName } from "../lib/keys";
  import { editAction, testSignature, type SaveState } from "../lib/autosave";
  import { checkNumbers, formLabel, NUMBER_FIELDS, parseNumber, type NumberFieldId } from "../lib/validation";
  import { assignHotkey, captureOutcome, CAPTURE_TIMEOUT_MESSAGE, TYPING_KEY_WARNING, type Assignment, type HotkeyRole } from "../lib/hotkeys";
  import { canSaveKey, deleteKeyQuestion, keyPlaceholder, keyState, keyUsage, missingKeys, unsavedDraftMessage, unsavedDrafts } from "../lib/apiKeys";
  import ModelPicker from "./ModelPicker.svelte";

  // App keeps this view mounted while another tab is shown (so nothing typed here is lost)
  // and reads `status` to flag unsaved settings on the tab.
  let { active = true, status = $bindable("saved") }: { active?: boolean; status?: SaveState } = $props();

  let s = $state<Settings | null>(null);
  let loadError = $state<string | null>(null);
  let providers = $state<Provider[]>([]);
  let sttProviders = $derived(providers.filter((p) => p.stt_models.length > 0));
  let llmProviders = $derived(providers.filter((p) => p.llm_api !== null));
  let sttProvider = $derived(providers.find((p) => p.id === s?.stt_provider));
  let llmProvider = $derived(providers.find((p) => p.id === s?.llm_provider));
  let efforts = $derived(s ? effortLevels(llmProvider, s.llm_model) : []);
  let keyStatus = $state<Record<string, boolean>>({});
  let keyDrafts = $state<Record<string, string>>({});
  let error = $state<string | null>(null);

  // Keys the current providers and level need (for the missing-key banner and the per-key status).
  let usage = $derived(s ? keyUsage(s) : {});
  let missing = $derived(s ? missingKeys(s, keyStatus) : []);
  const keyStateOf = (id: string) => (s ? keyState(id, s, keyStatus) : "unconfigured");

  // Autosave: every change is persisted ~500 ms after the last edit; the pill shows the outcome.
  // Number fields and the hand-typed model id only change `s` when committed (blur or Entrée).
  let saveState = $state<SaveState>("saved");
  let saveError = $state<string | null>(null);
  let savedFlash = $state(0);
  let lastSaved = "";
  let lastAttempted = "";
  let inFlight: Promise<void> | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let numberIssue = $derived(s ? checkNumbers(s) : null);
  $effect(() => {
    status = saveState;
  });

  async function load() {
    loadError = null;
    try {
      const [st, pr, ks] = await Promise.all([api.getSettings(), api.providers(), api.keyStatus()]);
      lastSaved = lastAttempted = JSON.stringify(st);
      providers = pr;
      keyStatus = ks;
      s = st;
    } catch (e) {
      loadError = String(e);
    }
  }
  onMount(load);

  $effect(() => {
    if (!s) return;
    const json = JSON.stringify($state.snapshot(s)); // deep read: any field change re-runs this
    const action = editAction(json, lastSaved, inFlight !== null);
    if (action === "ignore") return;
    clearTimeout(timer);
    if (action === "revert") {
      saveState = "saved";
      saveError = null;
      return;
    }
    saveState = "pending";
    timer = setTimeout(flush, 500);
  });

  // Safety net (App keeps the view mounted): never drop the last edit.
  onDestroy(() => {
    if (saveState === "pending") flush();
    if (capturing) cancelCapture();
  });

  // Switching provider selects its newest model.
  function onSttProvider() {
    if (s && sttProvider) s.stt_model = sttProvider.stt_models[0];
  }

  function onLlmProvider() {
    if (s && llmProvider) s.llm_model = llmProvider.llm_models[0].id;
    syncEffort();
  }

  function syncEffort() {
    if (!s) return;
    const levels = effortLevels(llmProvider, s.llm_model);
    if (levels.length > 0 && !levels.includes(s.llm_effort)) s.llm_effort = levels[0];
  }

  function flush(): Promise<void> {
    clearTimeout(timer);
    if (!s) return Promise.resolve();
    if (inFlight) return inFlight; // its follow-up saves what changed meanwhile
    const snap = $state.snapshot(s) as Settings;
    const json = JSON.stringify(snap);
    if (json === lastSaved) {
      saveState = "saved";
      saveError = null;
      return Promise.resolve();
    }
    snap.max_recording_ms = Math.round(snap.max_recording_ms);
    const invalid = checkNumbers(snap);
    if (invalid) {
      saveState = "invalid";
      saveError = invalid.message;
      return Promise.resolve();
    }
    lastAttempted = json;
    saveState = "saving";
    const run = (async () => {
      try {
        await api.saveSettings(snap);
        lastSaved = json;
        saveState = "saved";
        saveError = null;
        savedFlash++;
      } catch (e) {
        saveState = "failed";
        saveError = String(e);
      } finally {
        inFlight = null;
      }
      // Edits made while the request was in flight: save them too (but never retry the same failing payload).
      if (s && JSON.stringify($state.snapshot(s)) !== lastAttempted) await flush();
    })();
    inFlight = run;
    return run;
  }

  // ---- API keys: saved explicitly, one row at a time. ----
  let keyBusy = $state<Record<string, boolean>>({});
  let keyFeedback = $state<Record<string, { ok: boolean; text: string }>>({});
  let confirmDelete = $state<string | null>(null);
  const feedbackTimers: Record<string, ReturnType<typeof setTimeout>> = {};

  async function writeKey(id: string, key: string, done: string): Promise<boolean> {
    keyBusy[id] = true;
    test = null;
    clearTimeout(feedbackTimers[id]);
    try {
      await api.setApiKey(id, key);
      keyDrafts[id] = "";
      keyStatus = await api.keyStatus();
      keyFeedback[id] = { ok: true, text: done };
      feedbackTimers[id] = setTimeout(() => delete keyFeedback[id], 2000);
      return true;
    } catch (e) {
      keyFeedback[id] = { ok: false, text: String(e) };
      return false;
    } finally {
      keyBusy[id] = false;
    }
  }

  // Never sends an empty key: that is what deleting does, and only after a confirmation.
  async function saveKey(id: string): Promise<boolean> {
    if (!canSaveKey(keyDrafts[id]) || keyBusy[id]) return false;
    return writeKey(id, keyDrafts[id], "✓ Clé enregistrée");
  }

  async function deleteKey(id: string) {
    confirmDelete = null;
    await writeKey(id, "", "✓ Clé supprimée");
  }

  const autofocus = (node: HTMLElement) => node.focus();

  // ---- Provider test: runs on what is displayed, once saved. ----
  let test = $state<ProviderTest | null>(null);
  let testing = $state(false);
  let testBlock = $state<string | null>(null);
  let testSig = $derived(s ? testSignature(s) : "");
  $effect(() => {
    void testSig; // a result for other providers/models is stale
    test = null;
  });

  async function runTest() {
    test = null;
    error = null;
    testBlock = unsavedDraftMessage(unsavedDrafts(keyDrafts, providers));
    if (testBlock) return;
    testing = true;
    try {
      await flush();
      if (saveState === "invalid" || saveState === "failed") {
        error = "Réglages non enregistrés : corrigez l'erreur avant de tester.";
        return;
      }
      const sig = testSig;
      const result = await api.testProviders();
      if (sig === testSig) test = result;
    } catch (e) {
      error = String(e);
    } finally {
      testing = false;
    }
  }

  async function saveDraftsAndTest() {
    testBlock = null;
    for (const p of unsavedDrafts(keyDrafts, providers)) if (!(await saveKey(p.id))) return;
    await runTest();
  }

  // ---- Hotkey capture. ----
  let capturing = $state<HotkeyRole | null>(null);
  let captureCancelled = false;
  let hotkeyNote = $state<string | null>(null);
  let pendingAssign = $state<Extract<Assignment, { ok: true }> | null>(null);

  async function capture(role: HotkeyRole) {
    if (!s || capturing) return;
    capturing = role;
    captureCancelled = false;
    hotkeyNote = null;
    pendingAssign = null;
    try {
      const vk = await api.captureKey();
      const outcome = captureOutcome(vk, captureCancelled);
      if (outcome === "timeout") hotkeyNote = CAPTURE_TIMEOUT_MESSAGE;
      else if (outcome === "key" && s) {
        const a = assignHotkey(role, vk!, { trigger_vk: s.trigger_vk, lock_vk: s.lock_vk });
        if (!a.ok) hotkeyNote = a.note;
        else if (a.confirm) pendingAssign = a;
        else applyKeys(a);
      }
    } catch (e) {
      hotkeyNote = `Capture impossible : ${e}`;
    } finally {
      capturing = null;
    }
  }

  function applyKeys(a: Extract<Assignment, { ok: true }>) {
    if (!s) return;
    s.trigger_vk = a.keys.trigger_vk;
    s.lock_vk = a.keys.lock_vk;
    hotkeyNote = a.note;
    pendingAssign = null;
  }

  async function cancelCapture() {
    captureCancelled = true;
    try {
      await api.cancelCapture();
    } catch {
      // The capture still ends on its own after 10 s.
    }
  }

  // A capture must not keep listening while its tab is hidden.
  $effect(() => {
    if (!active && capturing) cancelCapture();
  });

  const show = (r: { Ok: string } | { Err: string }) => ("Ok" in r ? `✓ ${r.Ok}` : `✗ ${r.Err}`);
</script>

{#snippet numberField(id: NumberFieldId, value: number, set: (n: number) => void)}
  {@const f = NUMBER_FIELDS[id]}
  <label>{formLabel(f)}
    <input type="number" min={f.min} max={f.max} step={f.integer ? 1 : "any"} {value} aria-invalid={numberIssue?.field === id}
      onchange={(e) => set(parseNumber(e.currentTarget.value))} />
  </label>
{/snippet}

{#snippet hotkeyButton(role: HotkeyRole)}
  <button onclick={() => (capturing === role ? cancelCapture() : capture(role))} disabled={(capturing !== null && capturing !== role) || pendingAssign !== null}>
    {capturing === role ? "Annuler" : "Changer"}
  </button>
{/snippet}

{#if s}
  <section>
    <h2>Clés API</h2>
    {#each providers as p (p.id)}
      <form class="key" onsubmit={(e) => { e.preventDefault(); saveKey(p.id); }}>
        <span class="label">{p.label}</span>
        <span class="status">{keyStatus[p.id] ? "✓ enregistrée" : "✗ absente"}</span>
        {#if confirmDelete === p.id}
          <span class="confirm">{deleteKeyQuestion(p.label)}</span>
          <span class="actions">
            <button type="button" class="danger" onclick={() => deleteKey(p.id)}>Supprimer</button>
            <button type="button" onclick={() => (confirmDelete = null)} use:autofocus>Annuler</button>
          </span>
        {:else}
          <input type="password" placeholder={keyPlaceholder(keyStatus[p.id])} bind:value={keyDrafts[p.id]} />
          <span class="actions">
            <button type="submit" class:primary={canSaveKey(keyDrafts[p.id])} disabled={!canSaveKey(keyDrafts[p.id]) || keyBusy[p.id]}>Enregistrer</button>
            {#if keyStatus[p.id]}
              <button type="button" class="danger" disabled={keyBusy[p.id]} onclick={() => (confirmDelete = p.id)}>Supprimer</button>
            {/if}
          </span>
        {/if}
        {#if keyFeedback[p.id]}
          <span class="key-feedback" class:ok={keyFeedback[p.id].ok} class:error={!keyFeedback[p.id].ok} role="status">{keyFeedback[p.id].text}</span>
        {/if}
      </form>
    {/each}
    <button onclick={runTest} disabled={testing}>{testing ? "Test…" : "Tester la configuration"}</button>
    {#if testBlock}
      <p class="inline-confirm" role="status">
        {testBlock}
        <button class="primary" onclick={saveDraftsAndTest}>Enregistrer</button>
        <button onclick={() => (testBlock = null)}>Annuler</button>
      </p>
    {/if}
    {#if test}<p>Transcription : {show(test.stt)} — Correction : {show(test.llm)}</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}
  </section>

  <section>
    <h2>Transcription</h2>
    <label>Fournisseur
      <select bind:value={s.stt_provider} onchange={onSttProvider}>
        {#each sttProviders as p}<option value={p.id}>{p.label}</option>{/each}
      </select>
    </label>
    {#key s.stt_provider}
      <label>Modèle <ModelPicker models={sttProvider?.stt_models ?? []} bind:value={s.stt_model} /></label>
    {/key}
  </section>

  <section>
    <h2>Correction</h2>
    <label>Niveau
      <select bind:value={s.level}>
        <option value="raw">Brut (aucune correction)</option>
        <option value="clean">Nettoyé (vocabulaire, ponctuation, hésitations)</option>
        <option value="formatted">Mis en forme selon l'application</option>
      </select>
    </label>
    <label>Fournisseur
      <select bind:value={s.llm_provider} onchange={onLlmProvider}>
        {#each llmProviders as p}<option value={p.id}>{p.label}</option>{/each}
      </select>
    </label>
    {#key s.llm_provider}
      <label>Modèle <ModelPicker models={llmProvider?.llm_models.map((m) => m.id) ?? []} bind:value={s.llm_model} onchange={syncEffort} /></label>
    {/key}
    {#if efforts.length > 0}
      <label>Effort
        <select bind:value={s.llm_effort}>
          {#each efforts as e}<option value={e}>{e}</option>{/each}
        </select>
      </label>
    {/if}
  </section>

  <section>
    <h2>Raccourci</h2>
    <div class="key hotkey">
      <span class="label">Déclenchement</span>
      <strong>{capturing === "trigger" ? "Appuyez sur une touche… (Échap pour annuler)" : keyName(s.trigger_vk)}</strong>
      {@render hotkeyButton("trigger")}
    </div>
    <div class="key hotkey">
      <span class="label">Verrouillage (maintenir + touche)</span>
      <strong>{capturing === "lock" ? "Appuyez sur une touche… (Échap pour annuler)" : keyName(s.lock_vk)}</strong>
      {@render hotkeyButton("lock")}
      <button onclick={() => (s!.lock_vk = 0)} disabled={capturing !== null || pendingAssign !== null}>Aucune</button>
    </div>
    {#if pendingAssign}
      <p class="inline-confirm" role="status">
        <span>Déclenchement = <strong>{keyName(pendingAssign.keys.trigger_vk)}</strong>. {TYPING_KEY_WARNING}</span>
        <button class="primary" onclick={() => applyKeys(pendingAssign!)}>Continuer</button>
        <button onclick={() => (pendingAssign = null)} use:autofocus>Annuler</button>
      </p>
    {/if}
    {#if hotkeyNote}<p class="note" role="status">{hotkeyNote}</p>{/if}
    <label><input type="checkbox" bind:checked={s.gesture.double_tap_enabled} /> Double-tap pour verrouiller</label>
    <label><input type="checkbox" bind:checked={s.gesture.lock_key_enabled} /> Touche de verrouillage active</label>
    {@render numberField("hold_threshold_ms", s.gesture.hold_threshold_ms, (n) => (s!.gesture.hold_threshold_ms = n))}
    {@render numberField("double_tap_window_ms", s.gesture.double_tap_window_ms, (n) => (s!.gesture.double_tap_window_ms = n))}
  </section>

  <section>
    <h2>Avancé</h2>
    {@render numberField("restore_delay_ms", s.restore_delay_ms, (n) => (s!.restore_delay_ms = n))}
    {@render numberField("audio_retention_days", s.audio_retention_days, (n) => (s!.audio_retention_days = n))}
    {@render numberField("max_recording_min", s.max_recording_ms / 60000, (n) => (s!.max_recording_ms = n * 60000))}
  </section>

  <div class="save-status {saveState}" role="status" aria-live="polite">
    {#if saveState === "pending" || saveState === "saving"}
      <span class="spinner"></span> Enregistrement…
    {:else if saveState === "saved"}
      {#key savedFlash}<span class="check">✓</span>{/key} Réglages enregistrés
    {:else}
      ⚠ Non enregistré : {saveError}
    {/if}
  </div>
{:else if loadError}
  <section class="load-error" role="alert">
    <p>Impossible de charger les réglages : {loadError}</p>
    <button onclick={load}>Réessayer</button>
  </section>
{:else}
  <p class="loading" role="status">Chargement…</p>
{/if}

<style>
  section { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin-bottom: 12px; }
  h2 { font-size: 15px; margin: 0 0 10px; }
  /* Two columns everywhere: label text, then the control stretched over the rest of the row. */
  label { display: grid; grid-template-columns: 220px minmax(0, 1fr); align-items: center; column-gap: 12px; margin: 8px 0; font-size: 14px; }
  label > input:not([type="checkbox"]), label > select { width: 100%; box-sizing: border-box; }
  label:has(> input[type="checkbox"]) { display: flex; gap: 8px; padding-left: 232px; }
  .key { display: grid; grid-template-columns: 220px 110px minmax(0, 1fr) auto; align-items: center; column-gap: 12px; margin: 8px 0; font-size: 14px; }
  .key.hotkey { grid-template-columns: 220px minmax(0, 1fr) auto auto; }
  .key input { width: 100%; box-sizing: border-box; }
  .actions button:disabled, .key.hotkey button:disabled { opacity: 0.5; cursor: not-allowed; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .confirm { color: var(--danger); font-weight: 600; }
  .key-feedback { grid-column: 2 / -1; font-size: 13px; margin-top: 4px; }
  .key-feedback.ok { color: #16a34a; }
  .status { color: var(--muted); }
  :global(input[aria-invalid="true"]) { border-color: var(--danger); outline-color: var(--danger); }
  .inline-confirm { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; font-size: 14px; margin: 8px 0; }
  .note { font-size: 13px; color: var(--muted); margin: 4px 0 8px; }
  @media (max-width: 640px) {
    label, .key { grid-template-columns: 1fr; row-gap: 4px; }
    label:has(> input[type="checkbox"]) { padding-left: 0; }
    .key-feedback { grid-column: 1; }
    .actions { justify-content: flex-start; }
  }
  .error { color: var(--danger); }
  .loading { color: var(--muted); font-size: 14px; }
  .load-error p { margin: 0 0 10px; color: var(--danger); font-size: 14px; }
  .save-status { position: fixed; right: 16px; bottom: 16px; display: flex; align-items: center; gap: 8px; max-width: min(520px, calc(100vw - 32px));
    padding: 8px 14px; border-radius: 999px; font-size: 13px; background: var(--card); border: 1px solid var(--border);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.12); color: var(--muted); transition: color 0.2s, border-color 0.2s; }
  .save-status.saved { color: #16a34a; border-color: #86efac; }
  .save-status.invalid, .save-status.failed { color: var(--danger); border-color: var(--danger); border-radius: 12px; }
  .check { display: inline-block; font-weight: 700; animation: pop 0.45s ease-out; }
  .spinner { width: 12px; height: 12px; border: 2px solid currentColor; border-top-color: transparent; border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes pop { 0% { transform: scale(0.4); opacity: 0; } 60% { transform: scale(1.35); opacity: 1; } 100% { transform: scale(1); } }
  @keyframes spin { to { transform: rotate(360deg); } }
  section:last-of-type { margin-bottom: 64px; }
</style>
