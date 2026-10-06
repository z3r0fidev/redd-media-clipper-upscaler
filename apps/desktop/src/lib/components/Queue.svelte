<script lang="ts">
  import type { AudioSel } from "../bindings/AudioSel";
  import type { Mode } from "../bindings/Mode";
  import type { PartialOptions } from "../bindings/PartialOptions";
  import { api, errorText } from "../api";
  import type { TimeValue, VideoEntry } from "../manifest";
  import { app, infoFor, toast } from "../stores/app.svelte";
  import { clipCount, merge, removeClip, removeSource, runnable, setSourceOptions, updateClip } from "../stores/queue";
  import { applyEvent, initRun } from "../stores/run";
  import { baseName, fmt } from "../time";

  let invalid = $state<Record<string, string>>({});
  let busy = $state(false);

  const total = $derived(clipCount(app.queue));

  function show(v: TimeValue | undefined): string {
    if (v === undefined) return "";
    return typeof v === "number" ? fmt(v) : v;
  }

  async function editTime(input: string, i: number, field: "start" | "end", text: string) {
    const key = `${input}#${i}#${field}`;
    try {
      const value = field === "start" ? await api.parseTime(text) : await api.parseEnd(text || "end");
      delete invalid[key];
      app.queue = updateClip(app.queue, input, i, { [field]: value });
    } catch (e) {
      invalid[key] = errorText(e);
    }
  }

  function audioOptions(input: string): { label: string; value: string }[] {
    const tracks = (infoFor(input)?.streams ?? []).filter((s) => s.kind === "audio");
    return [
      { label: "All audio", value: "all" },
      { label: "No audio", value: "none" },
      ...tracks.map((t, n) => ({ label: `Track ${n + 1}${t.language ? ` (${t.language})` : ""} · ${t.codec}`, value: String(n + 1) })),
    ];
  }

  function audioValue(v: VideoEntry): string {
    const a = v.options?.audio;
    return a === undefined ? "all" : Array.isArray(a) ? String(a[0]) : a;
  }

  function setOption(v: VideoEntry, patch: PartialOptions) {
    app.queue = setSourceOptions(app.queue, v.input, { ...v.options, ...patch });
  }

  function setAudio(v: VideoEntry, value: string) {
    const audio: AudioSel | undefined = value === "all" ? undefined : value === "none" ? "none" : [Number(value)];
    setOption(v, { audio });
  }

  async function importClipList() {
    const targets = app.selected ? [app.selected] : app.queue.videos.map((v) => v.input);
    if (!targets.length) return toast("Select a source first", "error");
    try {
      const [list] = await api.pickFiles("cliplist");
      if (!list) return;
      app.queue = merge(app.queue, await api.loadCliplist(list, targets));
      toast(`Imported clips from ${baseName(list)}`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  async function exportQueue() {
    try {
      const path = await api.pickSavePath("rmedia-queue.json");
      if (!path) return;
      await api.exportManifest(runnable(app.queue), path);
      toast(`Saved ${baseName(path)} — run it with: rmedia batch "${baseName(path)}"`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  async function run() {
    busy = true;
    app.runIssues = [];
    try {
      const plan = await api.plan(runnable(app.queue));
      const errors = plan.issues.filter((i) => i.severity === "error");
      if (errors.length) {
        app.runIssues = errors.map((i) => i.message);
        app.run = null;
        return;
      }
      app.run = initRun(plan);
      const summary = await api.run(plan.run_id, (e) => {
        if (app.run) app.run = applyEvent(app.run, e);
      });
      if (app.run) app.run = { ...app.run, finished: true, summary };
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      busy = false;
    }
  }
</script>

<section class="queue">
  <header>
    <span class="section-title">Queue</span>
    <span class="faint">{total} clip{total === 1 ? "" : "s"}</span>
    <span class="spacer"></span>
    <button class="ghost" onclick={importClipList} title="Apply a clip list (.txt) to the selected source">Import list…</button>
    <button class="ghost" onclick={exportQueue} disabled={!total} title="Save as a manifest for `rmedia batch`">Export…</button>
  </header>

  <div class="list">
    {#each app.queue.videos as v (v.input)}
      <article class="card">
        <div class="card-head">
          <button class="ghost name" onclick={() => (app.selected = v.input)} title={v.input}>{baseName(v.input)}</button>
          <button class="icon ghost" aria-label="Remove {baseName(v.input)} from the queue" onclick={() => (app.queue = removeSource(app.queue, v.input))}>×</button>
        </div>
        <div class="opts">
          <select aria-label="Mode for {baseName(v.input)}" value={v.options?.mode ?? ""} onchange={(e) => setOption(v, { mode: (e.currentTarget.value || undefined) as Mode | undefined })}>
            <option value="">Default mode</option>
            <option value="fast">Fast (no re-encode)</option>
            <option value="accurate">Frame-accurate</option>
          </select>
          <select aria-label="Audio for {baseName(v.input)}" value={audioValue(v)} onchange={(e) => setAudio(v, e.currentTarget.value)}>
            {#each audioOptions(v.input) as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
          </select>
          <select aria-label="Subtitles for {baseName(v.input)}" value={v.options?.subtitles ?? "all"} onchange={(e) => setOption(v, { subtitles: e.currentTarget.value === "none" ? "none" : undefined })}>
            <option value="all">Keep subtitles</option>
            <option value="none">No subtitles</option>
          </select>
        </div>
        {#each v.clips as c, i (i)}
          <div class="row">
            <input
              type="text"
              class="mono t"
              class:invalid={invalid[`${v.input}#${i}#start`]}
              title={invalid[`${v.input}#${i}#start`] ?? "Start"}
              value={show(c.start)}
              onchange={(e) => editTime(v.input, i, "start", e.currentTarget.value)}
              aria-label="Start"
            />
            <span class="faint">→</span>
            <input
              type="text"
              class="mono t"
              class:invalid={invalid[`${v.input}#${i}#end`]}
              title={invalid[`${v.input}#${i}#end`] ?? "End: a time, +duration, or end"}
              value={c.duration !== undefined ? `+${c.duration}` : show(c.end ?? "end")}
              onchange={(e) => editTime(v.input, i, "end", e.currentTarget.value)}
              aria-label="End"
            />
            <input type="text" class="n" placeholder="name" value={c.name ?? ""} onchange={(e) => (app.queue = updateClip(app.queue, v.input, i, { name: e.currentTarget.value }))} aria-label="Clip name" />
            <button class="icon ghost" aria-label="Remove clip" onclick={() => (app.queue = removeClip(app.queue, v.input, i))}>×</button>
          </div>
        {:else}
          <p class="faint hint">No clips yet — mark in/out in the player.</p>
        {/each}
      </article>
    {:else}
      <p class="faint empty">Clips you add appear here.</p>
    {/each}
  </div>

  {#if app.runIssues.length}
    <div class="issues" role="alert">
      <b>Fix these before running:</b>
      <ul>{#each app.runIssues as m, i (i)}<li>{m}</li>{/each}</ul>
    </div>
  {/if}

  <footer>
    <span class="faint">Output: {app.settings?.defaults.output_dir ?? "“clips” folder next to each video"}</span>
    <button class="primary" disabled={!total || busy || (app.run != null && !app.run.finished)} onclick={run}>
      {busy ? "Running…" : `Run ${total} clip${total === 1 ? "" : "s"}`}
    </button>
  </footer>
</section>

<style>
  .queue { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-4); min-height: 0; height: 100%; }
  header, footer, .card-head, .row, .opts { display: flex; align-items: center; gap: var(--space-2); }
  .spacer { flex: 1; }
  .list { flex: 1; overflow: auto; display: flex; flex-direction: column; gap: var(--space-3); }
  .card { border: 1px solid var(--border); border-radius: var(--radius-m); padding: var(--space-2) var(--space-3) var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); background: var(--surface); }
  .card-head { justify-content: space-between; }
  .name { font-weight: 650; padding: 0 var(--space-1); max-width: 280px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; display: block; text-align: left; }
  .opts select { flex: 1; min-width: 0; font-size: 12px; height: 28px; }
  .t { width: 112px; font-size: 12.5px; }
  .n { flex: 1; min-width: 0; }
  .hint, .empty { margin: var(--space-1) 0; font-size: 12.5px; }
  .empty { text-align: center; padding: var(--space-5) 0; }
  .issues { border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent); background: color-mix(in srgb, var(--danger) 8%, var(--surface)); border-radius: var(--radius-m); padding: var(--space-2) var(--space-3); font-size: 12.5px; user-select: text; }
  .issues ul { margin: var(--space-1) 0 0; padding-left: 18px; }
  footer { justify-content: space-between; }
  footer .faint { font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
