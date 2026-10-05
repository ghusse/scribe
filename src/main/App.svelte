<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { missingKeys } from "../lib/apiKeys";
  import { needsAttention, type SaveState } from "../lib/autosave";
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
  // Set by the toast's « Voir » button; consumed by History once it has scrolled to the card.
  let focusRequest = $state<{ id: number } | null>(null);
  // Settings stays mounted (hidden) so its edits, drafts and errors survive a tab switch.
  let settingsStatus = $state<SaveState>("saved");

  onMount(() => {
    // Listened here (always mounted) rather than in History, which may not be mounted.
    const unFocus = listen<number>("focus-dictation", (e) => {
      focusRequest = { id: e.payload };
      tab = "history";
    });
    // First launch: without the API keys, open on the settings tab.
    // Same rule as needs_setup in src-tauri/src/bootstrap.rs.
    Promise.all([api.getSettings(), api.keyStatus()])
      .then(([st, k]) => {
        const missing = missingKeys(st, k).length > 0;
        if (missing && tab === "history" && focusRequest === null) tab = "settings";
      })
      .catch(() => {});
    return () => {
      unFocus.then((f) => f());
    };
  });
</script>

<nav>
  {#each tabs as t}
    <button class:active={tab === t.id} onclick={() => (tab = t.id)}>
      {t.label}{#if t.id === "settings" && needsAttention(settingsStatus)}<span class="dot" role="img" aria-label="(réglages non enregistrés)" title="Réglages non enregistrés"></span>{/if}
    </button>
  {/each}
</nav>
<main>
  {#if tab === "history"}<History focus={focusRequest} onfocused={() => (focusRequest = null)} />{:else if tab === "glossary"}<Glossary />{/if}
  <div hidden={tab !== "settings"}><Settings active={tab === "settings"} bind:status={settingsStatus} /></div>
</main>

<style>
  /* Text colours (--danger, --success, --link) meet WCAG AA on --card and --bg in both themes. */
  :global(:root) { --bg: #f7f7fb; --card: #fff; --text: #1f2330; --muted: #6b7080; --accent: #4f46e5; --border: #e3e4ec;
    --danger: #b91c1c; --success: #15803d; --link: #4338ca; color-scheme: light dark; accent-color: var(--accent); }
  @media (prefers-color-scheme: dark) {
    :global(:root) { --bg: #14151b; --card: #1d1f27; --text: #e8e9f0; --muted: #9a9eb0; --border: #2c2f3a;
      --danger: #f87171; --success: #4ade80; --link: #a5b4fc; }
  }
  :global(body) { margin: 0; font-family: system-ui, sans-serif; background: var(--bg); color: var(--text); }
  :global(button) { font: inherit; cursor: pointer; border-radius: 8px; border: 1px solid var(--border); background: var(--card); color: var(--text); padding: 6px 12px; }
  :global(button.primary) { background: var(--accent); color: #fff; border-color: var(--accent); }
  :global(button.danger) { color: var(--danger); }
  :global(input), :global(textarea), :global(select) { font: inherit; padding: 6px 8px; border-radius: 8px; border: 1px solid var(--border); background: var(--card); color: var(--text); }
  nav { display: flex; gap: 4px; padding: 12px 16px; border-bottom: 1px solid var(--border); background: var(--card); position: sticky; top: 0; }
  nav button { border: 0; background: transparent; }
  nav button.active { background: var(--accent); color: #fff; }
  .dot { display: inline-block; width: 8px; height: 8px; margin-left: 6px; border-radius: 50%; background: var(--danger); vertical-align: middle; }
  main { padding: 16px; max-width: 900px; margin: 0 auto; }
</style>
