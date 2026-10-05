<script lang="ts">
  // Known models in a dropdown (first = newest = default), plus « Autre… » for a model typed by hand.
  // Remount it (with {#key}) when the provider changes.
  let { models, value = $bindable(), onchange }: { models: string[]; value: string; onchange?: () => void } = $props();

  const CUSTOM = "__custom__";
  let forceCustom = $state(false);
  let selected = $derived(forceCustom || !models.includes(value) ? CUSTOM : value);

  function pick(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    forceCustom = v === CUSTOM;
    if (v !== CUSTOM) value = v;
    onchange?.();
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
    <input placeholder="Identifiant du modèle" bind:value oninput={() => onchange?.()} />
  {/if}
</div>

<style>
  .picker { display: flex; gap: 8px; width: 100%; min-width: 0; }
  .picker select, .picker input { flex: 1 1 0; min-width: 0; box-sizing: border-box; }
</style>
