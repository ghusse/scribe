<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, effortLevels, type Provider, type ProviderTest, type Settings } from "../lib/api";
  import { keyName } from "../lib/keys";
  import ModelPicker from "./ModelPicker.svelte";

  let s = $state<Settings | null>(null);
  let providers = $state<Provider[]>([]);
  let sttProviders = $derived(providers.filter((p) => p.stt_models.length > 0));
  let llmProviders = $derived(providers.filter((p) => p.llm_api !== null));
  let sttProvider = $derived(providers.find((p) => p.id === s?.stt_provider));
  let llmProvider = $derived(providers.find((p) => p.id === s?.llm_provider));
  let efforts = $derived(s ? effortLevels(llmProvider, s.llm_model) : []);
  let keyStatus = $state<Record<string, boolean>>({});
  let keyDrafts = $state<Record<string, string>>({});
  let capturing = $state<"trigger" | "lock" | null>(null);
  let error = $state<string | null>(null);

  // Autosave: every change is persisted ~500 ms after the last edit; the pill shows the outcome.
  type SaveState = "saved" | "pending" | "saving" | "invalid" | "failed";
  let saveState = $state<SaveState>("saved");
  let saveError = $state<string | null>(null);
  let savedFlash = $state(0);
  let lastSaved = "";
  let lastAttempted = "";
  let inFlight = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let test = $state<ProviderTest | null>(null);
  let testing = $state(false);

  onMount(async () => {
    [s, providers, keyStatus] = await Promise.all([api.getSettings(), api.providers(), api.keyStatus()]);
    lastSaved = lastAttempted = JSON.stringify($state.snapshot(s));
  });

  $effect(() => {
    if (!s) return;
    const json = JSON.stringify($state.snapshot(s)); // deep read: any field change re-runs this
    if (json === lastSaved) return;
    saveState = "pending";
    clearTimeout(timer);
    timer = setTimeout(flush, 500);
  });

  // Leaving the tab must not lose the last edit.
  onDestroy(() => {
    if (saveState === "pending") {
      clearTimeout(timer);
      flush();
    }
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

  async function saveKey(provider: string) {
    try {
      await api.setApiKey(provider, keyDrafts[provider] ?? "");
      keyDrafts[provider] = "";
      keyStatus = await api.keyStatus();
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function capture(which: "trigger" | "lock") {
    if (!s) return;
    capturing = which;
    const vk = await api.captureKey();
    capturing = null;
    if (vk === null) return;
    if (which === "trigger") s.trigger_vk = vk;
    else s.lock_vk = vk;
  }

  // bind:value on type=number yields null when cleared and allows decimals, which the
  // backend (u32/u64) rejects with an English serde error: validate here, in French.
  function checkNumbers(v: Settings): string | null {
    const fields: [string, unknown][] = [
      ["Seuil de maintien", v.gesture.hold_threshold_ms],
      ["Fenêtre de double-tap", v.gesture.double_tap_window_ms],
      ["Délai avant restauration du presse-papier", v.restore_delay_ms],
      ["Conserver l'audio", v.audio_retention_days],
    ];
    for (const [label, n] of fields) {
      if (typeof n !== "number" || !Number.isInteger(n) || n < 0) return `${label} : entrez un nombre entier positif.`;
    }
    if (!Number.isFinite(v.max_recording_ms) || v.max_recording_ms <= 0) return "Durée maximale : entrez un nombre de minutes.";
    return null;
  }

  async function flush() {
    if (!s || inFlight) return;
    const snap = $state.snapshot(s) as Settings;
    const json = JSON.stringify(snap);
    if (json === lastSaved) {
      saveState = "saved";
      return;
    }
    snap.max_recording_ms = Math.round(snap.max_recording_ms);
    const invalid = checkNumbers(snap);
    if (invalid) {
      saveState = "invalid";
      saveError = invalid;
      return;
    }
    inFlight = true;
    lastAttempted = json;
    saveState = "saving";
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
      inFlight = false;
    }
    // Edits made while the request was in flight: save them too (but never retry the same failing payload).
    if (s && JSON.stringify($state.snapshot(s)) !== lastAttempted) flush();
  }

  async function runTest() {
    testing = true;
    try {
      test = await api.testProviders();
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      testing = false;
    }
  }

  const show = (r: { Ok: string } | { Err: string }) => ("Ok" in r ? `✓ ${r.Ok}` : `✗ ${r.Err}`);
</script>

{#if s}
  <section>
    <h2>Clés API</h2>
    {#each providers as p}
      <div class="key">
        <span class="label">{p.label}</span>
        <span class="status">{keyStatus[p.id] ? "✓ enregistrée" : "✗ absente"}</span>
        <input type="password" placeholder="Nouvelle clé (vide = supprimer)" bind:value={keyDrafts[p.id]} />
        <button onclick={() => saveKey(p.id)}>Enregistrer</button>
      </div>
    {/each}
    <button onclick={runTest} disabled={testing}>{testing ? "Test…" : "Tester la configuration"}</button>
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
      <strong>{capturing === "trigger" ? "Appuyez sur une touche…" : keyName(s.trigger_vk)}</strong>
      <button onclick={() => capture("trigger")} disabled={capturing !== null}>Changer</button>
    </div>
    <div class="key hotkey">
      <span class="label">Verrouillage (maintenir + touche)</span>
      <strong>{capturing === "lock" ? "Appuyez sur une touche…" : keyName(s.lock_vk)}</strong>
      <button onclick={() => capture("lock")} disabled={capturing !== null}>Changer</button>
      <button onclick={() => (s!.lock_vk = 0)}>Aucune</button>
    </div>
    <label><input type="checkbox" bind:checked={s.gesture.double_tap_enabled} /> Double-tap pour verrouiller</label>
    <label><input type="checkbox" bind:checked={s.gesture.lock_key_enabled} /> Touche de verrouillage active</label>
    <label>Seuil de maintien (ms) <input type="number" min="100" max="2000" bind:value={s.gesture.hold_threshold_ms} /></label>
    <label>Fenêtre de double-tap (ms) <input type="number" min="150" max="1000" bind:value={s.gesture.double_tap_window_ms} /></label>
  </section>

  <section>
    <h2>Avancé</h2>
    <label>Délai avant restauration du presse-papier (ms) <input type="number" min="0" max="2000" bind:value={s.restore_delay_ms} /></label>
    <label>Conserver l'audio (jours, 0 = toujours) <input type="number" min="0" bind:value={s.audio_retention_days} /></label>
    <label>Durée maximale d'une dictée (min)
      <input type="number" min="1" max="10" step="any" value={s.max_recording_ms / 60000} oninput={(e) => (s!.max_recording_ms = Number((e.target as HTMLInputElement).value) * 60000)} />
    </label>
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
  .status { color: var(--muted); }
  @media (max-width: 640px) {
    label, .key { grid-template-columns: 1fr; row-gap: 4px; }
    label:has(> input[type="checkbox"]) { padding-left: 0; }
  }
  .error { color: var(--danger); }
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
