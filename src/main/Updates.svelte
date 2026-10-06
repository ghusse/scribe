<script lang="ts">
  import { api, type UpdateStatus } from "../lib/api";

  // Checked on demand only: opening the settings never calls the network (Scribe also checks once at startup).
  let status = $state<UpdateStatus | null>(null);
  let phase = $state<"idle" | "checking" | "installing">("idle");
  let error = $state<string | null>(null);

  async function check() {
    phase = "checking";
    error = null;
    try {
      status = await api.checkUpdate();
    } catch (e) {
      error = `Vérification impossible : ${e}`;
    } finally {
      phase = "idle";
    }
  }

  async function install() {
    phase = "installing";
    error = null;
    try {
      // Scribe restarts on success: this only returns on failure.
      await api.installUpdate();
    } catch (e) {
      error = `Installation impossible : ${e}`;
      phase = "idle";
    }
  }
</script>

<section>
  <h2>Mises à jour</h2>
  <div class="row">
    <button onclick={check} disabled={phase !== "idle"}>
      {phase === "checking" ? "Recherche…" : "Rechercher des mises à jour"}
    </button>
    {#if status && !status.available}<span role="status">Scribe {status.current} est à jour.</span>{/if}
  </div>
  {#if status?.available}
    {@const next = status.available}
    <div class="available" role="status">
      <p>Scribe <strong>{next.version}</strong> est disponible (version actuelle&nbsp;: {status.current}).</p>
      {#if next.notes}<pre class="notes">{next.notes}</pre>{/if}
      <button class="primary" onclick={install} disabled={phase !== "idle"}>
        {phase === "installing" ? "Installation… Scribe va redémarrer" : "Installer et redémarrer"}
      </button>
    </div>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  section { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin-bottom: 12px; font-size: 14px; }
  h2 { font-size: 15px; margin: 0 0 10px; }
  .row { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
  .available { margin-top: 10px; }
  .available p { margin: 0 0 6px; }
  .notes { white-space: pre-wrap; font-family: inherit; font-size: 13px; color: var(--muted); margin: 0 0 8px; max-height: 10em; overflow: auto; }
  .error { color: var(--danger); font-size: 13px; margin: 6px 0 0; }
</style>
