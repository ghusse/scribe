<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";

  // The state lives in the OS (registry Run key / LaunchAgent), not in the settings file: read it, switch it.
  let enabled = $state<boolean | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    try {
      enabled = await api.getAutostart();
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function toggle(e: Event) {
    // currentTarget is null once the handler awaits: keep the element.
    const input = e.currentTarget as HTMLInputElement;
    const want = input.checked;
    busy = true;
    try {
      enabled = await api.setAutostart(want);
      error = null;
    } catch (err) {
      error = String(err);
      // Show what is really in place, not the click that failed.
      input.checked = enabled ?? false;
    } finally {
      busy = false;
    }
  }

  onMount(load);
</script>

<section>
  <h2>Démarrage</h2>
  <label class="check">
    <input type="checkbox" checked={enabled ?? false} disabled={enabled === null || busy} onchange={toggle} />
    Lancer Scribe à l'ouverture de session
  </label>
  <p class="help">Scribe démarre alors discrètement, sans ouvrir de fenêtre, prêt à dicter.</p>
  {#if error}<p class="error" role="alert">Démarrage automatique indisponible&nbsp;: {error}</p>{/if}
</section>

<style>
  section { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin-bottom: 12px; font-size: 14px; }
  h2 { font-size: 15px; margin: 0 0 10px; }
  .check { display: flex; gap: 8px; align-items: center; }
  .help { color: var(--muted); font-size: 13px; margin: 6px 0 0; }
  .error { color: var(--danger); font-size: 13px; margin: 6px 0 0; }
</style>
