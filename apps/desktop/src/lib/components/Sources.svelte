<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { app, type SourceItem } from "../stores/app.svelte";
  import { addSource } from "../stores/queue";
  import { openManifest, openPaths, pickFolder, pickVideos, removeSourceItem, scan } from "../stores/sources.svelte";
  import { baseName, fmt } from "../time";

  function addAllToQueue() {
    let q = app.queue;
    for (const s of app.sources) if (s.info) q = addSource(q, s.path);
    app.queue = q;
  }

  onMount(() => {
    const un = api.onDropped(openPaths);
    api.takeOpened().then(openPaths).catch(() => {});
    return () => void un.then((f) => f());
  });

  function resolution(s: SourceItem) {
    const v = s.info?.streams.find((x) => x.kind === "video" && !x.is_attached_pic);
    return v?.width && v.height ? `${v.width}×${v.height}` : "";
  }
</script>

<section class="sources" aria-label="Sources">
  <header class="pane-head">
    <span class="section-title">Sources</span>
    <span class="faint">{app.sources.length}</span>
    <span class="spacer"></span>
    <button class="ghost small" onclick={openManifest} title="Open a .json or .csv batch manifest">Open manifest…</button>
  </header>

  <div class="actions">
    <button class="primary" onclick={pickVideos}>Add videos</button>
    <button onclick={pickFolder} disabled={!!scan.current}>Add folder</button>
  </div>

  {#if scan.current}
    <div class="scan" role="status">
      <div class="spinner" aria-hidden="true"></div>
      <span class="muted">{scan.current.text}</span>
      <button class="ghost small" onclick={() => scan.current && api.cancelScan(scan.current.id)}>Cancel</button>
    </div>
  {/if}

  <ul aria-label="Source videos">
    {#each app.sources as s (s.path)}
      <li class:selected={app.selected === s.path}>
        <button class="item" aria-current={app.selected === s.path} onclick={() => (app.selected = s.path)} title={s.path}>
          <span class="name">{baseName(s.path)}</span>
          {#if s.error}
            <span class="err">{s.error}</span>
          {:else if s.info}
            <span class="meta mono">{fmt(s.info.duration)} · {resolution(s)}</span>
          {:else}
            <span class="faint">reading…</span>
          {/if}
        </button>
        <button class="icon ghost remove" title="Remove" aria-label="Remove {baseName(s.path)}" onclick={() => removeSourceItem(s.path)}>×</button>
      </li>
    {:else}
      <li class="empty">
        <p>Drop videos or folders anywhere,</p>
        <p class="faint">or use <b>Add videos</b> / <b>Add folder</b>.</p>
      </li>
    {/each}
  </ul>

  {#if app.sources.length > 1}
    <button class="ghost all" onclick={addAllToQueue}>Add all to queue</button>
  {/if}
</section>

<style>
  .sources { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-3) var(--space-4) var(--space-4); min-height: 0; height: 100%; }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
  .scan { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-radius: var(--radius-m); background: var(--accent-soft); }
  .scan span { flex: 1; font-size: 12.5px; }
  .spinner { width: 14px; height: 14px; border: 2px solid var(--accent); border-right-color: transparent; border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  ul { list-style: none; margin: 0; padding: 0; overflow: auto; flex: 1; display: flex; flex-direction: column; gap: 2px; }
  li { display: flex; align-items: center; border-radius: var(--radius-m); transition: background 120ms var(--ease); }
  li:hover { background: var(--surface-2); }
  li.selected { background: var(--accent-soft); box-shadow: inset 3px 0 0 var(--accent); }
  .item { flex: 1; min-width: 0; height: auto; padding: var(--space-2) var(--space-3); border: 0; background: transparent; flex-direction: column; align-items: flex-start; gap: 2px; text-align: left; white-space: normal; }
  .item:hover { background: transparent; }
  .name { font-weight: 600; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: 12px; color: var(--text-2); }
  .err { font-size: 12px; color: var(--danger); }
  .remove { opacity: 0; }
  li:hover .remove, li.selected .remove, .remove:focus-visible { opacity: 1; }
  .empty { flex-direction: column; align-items: center; justify-content: center; text-align: center; padding: var(--space-5) var(--space-3); border: 1.5px dashed var(--border); color: var(--text-2); }
  .empty:hover { background: transparent; }
  .empty p { margin: 2px; }
  .all { align-self: flex-start; }
</style>
