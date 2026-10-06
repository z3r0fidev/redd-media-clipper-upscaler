<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "../api";
  import { app, toast, type SourceItem } from "../stores/app.svelte";
  import { addSource, merge } from "../stores/queue";
  import { baseName, fmt } from "../time";

  let scanning = $state<{ id: string; text: string } | null>(null);

  function upsert(item: SourceItem) {
    const i = app.sources.findIndex((s) => s.path === item.path);
    if (i >= 0) app.sources[i] = { ...app.sources[i], ...item };
    else app.sources.push(item);
    if (!app.selected) app.selected = item.path;
  }

  async function addFiles(paths: string[]) {
    for (const path of paths) {
      upsert({ path });
      try {
        upsert({ path, info: await api.probe(path) });
      } catch (e) {
        upsert({ path, error: errorText(e) });
      }
    }
  }

  async function scanFolder(root: string) {
    const id = crypto.randomUUID();
    scanning = { id, text: `Scanning ${baseName(root)}…` };
    try {
      const result = await api.scan(id, [root], {}, (e) => {
        if (e.type === "progress" && scanning) {
          scanning.text = `Checked ${e.probed} of ${e.candidates} candidate files`;
        } else if (e.type === "found") {
          upsert({ path: e.video.path, info: e.video });
        }
      });
      const rejected = result.rejected.length ? `, ${result.rejected.length} skipped (not playable video)` : "";
      toast(`Found ${result.videos.length} video(s) in ${baseName(root)}${rejected}`);
    } catch (e) {
      const msg = errorText(e);
      if (msg !== "cancelled") toast(msg, "error");
    } finally {
      scanning = null;
    }
  }

  async function pickVideos() {
    try {
      await addFiles(await api.pickFiles("video"));
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  async function pickFolder() {
    try {
      const dir = await api.pickFolder();
      if (dir) await scanFolder(dir);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  async function openManifest() {
    try {
      const [path] = await api.pickFiles("manifest");
      if (!path) return;
      const m = await api.loadManifest(path);
      app.queue = merge(app.queue, m);
      if (m.defaults && Object.keys(m.defaults).length) app.queue.defaults = { ...app.queue.defaults, ...m.defaults };
      await addFiles(m.videos.map((v) => v.input));
      toast(`Loaded ${m.videos.length} video(s) from ${baseName(path)}`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  function addAllToQueue() {
    let q = app.queue;
    for (const s of app.sources) if (s.info) q = addSource(q, s.path);
    app.queue = q;
  }

  function remove(path: string) {
    app.sources = app.sources.filter((s) => s.path !== path);
    if (app.selected === path) app.selected = app.sources[0]?.path ?? null;
  }

  // Dropped / command-line paths were already registered by the Rust side; folders are scanned.
  async function openPaths(paths: string[]) {
    for (const p of paths) {
      try {
        upsert({ path: p, info: await api.probe(p) });
      } catch {
        await scanFolder(p);
      }
    }
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

<section class="sources">
  <header>
    <span class="section-title">Sources</span>
    <span class="faint">{app.sources.length}</span>
  </header>

  <div class="actions">
    <button class="primary" onclick={pickVideos}>Add videos</button>
    <button onclick={pickFolder} disabled={!!scanning}>Add folder</button>
    <button class="ghost" onclick={openManifest} title="Open a .json or .csv batch manifest">Open manifest…</button>
  </div>

  {#if scanning}
    <div class="scan">
      <div class="spinner" aria-hidden="true"></div>
      <span class="muted">{scanning.text}</span>
      <button class="ghost" onclick={() => scanning && api.cancelScan(scanning.id)}>Cancel</button>
    </div>
  {/if}

  <ul role="listbox" aria-label="Source videos">
    {#each app.sources as s (s.path)}
      <li class:selected={app.selected === s.path} role="option" aria-selected={app.selected === s.path}>
        <button class="item" onclick={() => (app.selected = s.path)} title={s.path}>
          <span class="name">{baseName(s.path)}</span>
          {#if s.error}
            <span class="err">{s.error}</span>
          {:else if s.info}
            <span class="meta mono">{fmt(s.info.duration)} · {resolution(s)}</span>
          {:else}
            <span class="faint">reading…</span>
          {/if}
        </button>
        <button class="icon ghost remove" aria-label="Remove {baseName(s.path)}" onclick={() => remove(s.path)}>×</button>
      </li>
    {:else}
      <li class="empty">
        <p>Drop videos or folders here,</p>
        <p class="faint">or use <b>Add videos</b> / <b>Add folder</b>.</p>
      </li>
    {/each}
  </ul>

  {#if app.sources.length > 1}
    <button class="ghost all" onclick={addAllToQueue}>Add all to queue</button>
  {/if}
</section>

<style>
  .sources { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-4); min-height: 0; height: 100%; }
  header { display: flex; justify-content: space-between; align-items: baseline; }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
  .scan { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-radius: var(--radius-m); background: var(--accent-soft); }
  .scan span { flex: 1; font-size: 12.5px; }
  .spinner { width: 14px; height: 14px; border: 2px solid var(--accent); border-right-color: transparent; border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  ul { list-style: none; margin: 0; padding: 0; overflow: auto; flex: 1; display: flex; flex-direction: column; gap: 2px; }
  li { display: flex; align-items: center; border-radius: var(--radius-m); transition: background 120ms var(--ease); }
  li:hover { background: var(--surface-2); }
  li.selected { background: var(--accent-soft); }
  .item { flex: 1; min-width: 0; height: auto; padding: var(--space-2) var(--space-3); border: 0; background: transparent; flex-direction: column; align-items: flex-start; gap: 2px; text-align: left; }
  .item:hover { background: transparent; }
  .name { font-weight: 600; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: 12px; color: var(--text-2); }
  .err { font-size: 12px; color: var(--danger); }
  .remove { opacity: 0; }
  li:hover .remove, .remove:focus-visible { opacity: 1; }
  .empty { flex-direction: column; align-items: center; justify-content: center; text-align: center; padding: var(--space-5) var(--space-3); border: 1.5px dashed var(--border); color: var(--text-2); }
  .empty:hover { background: transparent; }
  .empty p { margin: 2px; }
  .all { align-self: flex-start; }
</style>
