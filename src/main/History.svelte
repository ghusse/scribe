<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick, untrack } from "svelte";
  import { api, type Dictation } from "../lib/api";
  import { canCopy, canRetranscribe, displayText, editDraft, editPayload, latestOnly, OUTCOME_BADGES, STALE, timingLine } from "../lib/history";

  let { focus = null, onfocused }: { focus?: { id: number } | null; onfocused?: () => void } = $props();

  const PAGE = 50;
  let items = $state<Dictation[]>([]);
  let query = $state("");
  let hasMore = $state(false);
  let error = $state<string | null>(null);
  let editing = $state<number | null>(null);
  let draft = $state("");
  let expanded = $state<number | null>(null);
  let confirmDelete = $state<number | null>(null);
  let busy = $state<number | null>(null);
  let highlighted = $state<number | null>(null);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  // Search results can come back out of order: only the latest request's page is shown.
  const latest = latestOnly();

  async function load(reset = true) {
    try {
      const page = await latest(api.listDictations(query || null, PAGE, reset ? 0 : items.length));
      if (page === STALE) return;
      items = reset ? page : [...items, ...page];
      hasMore = page.length === PAGE;
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  function onSearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => load(true), 250);
  }

  async function run(id: number, action: () => Promise<void>): Promise<boolean> {
    busy = id;
    try {
      await action();
      error = null;
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = null;
    }
  }

  function startEdit(d: Dictation) {
    editing = d.id;
    draft = editDraft(d);
  }

  async function saveEdit(d: Dictation) {
    // On failure keep the editor open (and the draft) so the error stays visible.
    if (!(await run(d.id, () => api.saveEditedText(d.id, editPayload(draft))))) return;
    editing = null;
    await load(true);
  }

  async function focusOn(id: number) {
    query = "";
    await load(true);
    highlighted = id;
    await tick();
    document.getElementById(`d-${id}`)?.scrollIntoView({ behavior: "smooth", block: "center" });
    onfocused?.();
  }

  // The focus-dictation event is received by App (always mounted) and passed down here,
  // including when this tab is mounted because of it.
  $effect(() => {
    const f = focus;
    if (f) untrack(() => focusOn(f.id));
  });

  onMount(() => {
    if (!untrack(() => focus)) load(true);
    const unChanged = listen("history-changed", () => load(true));
    return () => {
      clearTimeout(searchTimer);
      unChanged.then((f) => f());
    };
  });
</script>

<input class="search" placeholder="Rechercher dans l'historique…" bind:value={query} oninput={onSearch} />
{#if error}<p class="error">{error}</p>{/if}
{#if items.length === 0}
  <p class="muted">Aucune dictée. Maintenez la touche de déclenchement et parlez.</p>
{/if}

{#each items as d (d.id)}
  <article id="d-{d.id}" class:highlighted={highlighted === d.id}>
    <header>
      <span class="badge {d.outcome}">{OUTCOME_BADGES[d.outcome]}</span>
      <span class="muted">{new Date(d.created_at).toLocaleString("fr-FR")}</span>
      {#if d.app_name}<span class="muted">· {d.app_name}</span>{/if}
      <span class="muted right">
        {timingLine(d)}
      </span>
    </header>

    {#if editing === d.id}
      <textarea rows="4" bind:value={draft}></textarea>
      <div class="row">
        <button class="primary" onclick={() => saveEdit(d)}>Enregistrer la correction</button>
        <button onclick={() => (editing = null)}>Annuler</button>
      </div>
    {:else}
      <p class="text">{displayText(d)}</p>
    {/if}
    {#if d.error}<p class="error small">{d.error}</p>{/if}

    {#if expanded === d.id}
      <dl>
        <dt>Brut</dt><dd>{d.raw_text ?? "—"}</dd>
        <dt>Corrigé</dt><dd>{d.final_text ?? "—"}</dd>
        {#if d.edited_text}<dt>Votre correction</dt><dd>{d.edited_text}</dd>{/if}
        <dt>Modèles</dt><dd>{d.transcriber ?? "—"} / {d.corrector ?? "—"}</dd>
      </dl>
    {/if}

    <div class="row">
      <button onclick={() => run(d.id, () => api.copyDictation(d.id))} disabled={!canCopy(d)}>Copier</button>
      <button onclick={() => startEdit(d)} disabled={!canCopy(d)}>Corriger</button>
      <button onclick={() => run(d.id, () => api.retranscribe(d.id))} disabled={!canRetranscribe(d, busy)}>
        {busy === d.id ? "…" : "Retranscrire"}
      </button>
      <button onclick={() => (expanded = expanded === d.id ? null : d.id)}>{expanded === d.id ? "Moins" : "Détails"}</button>
      {#if confirmDelete === d.id}
        <button class="danger" onclick={() => run(d.id, async () => { await api.deleteDictation(d.id); confirmDelete = null; await load(true); })}>Confirmer la suppression</button>
        <button onclick={() => (confirmDelete = null)}>Annuler</button>
      {:else}
        <button class="danger" onclick={() => (confirmDelete = d.id)}>Supprimer</button>
      {/if}
    </div>
  </article>
{/each}
{#if hasMore}<button onclick={() => load(false)}>Plus</button>{/if}

<style>
  .search { width: 100%; box-sizing: border-box; margin-bottom: 12px; }
  article { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 12px 14px; margin-bottom: 10px; }
  article.highlighted { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent); }
  header { display: flex; gap: 8px; align-items: center; font-size: 13px; flex-wrap: wrap; }
  .right { margin-left: auto; }
  .muted { color: var(--muted); }
  .text { white-space: pre-wrap; margin: 8px 0; }
  .row { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 8px; }
  textarea { width: 100%; box-sizing: border-box; margin-top: 8px; }
  .badge { font-size: 12px; padding: 2px 8px; border-radius: 999px; background: #e0e7ff; color: #3730a3; }
  .badge.error { background: #fee2e2; color: #991b1b; }
  .badge.pasted_uncertain { background: #fef3c7; color: #92400e; }
  .error { color: var(--danger); }
  .small { font-size: 13px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; font-size: 13px; }
  dt { color: var(--muted); }
  dd { margin: 0; white-space: pre-wrap; }
</style>
