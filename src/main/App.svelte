<script lang="ts">
  import History from "./History.svelte";
  import Glossary from "./Glossary.svelte";
  import Settings from "./Settings.svelte";

  type Tab = "history" | "glossary" | "settings";
  let tab = $state<Tab>("history");
  const tabs: { id: Tab; label: string }[] = [
    { id: "history", label: "Historique" },
    { id: "glossary", label: "Vocabulaire" },
    { id: "settings", label: "Réglages" },
  ];
</script>

<nav>
  {#each tabs as t}
    <button class:active={tab === t.id} onclick={() => (tab = t.id)}>{t.label}</button>
  {/each}
</nav>
<main>
  {#if tab === "history"}<History />{:else if tab === "glossary"}<Glossary />{:else}<Settings />{/if}
</main>

<style>
  :global(:root) { --bg: #f7f7fb; --card: #fff; --text: #1f2330; --muted: #6b7080; --accent: #4f46e5; --border: #e3e4ec; --danger: #dc2626; }
  @media (prefers-color-scheme: dark) {
    :global(:root) { --bg: #14151b; --card: #1d1f27; --text: #e8e9f0; --muted: #9a9eb0; --border: #2c2f3a; }
  }
  :global(body) { margin: 0; font-family: system-ui, sans-serif; background: var(--bg); color: var(--text); }
  :global(button) { font: inherit; cursor: pointer; border-radius: 8px; border: 1px solid var(--border); background: var(--card); color: var(--text); padding: 6px 12px; }
  :global(button.primary) { background: var(--accent); color: #fff; border-color: var(--accent); }
  :global(button.danger) { color: var(--danger); }
  :global(input), :global(textarea), :global(select) { font: inherit; padding: 6px 8px; border-radius: 8px; border: 1px solid var(--border); background: var(--card); color: var(--text); }
  nav { display: flex; gap: 4px; padding: 12px 16px; border-bottom: 1px solid var(--border); background: var(--card); position: sticky; top: 0; }
  nav button { border: 0; background: transparent; }
  nav button.active { background: var(--accent); color: #fff; }
  main { padding: 16px; max-width: 900px; margin: 0 auto; }
</style>
