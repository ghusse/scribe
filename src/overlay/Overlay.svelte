<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api, type OverlayEvent } from "../lib/api";

  let ov = $state<OverlayEvent>({ kind: "idle" });
  let levels = $state<number[]>(Array(12).fill(0));
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    const unOverlay = listen<OverlayEvent>("overlay", (e) => {
      clearTimeout(hideTimer);
      ov = e.payload;
      if (ov.kind === "recording" && !ov.locked) levels = Array(12).fill(0);
      if (ov.kind === "toast") hideTimer = setTimeout(dismiss, ov.level === "error" ? 10000 : 6000);
    });
    const unLevel = listen<number>("audio-level", (e) => {
      levels = [...levels.slice(1), Math.min(1, e.payload * 6)];
    });
    return () => {
      unOverlay.then((f) => f());
      unLevel.then((f) => f());
    };
  });

  function dismiss() {
    clearTimeout(hideTimer);
    ov = { kind: "idle" };
    api.overlayDismiss();
  }

  async function copy(id: number | null) {
    if (id !== null) await api.copyDictation(id);
    dismiss();
  }
</script>

{#if ov.kind === "recording"}
  <div class="pill">
    <span class="dot"></span>
    <div class="bars">
      {#each levels as l}<span style="height: {4 + l * 24}px"></span>{/each}
    </div>
    {#if ov.locked}<span class="tag">Verrouillé</span>{/if}
  </div>
{:else if ov.kind === "processing"}
  <div class="pill"><span class="spinner"></span><span class="label">Transcription…</span></div>
{:else if ov.kind === "toast"}
  <div class="toast {ov.level}">
    <div class="text">
      <strong>{ov.message}</strong>
      {#if ov.preview}<p>{ov.preview}</p>{/if}
    </div>
    <div class="actions">
      {#if ov.dictation_id !== null && ov.level !== "error"}
        <button onclick={() => copy(ov.kind === "toast" ? ov.dictation_id : null)}>Copier</button>
      {/if}
      {#if ov.dictation_id !== null}
        <button onclick={() => api.openHistory(ov.kind === "toast" ? ov.dictation_id : null)}>Voir</button>
      {/if}
      <button class="close" onclick={dismiss} aria-label="Fermer">×</button>
    </div>
  </div>
{/if}

<style>
  :global(body) { font-family: system-ui, sans-serif; display: flex; justify-content: center; align-items: flex-end; height: 100vh; }
  .pill, .toast { background: rgba(24, 24, 32, 0.92); color: #fff; border-radius: 999px; padding: 10px 18px;
    display: flex; align-items: center; gap: 12px; box-shadow: 0 4px 18px rgba(0,0,0,.35); margin-bottom: 8px; }
  .toast { border-radius: 14px; max-width: 420px; align-items: flex-start; }
  .toast.error { border-left: 4px solid #ef4444; }
  .toast.copied, .toast.uncertain { border-left: 4px solid #6366f1; }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: #ef4444; animation: pulse 1s infinite; }
  .bars { display: flex; align-items: center; gap: 3px; height: 28px; }
  .bars span { width: 4px; background: #a5b4fc; border-radius: 2px; transition: height 60ms linear; }
  .tag { font-size: 12px; background: #6366f1; padding: 2px 8px; border-radius: 999px; }
  .label { font-size: 14px; }
  .spinner { width: 14px; height: 14px; border: 2px solid #a5b4fc; border-top-color: transparent; border-radius: 50%; animation: spin .8s linear infinite; }
  .text { flex: 1; font-size: 13px; }
  .text p { margin: 4px 0 0; opacity: .8; }
  .actions { display: flex; gap: 6px; }
  button { background: #3f3f56; color: #fff; border: 0; border-radius: 8px; padding: 4px 10px; cursor: pointer; font-size: 12px; }
  button.close { background: transparent; font-size: 16px; padding: 0 4px; }
  @keyframes pulse { 50% { opacity: .4; } }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
