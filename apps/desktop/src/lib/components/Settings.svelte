<script lang="ts">
  import type { Settings } from "../bindings/Settings";
  import type { TierInfo } from "../bindings/TierInfo";
  import { api, errorText } from "../api";
  import { app, toast } from "../stores/app.svelte";

  let draft = $state<Settings | null>(app.settings ? structuredClone($state.snapshot(app.settings)) : null);

  async function chooseOutput() {
    try {
      const dir = await api.pickFolder();
      if (dir && draft) draft.defaults.output_dir = dir;
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  async function chooseEngine(reset: boolean) {
    try {
      app.engine = await api.pickEngine(reset);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  let checking = $state(false);

  async function recheck() {
    checking = true;
    try {
      app.engine = await api.doctorRecheck();
      toast("Machine checked");
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      checking = false;
    }
  }

  function tierText(info: TierInfo): string {
    if (!info.available) return `no (${info.reason ?? "not detected"})`;
    return info.fps ? `yes, ${Math.round(info.fps)} fps` : "yes";
  }

  async function save() {
    if (!draft) return;
    try {
      const s = $state.snapshot(draft) as Settings;
      await api.setSettings(s);
      app.settings = s;
      app.engine = await api.doctor();
      app.showSettings = false;
      toast("Settings saved");
    } catch (e) {
      toast(errorText(e), "error");
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && (app.showSettings = false)} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (app.showSettings = false)}>
  <div class="dialog panel" role="dialog" aria-modal="true" aria-labelledby="settings-title">
    <h2 id="settings-title">Settings</h2>
    {#if draft}
      <fieldset>
        <legend class="section-title">New clips</legend>
        <label>Mode
          <select bind:value={draft.defaults.mode}>
            <option value={undefined}>Fast (no re-encode)</option>
            <option value="accurate">Frame-accurate</option>
          </select>
        </label>
        <label>Output folder
          <span class="row">
            <input type="text" readonly value={draft.defaults.output_dir ?? "“clips” next to each video"} />
            <button onclick={chooseOutput}>Choose…</button>
            {#if draft.defaults.output_dir}<button class="ghost" onclick={() => draft && delete draft.defaults.output_dir}>Reset</button>{/if}
          </span>
        </label>
        <label>If the file exists
          <select bind:value={draft.defaults.on_exists}>
            <option value={undefined}>Skip it</option>
            <option value="rename">Add a number</option>
            <option value="overwrite">Replace it</option>
          </select>
        </label>
        <label>File names
          <input type="text" class="mono" placeholder={"{source}_{index:03}_{name-or-range}.{ext}"} bind:value={draft.defaults.name_template} />
        </label>
      </fieldset>
      <fieldset>
        <legend class="section-title">Performance</legend>
        <label>Clips at once
          <input type="number" min="1" max="64" placeholder="auto" bind:value={draft.jobs} />
        </label>
        <label>Preview cache (MB)
          <input type="number" min="100" step="100" bind:value={draft.proxy_cache_max_mb} />
        </label>
      </fieldset>
    {/if}
    <fieldset>
      <legend class="section-title">Video engine</legend>
      {#if app.engine}
        <p class="mono small">{app.engine.ffmpeg?.path ?? "not found"} <span class="pill">{app.engine.ffmpeg?.source ?? "—"}</span></p>
        <span class="row">
          <button class="small" onclick={() => chooseEngine(false)}>Choose ffmpeg…</button>
          {#if app.engine.ffmpeg?.source === "config"}<button class="ghost small" onclick={() => chooseEngine(true)}>Use automatic</button>{/if}
        </span>
        <p class="small">Version {app.engine.version ?? "?"} · accurate mode writes {app.engine.accurate_containers.join(", ") || "nothing"}</p>
        {#each app.engine.problems as p, i (i)}<p class="small bad">{p}</p>{/each}
      {/if}
    </fieldset>
    <fieldset>
      <legend class="section-title">Upscaling quality tiers</legend>
      {#if app.engine?.tiers}
        {@const t = app.engine.tiers}
        <p class="small">CPU: {tierText(t.cpu)} · GPU: {tierText(t.gpu)} · AI: {tierText(t.ai)}</p>
        {#each app.engine.gpus as g (g.index)}
          <p class="small mono">{g.name} — {g.usable ? "usable" : `not used: ${g.reason ?? "unknown"}`}</p>
        {/each}
        {#if app.engine.capability}
          <p class="small">Checked {app.engine.capability.checked_at}{#each app.engine.capability.demoted as d (d.tier)} · {d.tier} failed since: {d.error}{/each}</p>
        {/if}
      {:else}
        <p class="small">Not checked yet.</p>
      {/if}
      <button class="small" disabled={checking} onclick={recheck}>{checking ? "Checking…" : "Check again"}</button>
    </fieldset>
    <footer>
      <button class="ghost" onclick={() => (app.showSettings = false)}>Cancel</button>
      <button class="primary" onclick={save}>Save</button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(8, 10, 16, 0.5); backdrop-filter: blur(6px); display: grid; place-items: center; z-index: 10; }
  .dialog { width: min(560px, 92vw); max-height: 88vh; overflow: auto; padding: var(--space-5); display: flex; flex-direction: column; gap: var(--space-4); animation: rise 180ms var(--ease); }
  h2 { margin: 0; font-size: 17px; }
  fieldset { border: 0; padding: 0; margin: 0; display: flex; flex-direction: column; gap: var(--space-2); }
  legend { margin-bottom: var(--space-2); }
  label { display: grid; grid-template-columns: 150px 1fr; align-items: center; gap: var(--space-3); color: var(--text-2); }
  .row { display: flex; gap: var(--space-2); }
  .row input { flex: 1; min-width: 0; }
  .small { margin: 0; font-size: 12.5px; user-select: text; }
  .bad { color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: var(--space-2); }
</style>
