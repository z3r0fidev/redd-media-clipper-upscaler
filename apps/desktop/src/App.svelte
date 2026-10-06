<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "./lib/api";
  import Player from "./lib/components/Player.svelte";
  import Queue from "./lib/components/Queue.svelte";
  import RunPanel from "./lib/components/RunPanel.svelte";
  import Settings from "./lib/components/Settings.svelte";
  import Sources from "./lib/components/Sources.svelte";
  import { app, toast } from "./lib/stores/app.svelte";

  onMount(async () => {
    try {
      app.settings = await api.getSettings();
      app.engine = await api.doctor();
      if (!app.engine.ok) toast(app.engine.problems.join("; "), "error");
    } catch (e) {
      toast(errorText(e), "error");
    }
  });
</script>

<div class="shell">
  <header class="top">
    <div class="brand"><span class="mark">▶</span> rmedia</div>
    <span class="spacer"></span>
    {#if app.engine && !app.engine.ok}
      <button class="pill bad" onclick={() => (app.showSettings = true)} title={app.engine.problems.join("; ")}>● Video engine missing — fix in Settings</button>
    {/if}
    <button class="ghost" onclick={() => (app.showSettings = true)}>Settings</button>
  </header>

  <main>
    <aside class="panel side"><Sources /></aside>
    <div class="center panel"><Player /></div>
    <aside class="panel side"><Queue /></aside>
  </main>

  <RunPanel />

  {#if app.toast}
    <div class="toast panel" class:error={app.toast.kind === "error"} role={app.toast.kind === "error" ? "alert" : "status"}>
      <span>{app.toast.text}</span>
      <button class="icon ghost" aria-label="Dismiss" onclick={() => (app.toast = null)}>×</button>
    </div>
  {/if}
  {#if app.showSettings}<Settings />{/if}
</div>

<style>
  .shell { height: 100%; display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-3); }
  .top { display: flex; align-items: center; gap: var(--space-3); padding: 0 var(--space-2); }
  .brand { display: flex; align-items: center; gap: var(--space-2); font-weight: 750; font-size: 15px; letter-spacing: -0.01em; }
  .mark { display: grid; place-items: center; width: 24px; height: 24px; border-radius: 7px; font-size: 11px; color: white; background: linear-gradient(135deg, var(--accent-2), var(--accent)); }
  .spacer { flex: 1; }
  main { flex: 1; min-height: 360px; display: grid; grid-template-columns: minmax(200px, 260px) minmax(380px, 1fr) minmax(300px, 380px); gap: var(--space-3); }
  @media (max-width: 1100px) { main { grid-template-columns: 200px minmax(0, 1fr) 300px; } }
  main > * { min-height: 0; overflow: hidden; }
  /* The player is the hero; side panes sit slightly back. */
  .side { background: color-mix(in srgb, var(--surface) 70%, var(--bg)); box-shadow: none; }
  .pill.bad { height: 26px; border: 0; cursor: pointer; }
  .toast { position: fixed; bottom: var(--space-5); left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: var(--space-2); padding: var(--space-1) var(--space-1) var(--space-1) var(--space-4); max-width: 70vw; z-index: 20; user-select: text; animation: rise 200ms var(--ease); }
  .toast.error { border-color: color-mix(in srgb, var(--danger) 50%, transparent); color: var(--danger); }
</style>
