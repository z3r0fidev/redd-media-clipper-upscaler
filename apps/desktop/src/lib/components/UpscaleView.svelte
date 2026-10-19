<script lang="ts">
  // "Upscale for phone" controls: device preset, framing, quality tier, then "Upscale now"
  // (this job on its own) or "Add to queue". Defaults come from Settings › upscale_defaults, so
  // a phone-ready file takes three interactions (SC-007).
  import type { RangeArg } from "../api";
  import type { Motion } from "../bindings/Motion";
  import type { Reframe } from "../bindings/Reframe";
  import type { TierChoice } from "../bindings/TierChoice";
  import type { TierInfo } from "../bindings/TierInfo";
  import { app, toast } from "../stores/app.svelte";
  import { canRun, runManifest, runner } from "../stores/runner.svelte";
  import { addUpscale, onlyJob, upscaleClip } from "../stores/upscale";
  import { setOptions, upscale } from "../stores/upscale.svelte";
  import { baseName, fmt } from "../time";

  let { path, range }: { path: string; range: RangeArg } = $props();

  const o = $derived(upscale.options);
  const preset = $derived(upscale.presets?.presets.find((p) => p.id === o.preset));
  const tiers = $derived(app.engine?.tiers);
  const reframes: { value: Reframe; label: string; hint: string }[] = [
    { value: "smart", label: "Follow faces", hint: "Pans with the subject; blurred background where nobody fits" },
    { value: "center", label: "Centre crop", hint: "Crops the middle of the frame" },
    { value: "blur", label: "Whole frame", hint: "The whole frame over a blurred copy that fills the screen" },
  ];

  function tierState(t: TierChoice): { disabled: boolean; note: string } {
    if (t === "auto") return { disabled: false, note: "picks the fastest tier that works here" };
    const info: TierInfo | undefined = tiers?.[t];
    if (!info) return { disabled: t === "ai", note: t === "ai" ? "not available" : "not checked yet" };
    if (!info.available) return { disabled: true, note: info.reason ?? "not available" };
    return { disabled: false, note: info.fps ? `${Math.round(info.fps)} fps in the self-test` : "available" };
  }

  const rangeText = $derived(range.start <= 0 && range.end == null ? "Whole video" : `${fmt(range.start)} → ${range.end == null ? "end" : fmt(range.end)}`);
  const status = $derived.by(() => {
    if (o.reframe !== "smart") return "";
    if (upscale.analyzing) return `Finding faces… ${Math.round(upscale.progress * 100)}%`;
    if (upscale.analysisError) return upscale.analysisError;
    const a = upscale.analysis;
    if (!a) return "";
    const blurred = a.fallback_ranges.length;
    return `Crop path ready · ${a.shots.length} shot${a.shots.length === 1 ? "" : "s"}${blurred ? ` · ${blurred} blurred range${blurred === 1 ? "" : "s"}` : ""}`;
  });

  function entry() {
    return upscaleClip($state.snapshot(upscale.options), $state.snapshot(range));
  }

  function addToQueue() {
    const { queue } = addUpscale(app.queue, path, entry());
    app.queue = queue;
    toast(`Queued ${baseName(path)} for ${preset?.name ?? o.preset}`);
  }

  async function upscaleNow() {
    const { queue, index } = addUpscale(app.queue, path, entry());
    app.queue = queue;
    await runManifest(onlyJob(queue, path, index));
  }
</script>

<section class="controls" aria-label="Upscale for phone">
  <div class="field preset">
    <label for="up-preset" class="section-title">Phone</label>
    <select id="up-preset" value={o.preset} onchange={(e) => setOptions({ preset: e.currentTarget.value })}>
      {#each upscale.presets?.presets ?? [] as p (p.id)}
        <option value={p.id}>{p.name} · {p.width}×{p.height}</option>
      {/each}
    </select>
    {#if preset}<p class="faint note" title={preset.description}>{preset.devices.join(", ")}</p>{/if}
  </div>

  <div class="field">
    <span class="section-title" id="up-reframe">Framing</span>
    <div class="seg" role="radiogroup" aria-labelledby="up-reframe">
      {#each reframes as r (r.value)}
        <button role="radio" aria-checked={o.reframe === r.value} class:on={o.reframe === r.value} title={r.hint} onclick={() => setOptions({ reframe: r.value })}>{r.label}</button>
      {/each}
    </div>
    {#if o.reframe === "smart"}
      <div class="status" aria-live="polite">
        {#if upscale.analyzing}<div class="bar mini" role="progressbar" aria-label="Finding faces" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(upscale.progress * 100)}><div style="width: {Math.round(upscale.progress * 100)}%"></div></div>{/if}
        <span class="faint note" class:bad={!!upscale.analysisError}>{status}</span>
      </div>
    {/if}
  </div>

  <div class="field row2">
    {#if o.reframe === "smart"}
      <label>
        <span class="section-title">Camera</span>
        <select value={o.motion} onchange={(e) => setOptions({ motion: e.currentTarget.value as Motion })}>
          <option value="calm">Calm</option>
          <option value="normal">Normal</option>
          <option value="fast">Fast</option>
        </select>
      </label>
    {/if}
    <label>
      <span class="section-title">Quality</span>
      <select value={o.tier} onchange={(e) => setOptions({ tier: e.currentTarget.value as TierChoice })}>
        {#each [["auto", "Automatic"], ["cpu", "CPU"], ["gpu", "GPU"], ["ai", "AI"]] as [value, label] (value)}
          {@const s = tierState(value as TierChoice)}
          <option {value} disabled={s.disabled && o.tier !== value} title={s.note}>{label}{s.disabled ? " (unavailable)" : ""}</option>
        {/each}
      </select>
      <span class="faint note" title={tierState(o.tier).note}>{tierState(o.tier).note}</span>
    </label>
  </div>

  <footer>
    <span class="faint note" title="Mark In/Out in the player to upscale part of the video">{rangeText}</span>
    <span class="spacer"></span>
    <button onclick={addToQueue}>Add to queue</button>
    <button class="primary" disabled={!canRun()} onclick={upscaleNow}>{runner.busy ? "Working…" : "Upscale now"}</button>
  </footer>
</section>

<style>
  .controls { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-3) var(--space-4); align-items: start; }
  .field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .row2 { flex-direction: row; gap: var(--space-2); }
  .row2 label { display: flex; flex-direction: column; gap: 6px; flex: 1; min-width: 0; }
  select { width: 100%; min-width: 0; }
  .note { margin: 0; font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note.bad { color: var(--danger); white-space: normal; }
  .seg { display: flex; padding: 2px; gap: 2px; border-radius: var(--radius-m); background: var(--surface-2); border: 1px solid var(--border); }
  .seg button { flex: 1; min-width: 0; height: 28px; padding: 0 6px; font-size: 12px; border: 0; background: transparent; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .seg button.on { background: var(--surface); color: var(--text); box-shadow: var(--shadow); font-weight: 600; }
  .status { display: flex; align-items: center; gap: var(--space-2); min-height: 16px; }
  .bar.mini { width: 72px; height: 4px; flex-shrink: 0; }
  footer { grid-column: 1 / -1; display: flex; align-items: center; gap: var(--space-2); }
  .spacer { flex: 1; }
</style>
