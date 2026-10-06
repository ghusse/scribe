<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Permission, type PermissionStatus } from "../lib/api";
  import { missingPermissions, PERMISSION_COPY, permissionActions } from "../lib/permissions";

  /** Re-read while something is missing: the user grants it in System Settings, outside Scribe. */
  const POLL_MS = 1500;

  let missing = $state<PermissionStatus[]>([]);
  let error = $state<string | null>(null);

  async function refresh() {
    try {
      missing = missingPermissions(await api.permissions());
    } catch {
      // Keep the last known state; the next poll tries again.
    }
  }

  async function request(permission: Permission) {
    error = null;
    try {
      missing = missingPermissions(await api.requestPermission(permission));
    } catch (e) {
      error = `Demande impossible : ${e}`;
    }
  }

  async function openSettings(permission: Permission) {
    error = null;
    try {
      await api.openPermissionSettings(permission);
    } catch (e) {
      error = `Ouverture des Réglages impossible : ${e}`;
    }
  }

  onMount(() => {
    refresh();
    const timer = setInterval(() => {
      if (missing.length > 0) refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  });
</script>

{#if missing.length > 0}
  <section aria-label="Autorisations manquantes">
    <h2>Scribe a besoin de votre autorisation</h2>
    {#each missing as status (status.permission)}
      {@const copy = PERMISSION_COPY[status.permission]}
      <div class="permission">
        <h3>{copy.title}</h3>
        <p>{copy.why}</p>
        <p class="how">{copy.how}</p>
        <div class="row">
          {#each permissionActions(status) as action}
            {#if action === "request"}
              <button class="primary" onclick={() => request(status.permission)}>Autoriser</button>
            {:else}
              <button onclick={() => openSettings(status.permission)}>Ouvrir les Réglages Système</button>
            {/if}
          {/each}
        </div>
      </div>
    {/each}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  section { background: var(--card); border: 1px solid var(--danger); border-radius: 12px; padding: 12px 16px; margin-bottom: 12px; font-size: 14px; }
  h2 { font-size: 15px; margin: 0 0 10px; }
  h3 { font-size: 14px; margin: 0 0 4px; }
  .permission + .permission { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border); }
  p { margin: 0 0 6px; }
  .how { color: var(--muted); font-size: 13px; }
  .row { display: flex; gap: 10px; flex-wrap: wrap; }
  .error { color: var(--danger); font-size: 13px; margin: 8px 0 0; }
</style>
