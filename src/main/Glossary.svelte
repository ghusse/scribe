<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Term } from "../lib/api";

  let terms = $state<Term[]>([]);
  let term = $state("");
  let variants = $state("");
  let note = $state("");
  let error = $state<string | null>(null);
  let editId = $state<number | null>(null);
  let edit = $state({ term: "", variants: "", note: "" });

  const split = (s: string) => s.split(",").map((v) => v.trim()).filter(Boolean);

  async function load() {
    terms = await api.listTerms();
  }

  async function add(e: Event) {
    e.preventDefault();
    try {
      await api.addTerm(term, split(variants), note || null);
      term = variants = note = "";
      error = null;
      await load();
    } catch (err) {
      error = String(err);
    }
  }

  function startEdit(t: Term) {
    editId = t.id;
    edit = { term: t.term, variants: t.variants.join(", "), note: t.note ?? "" };
  }

  async function saveEdit(id: number) {
    try {
      await api.updateTerm(id, edit.term, split(edit.variants), edit.note || null);
      editId = null;
      error = null;
      await load();
    } catch (err) {
      error = String(err);
    }
  }

  async function remove(id: number) {
    await api.deleteTerm(id);
    await load();
  }

  onMount(load);
</script>

<form onsubmit={add}>
  <input placeholder="Terme (ex. Kubernetes)" bind:value={term} required />
  <input placeholder="Mal entendu (séparés par des virgules)" bind:value={variants} />
  <input placeholder="Note / contexte (optionnel)" bind:value={note} />
  <button class="primary" type="submit">Ajouter</button>
</form>
{#if error}<p class="error">{error}</p>{/if}

<table>
  <thead><tr><th>Terme</th><th>Entendu</th><th>Note</th><th>Usages</th><th></th></tr></thead>
  <tbody>
    {#each terms as t (t.id)}
      <tr>
        {#if editId === t.id}
          <td><input bind:value={edit.term} /></td>
          <td><input bind:value={edit.variants} /></td>
          <td><input bind:value={edit.note} /></td>
          <td>{t.use_count}</td>
          <td><button class="primary" onclick={() => saveEdit(t.id)}>OK</button> <button onclick={() => (editId = null)}>Annuler</button></td>
        {:else}
          <td><strong>{t.term}</strong></td>
          <td>{t.variants.join(", ")}</td>
          <td>{t.note ?? ""}</td>
          <td>{t.use_count}</td>
          <td><button onclick={() => startEdit(t)}>Modifier</button> <button class="danger" onclick={() => remove(t.id)}>Supprimer</button></td>
        {/if}
      </tr>
    {:else}
      <tr><td colspan="5" class="muted">Glossaire vide : ajoutez les noms propres et le jargon de vos domaines.</td></tr>
    {/each}
  </tbody>
</table>

<style>
  form { display: grid; grid-template-columns: 1fr 1.4fr 1.4fr auto; gap: 8px; margin-bottom: 12px; }
  table { width: 100%; border-collapse: collapse; background: var(--card); border-radius: 12px; overflow: hidden; }
  th, td { text-align: left; padding: 8px 10px; border-bottom: 1px solid var(--border); font-size: 14px; }
  th { color: var(--muted); font-weight: 500; }
  td input { width: 100%; box-sizing: border-box; }
  .error { color: var(--danger); }
  .muted { color: var(--muted); }
  @media (max-width: 700px) { form { grid-template-columns: 1fr; } }
</style>
