<script lang="ts">
  import { api } from "../api";
  import { app } from "../stores/app.svelte";
  import { statusLabel, upscaleDetail, type JobView } from "../stores/run";
  import { baseName } from "../time";

  const run = $derived(app.run);
  let expanded = $state(false);
  // Open the details automatically when something fails.
  $effect(() => {
    if (run?.jobs.some((j) => j.status === "failed")) expanded = true;
  });
  const eta = (ms?: number) => (ms == null ? "" : ms < 60_000 ? `${Math.ceil(ms / 1000)} s left` : `${Math.ceil(ms / 60_000)} min left`);

  function detail(j: JobView): string {
    const o = j.outcome;
    if (!o) return j.warnings.join(" · ");
    if (o.status === "failed") return o.error.message;
    if (o.status === "skipped") return o.reason;
    if (o.status === "succeeded" && o.measure === "clip" && o.adjusted) {
      // `adjusted` also covers an end that moved (e.g. clamped to the media end).
      if (Math.abs(o.actual.start - j.requestedStart) > 0.05) return `Starts on the keyframe at ${o.actual.start.toFixed(3)} s`;
      return `Ends at ${o.actual.end.toFixed(3)} s`;
    }
    return [upscaleDetail(o), ...j.warnings].filter(Boolean).join(" · ");
  }
</script>

{#if run}
  <section class="run panel" aria-label="Run">
    <header>
      <button class="icon ghost" aria-expanded={expanded} aria-controls="run-jobs" onclick={() => (expanded = !expanded)} title={expanded ? "Hide jobs" : "Show jobs"}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" style="transform: rotate({expanded ? 180 : 0}deg); transition: transform 200ms var(--ease)"><path d="M6 15l6-6 6 6" /></svg>
      </button>
      <strong>{run.finished ? "Finished" : "Running"}</strong>
      <span class="muted">{run.done} / {run.total} jobs</span>
      <div class="bar grow" role="progressbar" aria-label="Overall progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(run.fraction * 100)}><div style="width: {Math.round(run.fraction * 100)}%"></div></div>
      <span class="faint">{run.finished ? "" : eta(run.etaMs)}</span>
      {#if !run.finished}
        <button class="danger" onclick={() => api.cancelRun(run.runId)}>Cancel all</button>
      {:else}
        <button class="ghost" onclick={() => (app.run = null)}>Close</button>
      {/if}
    </header>
    {#if run.summary}
      <p class="summary" aria-live="polite">
        <span class="ok">{run.summary.succeeded} succeeded</span>
        {#if run.summary.skipped}<span>· {run.summary.skipped} skipped</span>{/if}
        {#if run.summary.failed}<span class="bad">· {run.summary.failed} failed</span>{/if}
        {#if run.summary.cancelled}<span>· {run.summary.cancelled} cancelled</span>{/if}
      </p>
    {/if}
    {#if expanded}
    <ul id="run-jobs">
      {#each run.jobs as j (j.id)}
        <li class={j.status}>
          <span class="status">{statusLabel(j)}</span>
          <span class="out" title={j.output}>{baseName(j.output)}</span>
          {#if j.status === "running"}
            <div class="bar mini"><div style="width: {Math.round(j.fraction * 100)}%"></div></div>
            <span class="faint">{eta(j.etaMs)}</span>
            <button class="ghost" onclick={() => api.cancelRun(run.runId, j.id)}>Cancel</button>
          {:else}
            <span class="detail" title={detail(j)}>{detail(j)}</span>
          {/if}
        </li>
      {/each}
    </ul>
    {/if}
  </section>
{/if}

<style>
  .run { display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-2) var(--space-4); max-height: min(36vh, 280px); flex-shrink: 0; animation: rise 200ms var(--ease); }
  header { display: flex; align-items: center; gap: var(--space-3); }
  .grow { flex: 1; }
  .summary { margin: 0; display: flex; gap: var(--space-2); font-size: 12.5px; }
  .ok { color: var(--ok); }
  .bad { color: var(--danger); }
  ul { list-style: none; margin: 0; padding: 0; overflow: auto; display: flex; flex-direction: column; gap: 2px; }
  li { display: grid; grid-template-columns: 76px minmax(120px, 1.2fr) minmax(0, 2fr) auto auto; align-items: center; gap: var(--space-2); padding: 4px var(--space-2); border-radius: var(--radius-s); font-size: 12.5px; user-select: text; }
  li:nth-child(odd) { background: var(--surface-2); }
  .status { font-weight: 600; color: var(--text-2); }
  li.done .status { color: var(--ok); }
  li.failed .status { color: var(--danger); }
  li.running .status { color: var(--accent); }
  .out, .detail { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .detail { grid-column: span 3; color: var(--text-2); }
  .mini { height: 5px; }
</style>
