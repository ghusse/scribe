<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { api, effortLevels, type Provider, type ProviderTest, type Settings } from "../lib/api";
  import { chordName, keyName } from "../lib/keys";
  import { editAction, SAVED_FADE_MS, staleGuard, statusView, testSignature, type SaveState } from "../lib/autosave";
  import { checkNumbers, fieldHelp, NUMBER_FIELDS, parseNumber, retentionOptions, type NumberFieldId } from "../lib/validation";
  import { assignHotkey, captureOutcome, CAPTURE_TIMEOUT_MESSAGE, normalizeLock, pendingSummary, swallowsKey, TYPING_KEY_WARNING, type Assignment, type HotkeyRole } from "../lib/hotkeys";
  import { canSaveKey, deleteKeyQuestion, KEY_STATE_LABELS, keyPlaceholder, keyState, keyUsage, missingKeys, missingKeyWarning, rolesText, setupBannerParts, splitProviders, unsavedDraftMessage, unsavedDrafts, type KeyRole } from "../lib/apiKeys";
  import { testLine, translateProviderError } from "../lib/providerTest";
  import { effortLabel, HOTKEY_HELP, KEYS_HELP, LEVEL_OPTIONS, RAW_LEVEL_NOTE } from "../lib/settingsCopy";
  import ModelPicker from "./ModelPicker.svelte";
  import Startup from "./Startup.svelte";

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
  let keyRows = $derived(splitProviders(providers, usage));
  const labelOf = (id: string) => providers.find((p) => p.id === id)?.label ?? id;
  let banner = $derived(setupBannerParts(missing, labelOf));
  let raw = $derived(s?.level === "raw");

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
  // « ✓ Réglages enregistrés » fades out after SAVED_FADE_MS; errors stay until fixed.
  let flashing = $state(false);
  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  let showDetails = $state(false);
  let bar = $derived(statusView(saveState, saveError, flashing));
  $effect(() => {
    status = saveState;
  });

  async function load() {
    loadError = null;
    try {
      const [st, pr, ks] = await Promise.all([api.getSettings(), api.providers(), api.keyStatus()]);
      const shown = normalizeLock(st); // before lastSaved: showing it is not an edit
      lastSaved = lastAttempted = JSON.stringify(shown);
      providers = pr;
      keyStatus = ks;
      s = shown;
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
        flashing = true;
        clearTimeout(flashTimer);
        flashTimer = setTimeout(() => (flashing = false), SAVED_FADE_MS);
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

  // « Réessayer »: send again the payload that failed (flush never retries it by itself).
  function retrySave() {
    lastAttempted = "";
    showDetails = false;
    flush();
  }

  // ---- API keys: saved explicitly, one row at a time. ----
  let keyBusy = $state<Record<string, boolean>>({});
  let keyFeedback = $state<Record<string, { ok: boolean; text: string }>>({});
  let confirmDelete = $state<string | null>(null);
  const feedbackTimers: Record<string, ReturnType<typeof setTimeout>> = {};

  async function writeKey(id: string, key: string, done: string): Promise<boolean> {
    keyBusy[id] = true;
    test = null;
    testGuard.invalidate(); // a test still running used the old key
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

  // The row's input / « Supprimer » button, to put the focus back when the confirmation closes.
  let keyInputs = $state<Record<string, HTMLInputElement>>({});
  let deleteButtons = $state<Record<string, HTMLButtonElement>>({});

  async function deleteKey(id: string) {
    confirmDelete = null;
    await tick();
    keyInputs[id]?.focus();
    await writeKey(id, "", "✓ Clé supprimée");
  }

  async function cancelDelete(id: string) {
    confirmDelete = null;
    await tick();
    deleteButtons[id]?.focus();
  }

  const escCancelsDelete = (id: string) => (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelDelete(id);
    }
  };

  const autofocus = (node: HTMLElement) => node.focus();

  // « Ajouter la clé » (banner / inline warning): bring the provider's key field into view.
  let othersOpen = $state(false);
  async function focusKey(id: string) {
    if (keyRows.others.some((p) => p.id === id)) othersOpen = true;
    if (confirmDelete === id) confirmDelete = null;
    await tick();
    keyInputs[id]?.scrollIntoView({ block: "center", behavior: "smooth" });
    keyInputs[id]?.focus({ preventScroll: true });
  }

  // ---- Provider test: runs on what is displayed, once saved. ----
  let test = $state<ProviderTest | null>(null);
  let testing = $state(false);
  let testBlock = $state<string | null>(null);
  let testSig = $derived(s ? testSignature(s) : "");
  const testGuard = staleGuard();
  $effect(() => {
    void testSig; // a result for other providers/models is stale
    test = null;
    testGuard.invalidate();
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
        error = "Réglages non enregistrés\u00a0: corrigez l'erreur avant de tester.";
        return;
      }
      const token = testGuard.token();
      const result = await api.testProviders();
      if (testGuard.isCurrent(token)) test = result;
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
      const keys = await api.captureKey();
      const outcome = captureOutcome(keys, captureCancelled);
      if (outcome === "timeout") hotkeyNote = CAPTURE_TIMEOUT_MESSAGE;
      else if (outcome === "keys" && s) {
        const a = assignHotkey(role, keys!, { trigger_keys: s.trigger_keys, lock_vk: s.lock_vk });
        if (!a.ok) hotkeyNote = a.note;
        else if (a.confirm) pendingAssign = a;
        else applyKeys(a);
      }
    } catch (e) {
      hotkeyNote = `Capture impossible\u00a0: ${e}`;
    } finally {
      captureEnded = { role, at: performance.now() };
      capturing = null;
    }
  }

  // The captured key also reaches the focused « Annuler » button: Space/Enter must not activate it
  // (that would cancel the capture or start another one). Its keyup is swallowed too.
  let captureEnded: { role: HotkeyRole; at: number } | null = null;
  let swallowedCode: string | null = null;

  function hotkeyKeydown(e: KeyboardEvent, role: HotkeyRole) {
    if (!swallowsKey(e.key, role, capturing, captureEnded, performance.now())) return;
    e.preventDefault();
    swallowedCode = e.code;
  }

  function hotkeyKeyup(e: KeyboardEvent) {
    if (swallowedCode === null || e.code !== swallowedCode) return;
    e.preventDefault();
    swallowedCode = null;
  }

  function applyKeys(a: Extract<Assignment, { ok: true }>) {
    if (!s) return;
    s.trigger_keys = a.keys.trigger_keys;
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

  let testLines = $derived(
    test && s
      ? [
          testLine("Transcription", sttProvider?.label ?? s.stt_provider, s.stt_model, test.stt),
          testLine("Correction", llmProvider?.label ?? s.llm_provider, s.llm_model, test.llm),
        ]
      : [],
  );
  let errorSummary = $derived(error ? translateProviderError(error, null) : null);
</script>

{#snippet numberField(id: NumberFieldId, value: number, set: (n: number) => void)}
  {@const f = NUMBER_FIELDS[id]}
  <label class="field">
    <span>{f.label}</span>
    <span class="num">
      <input type="number" min={f.min} max={f.max} step="1" {value} aria-invalid={numberIssue?.field === id} aria-describedby="help-{id}"
        onchange={(e) => set(parseNumber(e.currentTarget.value))} />
      <span class="unit">{f.unit}</span>
    </span>
    <span class="help" id="help-{id}">{fieldHelp(f)}</span>
  </label>
{/snippet}

{#snippet hotkeyButton(role: HotkeyRole)}
  <button onclick={() => (capturing === role ? cancelCapture() : capture(role))}
    aria-label={capturing === role ? "Annuler la capture" : role === "trigger" ? "Changer le raccourci de déclenchement" : "Changer la touche de verrouillage"}
    onkeydown={(e) => hotkeyKeydown(e, role)} onkeyup={hotkeyKeyup} disabled={(capturing !== null && capturing !== role) || pendingAssign !== null}>
    {capturing === role ? "Annuler" : "Changer"}
  </button>
{/snippet}

{#snippet keyLine(id: string, role: KeyRole)}
  {#if keyStatus[id]}
    <p class="key-line ok">✓ Clé {labelOf(id)} enregistrée</p>
  {:else}
    <p class="key-line warn">{missingKeyWarning(labelOf(id), role)} <button type="button" class="link" onclick={() => focusKey(id)}>Ajouter la clé</button></p>
  {/if}
{/snippet}

{#snippet keyRow(p: { id: string; label: string })}
  {@const st = keyStateOf(p.id)}
  <form class="key" onsubmit={(e) => { e.preventDefault(); saveKey(p.id); }}>
    <span class="who">
      <span class="label">{p.label}</span>
      <span class="status {st}">{KEY_STATE_LABELS[st]}</span>
      {#if usage[p.id]}<span class="usage">utilisée pour&nbsp;: {rolesText(usage[p.id])}</span>{/if}
    </span>
    {#if confirmDelete === p.id}
      <span class="confirm">{deleteKeyQuestion(p.label)}</span>
      <span class="actions">
        <button type="button" class="danger" onclick={() => deleteKey(p.id)} onkeydown={escCancelsDelete(p.id)}>Supprimer</button>
        <button type="button" onclick={() => cancelDelete(p.id)} onkeydown={escCancelsDelete(p.id)} use:autofocus>Annuler</button>
      </span>
    {:else}
      <input type="password" placeholder={keyPlaceholder(keyStatus[p.id])} aria-label="Clé {p.label}" bind:value={keyDrafts[p.id]} bind:this={keyInputs[p.id]} />
      <span class="actions">
        <button type="submit" class:primary={canSaveKey(keyDrafts[p.id])} disabled={!canSaveKey(keyDrafts[p.id]) || keyBusy[p.id]}>Enregistrer</button>
        {#if keyStatus[p.id]}
          <button type="button" class="danger" disabled={keyBusy[p.id]} onclick={() => (confirmDelete = p.id)} bind:this={deleteButtons[p.id]}>Supprimer</button>
        {/if}
      </span>
    {/if}
    {#if keyFeedback[p.id]}
      <span class="key-feedback" class:ok={keyFeedback[p.id].ok} class:error={!keyFeedback[p.id].ok} role="status">{keyFeedback[p.id].text}</span>
    {/if}
  </form>
{/snippet}

{#if s}
  {#if banner.length > 0}
    <div class="banner" role="note">
      Pour commencer, ajoutez la clé
      {#each banner as part, i}{#if i > 0}{i === banner.length - 1 ? " et " : ", "}{/if}<button type="button" class="link" onclick={() => focusKey(missing[i].provider)}>{part.label}</button> ({part.roles}){/each}.
    </div>
  {/if}

  <section>
    <h2>Raccourci</h2>
    <p class="help">{HOTKEY_HELP}</p>
    <div class="hotkeys">
      <div class="hotkey">
        <span class="label">Déclenchement</span>
        <strong>{capturing === "trigger" ? "Appuyez sur la touche ou la combinaison, puis relâchez… (Échap pour annuler)" : chordName(s.trigger_keys)}</strong>
        {@render hotkeyButton("trigger")}
      </div>
      <label class="check"><input type="checkbox" bind:checked={s.gesture.double_tap_enabled} /> Double-tap pour verrouiller</label>
      <div class="hotkey">
        <span class="label">Verrouillage (pendant le maintien)</span>
        <strong>{capturing === "lock" ? "Appuyez sur une touche… (Échap pour annuler)" : keyName(s.lock_vk)}</strong>
        {@render hotkeyButton("lock")}
        <button onclick={() => (s!.lock_vk = 0)} aria-label="Désactiver la touche de verrouillage"
          disabled={s.lock_vk === 0 || capturing !== null || pendingAssign !== null}>Désactiver</button>
      </div>
    </div>
    {#if pendingAssign}
      {@const sum = pendingSummary(pendingAssign.keys, { trigger_keys: s.trigger_keys, lock_vk: s.lock_vk })}
      <p class="inline-confirm" role="status">
        <span>Déclenchement = <strong>{sum.trigger}</strong>{#if sum.lock}, Verrouillage = <strong>{sum.lock}</strong>{/if}. {TYPING_KEY_WARNING}</span>
        <button class="primary" onclick={() => applyKeys(pendingAssign!)}>Continuer</button>
        <button onclick={() => (pendingAssign = null)} use:autofocus>Annuler</button>
      </p>
    {/if}
    {#if hotkeyNote}<p class="note" role="status">{hotkeyNote}</p>{/if}
  </section>

  <section>
    <h2>Correction</h2>
    <label>Niveau de correction
      <select bind:value={s.level}>
        {#each LEVEL_OPTIONS as o}<option value={o.value}>{o.label}</option>{/each}
      </select>
    </label>
  </section>

  <section>
    <h2>Fournisseurs et modèles</h2>
    <h3>Transcription</h3>
    <label>Fournisseur
      <select bind:value={s.stt_provider} onchange={onSttProvider}>
        {#each sttProviders as p}<option value={p.id}>{p.label}</option>{/each}
      </select>
    </label>
    {@render keyLine(s.stt_provider, "transcription")}
    {#key s.stt_provider}
      <label>Modèle <ModelPicker models={sttProvider?.stt_models ?? []} bind:value={s.stt_model} /></label>
    {/key}

    <h3>Correction</h3>
    {#if raw}<p class="note indent">{RAW_LEVEL_NOTE}</p>{/if}
    <fieldset disabled={raw} class:off={raw}>
      <label>Fournisseur
        <select bind:value={s.llm_provider} onchange={onLlmProvider}>
          {#each llmProviders as p}<option value={p.id}>{p.label}</option>{/each}
        </select>
      </label>
      {#if !raw}{@render keyLine(s.llm_provider, "correction")}{/if}
      {#key s.llm_provider}
        <label>Modèle <ModelPicker models={llmProvider?.llm_models.map((m) => m.id) ?? []} bind:value={s.llm_model} onchange={syncEffort} /></label>
      {/key}
      {#if efforts.length > 0}
        <label>Réflexion du modèle
          <select bind:value={s.llm_effort}>
            {#each efforts as e}<option value={e}>{effortLabel(e)}</option>{/each}
          </select>
        </label>
      {/if}
    </fieldset>

    <div class="test">
      <button onclick={runTest} disabled={testing}>{testing ? "Test en cours…" : raw ? "Tester la transcription" : "Tester transcription et correction"}</button>
      {#if testBlock}
        <p class="inline-confirm" role="status">
          {testBlock}
          <button class="primary" onclick={saveDraftsAndTest}>Enregistrer</button>
          <button onclick={() => (testBlock = null)}>Annuler</button>
        </p>
      {/if}
      <div class="test-result" role="status">
        {#each testLines as l}
          <p class:ok={l.ok} class:fail={!l.ok}><span class="title">{l.title}</span>&nbsp;: {l.text}</p>
          {#if l.raw}<details><summary>Détails</summary><pre>{l.raw}</pre></details>{/if}
        {/each}
        {#if error}
          <p class="fail">{errorSummary ? `✗ Test impossible\u00a0: ${errorSummary}` : error}</p>
          {#if errorSummary}<details><summary>Détails</summary><pre>{error}</pre></details>{/if}
        {/if}
      </div>
    </div>
  </section>

  <section>
    <h2>Clés API</h2>
    <p class="help">{KEYS_HELP}</p>
    {#each keyRows.used as p (p.id)}{@render keyRow(p)}{/each}
    {#if keyRows.others.length > 0}
      <details class="others" bind:open={othersOpen}>
        <summary>Autres fournisseurs</summary>
        {#each keyRows.others as p (p.id)}{@render keyRow(p)}{/each}
      </details>
    {/if}
  </section>

  <Startup />

  <section>
    <h2>Avancé</h2>
    {@render numberField("hold_threshold_ms", s.gesture.hold_threshold_ms, (n) => (s!.gesture.hold_threshold_ms = n))}
    {@render numberField("double_tap_window_ms", s.gesture.double_tap_window_ms, (n) => (s!.gesture.double_tap_window_ms = n))}
    {@render numberField("restore_delay_ms", s.restore_delay_ms, (n) => (s!.restore_delay_ms = n))}
    <label class="field">
      <span>{NUMBER_FIELDS.audio_retention_days.label}</span>
      <select class="short" bind:value={s.audio_retention_days}>
        {#each retentionOptions(s.audio_retention_days) as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
      </select>
    </label>
    {@render numberField("max_recording_min", s.max_recording_ms / 60000, (n) => (s!.max_recording_ms = n * 60000))}
  </section>

  <div class="statusbar {bar.tone}" class:hidden={!bar.visible}>
    <span class="msg" role="status">
      {#if !bar.alert}
        {#if bar.tone === "muted"}<span class="spinner"></span>{:else}{#key savedFlash}<span class="tick">✓</span>{/key}{/if}
        {bar.tone === "muted" ? bar.text : bar.text.replace(/^✓ /, "")}
      {/if}
    </span>
    <span class="msg alert" role="alert" title={bar.alert ? bar.text : undefined}>{bar.alert ? bar.text : ""}</span>
    {#if bar.details}
      <button type="button" class="small" aria-expanded={showDetails} onclick={() => (showDetails = !showDetails)}>Détails</button>
    {/if}
    {#if bar.retry}<button type="button" class="small primary" onclick={retrySave}>Réessayer</button>{/if}
    {#if bar.details && showDetails}<pre class="details">{bar.details}</pre>{/if}
  </div>
{:else if loadError}
  <section class="load-error" role="alert">
    <p>Impossible de charger les réglages&nbsp;: {loadError}</p>
    <button onclick={load}>Réessayer</button>
  </section>
{:else}
  <p class="loading" role="status">Chargement…</p>
{/if}

<style>
  section { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin-bottom: 12px; font-size: 14px; }
  h2 { font-size: 15px; margin: 0 0 10px; }
  h3 { font-size: 14px; margin: 16px 0 4px; color: var(--muted); font-weight: 600; }
  h2 + h3 { margin-top: 0; }
  .help { font-size: 13px; color: var(--muted); margin: -4px 0 10px; }
  /* Two columns everywhere: label text, then the control stretched over the rest of the row. */
  label { display: grid; grid-template-columns: 220px minmax(0, 1fr); align-items: center; column-gap: 12px; margin: 8px 0; }
  label > input:not([type="checkbox"]), label > select { width: 100%; box-sizing: border-box; }
  label.check { display: flex; gap: 8px; }
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  fieldset.off { opacity: 0.55; }
  fieldset:disabled :global(select), fieldset:disabled :global(input) { cursor: not-allowed; }
  .indent, .key-line { margin: -2px 0 8px 232px; }
  .key-line { font-size: 13px; }
  .key-line.ok { color: var(--muted); }
  .key-line.warn { color: var(--danger); }

  /* Raccourci: one grid for both rows so the « Changer » buttons line up. */
  .hotkeys { display: grid; grid-template-columns: 220px minmax(0, 1fr) auto auto; align-items: center; gap: 8px 12px; margin: 8px 0; }
  .hotkey { display: contents; }
  .hotkey > button:first-of-type { grid-column: 3; }
  .hotkeys label.check { grid-column: 2 / -1; margin: 0; }
  .hotkeys button:disabled, .actions button:disabled { opacity: 0.5; cursor: not-allowed; }

  /* Number fields: short input, unit suffix, grey help below. */
  label.field { row-gap: 2px; }
  .num { display: flex; align-items: center; gap: 6px; }
  .num input { width: 8ch; box-sizing: content-box; }
  .unit { color: var(--muted); }
  label.field .help { grid-column: 2; font-size: 12px; margin: 0; }
  label > select.short { width: auto; justify-self: start; min-width: 14ch; }

  /* API keys: who (name, status, usage) | field | buttons. */
  .key { display: grid; grid-template-columns: 220px minmax(0, 1fr) auto; align-items: center; column-gap: 12px; margin: 8px 0; }
  .who { display: flex; flex-wrap: wrap; align-items: baseline; gap: 2px 8px; }
  .usage { flex-basis: 100%; font-size: 12px; color: var(--muted); }
  .key input { width: 100%; box-sizing: border-box; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .confirm { color: var(--danger); font-weight: 600; }
  .key-feedback { grid-column: 2 / -1; font-size: 13px; margin-top: 4px; }
  .key-feedback.ok { color: var(--success); }
  .key-feedback.error { color: var(--danger); }
  .status { font-size: 13px; color: var(--muted); }
  .status.saved { color: var(--success); }
  .status.required { color: var(--danger); font-weight: 600; }
  details.others { margin-top: 4px; }
  details.others > summary { cursor: pointer; color: var(--muted); padding: 4px 0; }

  .banner { margin-bottom: 12px; padding: 10px 14px; border-radius: 12px; font-size: 14px; border: 1px solid var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, var(--card)); }
  .link { border: 0; background: none; padding: 0; color: var(--link); font-weight: 600; text-decoration: underline; cursor: pointer; }

  .test { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--border); }
  .test-result p { margin: 8px 0 0; }
  .test-result .ok { color: var(--success); }
  .test-result .fail { color: var(--danger); }
  .test-result .title { font-weight: 600; color: var(--text); }
  details summary { cursor: pointer; }
  .test-result details { font-size: 13px; color: var(--muted); margin: 2px 0 0; }
  pre { white-space: pre-wrap; word-break: break-word; font-size: 12px; margin: 4px 0 0; }

  :global(input[aria-invalid="true"]) { border-color: var(--danger); outline-color: var(--danger); }
  .inline-confirm { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin: 8px 0; }
  .note { font-size: 13px; color: var(--muted); margin: 4px 0 8px; }
  .note.indent { margin-left: 232px; }
  .loading { color: var(--muted); font-size: 14px; }
  .load-error p { margin: 0 0 10px; color: var(--danger); font-size: 14px; }

  /* Sticky bar at the bottom of the view: the content never scrolls under it. */
  .statusbar { position: sticky; bottom: 0; z-index: 1; display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; min-height: 30px;
    margin: 0 -16px -16px; padding: 8px 16px; background: var(--card); border-top: 1px solid var(--border); font-size: 13px;
    color: var(--muted); transition: opacity 0.4s; }
  .statusbar.hidden { opacity: 0; pointer-events: none; }
  .statusbar.success { color: var(--success); }
  .statusbar.danger { color: var(--danger); border-top-color: var(--danger); }
  .statusbar .msg { display: flex; align-items: center; gap: 8px; flex: 1 1 0; min-width: 0; white-space: nowrap; }
  .statusbar .msg.alert { display: block; overflow: hidden; text-overflow: ellipsis; }
  .statusbar .msg:empty { display: none; }
  .statusbar button.small { padding: 3px 10px; font-size: 13px; }
  .statusbar .details { flex-basis: 100%; color: var(--text); max-height: 120px; overflow: auto; }
  .tick { display: inline-block; font-weight: 700; animation: pop 0.45s ease-out; }
  .spinner { width: 12px; height: 12px; flex: none; border: 2px solid currentColor; border-top-color: transparent; border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes pop { 0% { transform: scale(0.4); opacity: 0; } 60% { transform: scale(1.35); opacity: 1; } 100% { transform: scale(1); } }
  @keyframes spin { to { transform: rotate(360deg); } }

  @media (max-width: 640px) {
    label { grid-template-columns: minmax(0, 1fr); row-gap: 4px; }
    label.field .help { grid-column: 1; }
    .indent, .key-line, .note.indent { margin-left: 0; }
    .hotkeys { grid-template-columns: minmax(0, 1fr) auto auto; }
    .hotkey > .label, .hotkeys label.check { grid-column: 1 / -1; }
    .hotkey > .label { margin-top: 4px; }
    .hotkey > button:first-of-type { grid-column: 2; }
    .key { grid-template-columns: minmax(0, 1fr) auto; row-gap: 4px; }
    .who { grid-column: 1 / -1; }
    .key-feedback { grid-column: 1 / -1; }
  }
</style>
