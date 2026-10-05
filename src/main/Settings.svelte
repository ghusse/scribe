<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ProviderTest, type Settings, type SttPreset } from "../lib/api";
  import { keyName } from "../lib/keys";

  const PROVIDERS: { id: string; label: string }[] = [
    { id: "openai", label: "OpenAI" },
    { id: "groq", label: "Groq" },
    { id: "mistral", label: "Mistral" },
    { id: "anthropic", label: "Anthropic (correction)" },
  ];

  let s = $state<Settings | null>(null);
  let presets = $state<SttPreset[]>([]);
  let keyStatus = $state<Record<string, boolean>>({});
  let keyDrafts = $state<Record<string, string>>({});
  let capturing = $state<"trigger" | "lock" | null>(null);
  let message = $state<string | null>(null);
  let error = $state<string | null>(null);
  let test = $state<ProviderTest | null>(null);
  let testing = $state(false);

  onMount(async () => {
    [s, presets, keyStatus] = await Promise.all([api.getSettings(), api.sttPresets(), api.keyStatus()]);
  });

  function applyPreset(id: string) {
    const p = presets.find((x) => x.id === id);
    if (s && p) {
      s.stt_base_url = p.base_url;
      s.stt_model = p.default_model;
    }
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

  async function save() {
    if (!s) return;
    const snap = $state.snapshot(s) as Settings;
    snap.max_recording_ms = Math.round(snap.max_recording_ms);
    const invalid = checkNumbers(snap);
    if (invalid) {
      error = invalid;
      message = null;
      return;
    }
    try {
      await api.saveSettings(snap);
      message = "Réglages enregistrés.";
      error = null;
    } catch (e) {
      error = String(e);
      message = null;
    }
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
    {#each PROVIDERS as p}
      <div class="key">
        <span class="label">{p.label}</span>
        <span>{keyStatus[p.id] ? "✓ enregistrée" : "✗ absente"}</span>
        <input type="password" placeholder="Nouvelle clé (vide = supprimer)" bind:value={keyDrafts[p.id]} />
        <button onclick={() => saveKey(p.id)}>Enregistrer</button>
      </div>
    {/each}
    <button onclick={runTest} disabled={testing}>{testing ? "Test…" : "Tester la configuration"}</button>
    {#if test}<p>Transcription : {show(test.stt)} — Correction : {show(test.llm)}</p>{/if}
  </section>

  <section>
    <h2>Transcription</h2>
    <label>Fournisseur
      <select bind:value={s.stt_preset} onchange={() => applyPreset(s!.stt_preset)}>
        {#each presets as p}<option value={p.id}>{p.label}</option>{/each}
      </select>
    </label>
    <label>URL <input bind:value={s.stt_base_url} /></label>
    <label>Modèle <input bind:value={s.stt_model} /></label>
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
    <label>Modèle Claude <input bind:value={s.llm_model} /></label>
    <label>Effort
      <select bind:value={s.llm_effort}>
        <option value="low">low</option><option value="medium">medium</option><option value="high">high</option>
      </select>
    </label>
  </section>

  <section>
    <h2>Raccourci</h2>
    <div class="key">
      <span class="label">Déclenchement</span>
      <strong>{capturing === "trigger" ? "Appuyez sur une touche…" : keyName(s.trigger_vk)}</strong>
      <button onclick={() => capture("trigger")} disabled={capturing !== null}>Changer</button>
    </div>
    <div class="key">
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

  <button class="primary" onclick={save}>Enregistrer les réglages</button>
  {#if message}<p class="ok">{message}</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}
{/if}

<style>
  section { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin-bottom: 12px; }
  h2 { font-size: 15px; margin: 0 0 10px; }
  label { display: flex; align-items: center; gap: 8px; margin: 6px 0; font-size: 14px; }
  label input:not([type="checkbox"]), label select { flex: 1; }
  .key { display: flex; align-items: center; gap: 10px; margin: 6px 0; font-size: 14px; flex-wrap: wrap; }
  .key input { flex: 1; min-width: 200px; }
  .label { width: 210px; color: var(--muted); }
  .ok { color: #16a34a; }
  .error { color: var(--danger); }
</style>
