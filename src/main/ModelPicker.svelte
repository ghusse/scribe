<script lang="ts">
  import { tick } from "svelte";
  import { checkModelId } from "../lib/validation";

  // Known models in a dropdown (first = newest = default), plus « Autre… » for a model typed by hand.
  // Remount it (with {#key}) when the provider changes.
  let { models, value = $bindable(), onchange }: { models: string[]; value: string; onchange?: () => void } = $props();

  const CUSTOM = "__custom__";
  let forceCustom = $state(false);
  let selected = $derived(forceCustom || !models.includes(value) ? CUSTOM : value);
  let input = $state<HTMLInputElement>();
  let invalid = $state<string | null>(null);

  async function pick(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    forceCustom = v === CUSTOM;
    invalid = null;
    if (v !== CUSTOM) {
      value = v;
      onchange?.();
      return;
    }
    // Typing replaces the prefilled id; leaving it as is keeps the current model.
    await tick();
    input?.focus();
    input?.select();
  }

  // Committed on change (blur or Entrée), never per keystroke: « claude- » must not be saved.
  function commit(e: Event & { currentTarget: HTMLInputElement }) {
    const r = checkModelId(e.currentTarget.value);
    if (!r.ok) {
      invalid = r.message;
      return;
    }
    invalid = null;
    e.currentTarget.value = r.value;
    if (r.value !== value) {
      value = r.value;
      onchange?.();
    }
  }
</script>

<div class="picker">
  <select value={selected} onchange={pick}>
    {#each models as m, i}
      <option value={m}>{m}{i === 0 ? " (dernier, par défaut)" : ""}</option>
    {/each}
    <option value={CUSTOM}>Autre…</option>
  </select>
  {#if selected === CUSTOM}
    <input bind:this={input} {value} title={value} placeholder="ex. gpt-4o-mini (identifiant exact de l'API)"
      aria-label="Identifiant du modèle personnalisé" aria-invalid={invalid !== null} onchange={commit} />
  {/if}
  {#if invalid}<span class="picker-error" role="alert">{invalid}</span>{/if}
</div>

<style>
  .picker { display: flex; flex-wrap: wrap; gap: 4px 8px; width: 100%; min-width: 0; }
  .picker select, .picker input { flex: 1 1 0; min-width: 0; box-sizing: border-box; }
  .picker-error { flex-basis: 100%; color: var(--danger); font-size: 13px; }
</style>
