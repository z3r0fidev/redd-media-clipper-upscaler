<script lang="ts">
  // "GIF for Reddit" controls: target, format, shape and framing, every CLI override in a
  // drawer, the live size estimate, then "Export now" (this job on its own) or "Add to queue",
  // and the result card. Defaults come from Settings › gif_defaults, so a GIF takes three
  // interactions (SC-007): the tab, the target (or the default), Export now.
  import type { RangeArg } from "../api";
  import type { Dither } from "../bindings/Dither";
  import type { GifShape } from "../bindings/GifShape";
  import type { Reframe } from "../bindings/Reframe";
  import { toast } from "../stores/app.svelte";
  import { megabytes, resolveBudget } from "../stores/gif";
  import { addGifToQueue, exportNow, gif, setGifOptions } from "../stores/gif.svelte";
  import { canRun, runner } from "../stores/runner.svelte";
  import { baseName, fmt } from "../time";

  let { path, range }: { path: string; range: RangeArg } = $props();

  const o = $derived(gif.options);
  const target = $derived(gif.targets?.targets.find((t) => t.id === o.target));
  const shapes: { value: GifShape; label: string }[] = [
    { value: "source", label: "As filmed" },
    { value: "9:16", label: "9:16" },
    { value: "4:5", label: "4:5" },
    { value: "1:1", label: "1:1" },
  ];
  const framings: { value: Reframe; label: string; hint: string }[] = [
    { value: "smart", label: "Follow faces", hint: "Pans with the person; a blurred background where nobody fits" },
    { value: "center", label: "Centre", hint: "Crops the middle of the picture" },
    { value: "blur", label: "Show all", hint: "The whole picture over a blurred copy" },
  ];
  const dithers: { value: string; label: string; dither: Dither | null }[] = [
    { value: "", label: "The target's", dither: null },
    { value: "bayer:4", label: "Ordered (bayer 4)", dither: { bayer: 4 } },
    { value: "bayer:5", label: "Ordered (bayer 5)", dither: { bayer: 5 } },
    { value: "floyd_steinberg", label: "Floyd–Steinberg", dither: "floyd_steinberg" },
    { value: "sierra2_4a", label: "Sierra", dither: "sierra2_4a" },
    { value: "none", label: "None", dither: "none" },
  ];
  const ditherKey = (d: Dither | undefined) => (d == null ? "" : typeof d === "string" ? d : `bayer:${d.bayer}`);

  let drawer = $state(false);
  const validRange = $derived(range.end == null || range.end > range.start);
  const rangeText = $derived(
    !validRange ? "Out must be after In" : range.start <= 0 && range.end == null ? "Whole video" : `Part: ${fmt(range.start)} → ${range.end == null ? "end" : fmt(range.end)}`,
  );
  const blockedWhy = $derived(!validRange ? "Set In before Out" : !canRun() ? "Another run is in progress" : "");
  const budgetText = $derived.by(() => {
    const b = o.budget === null ? null : resolveBudget(o.budget ?? {}, target?.budget ?? null);
    return b ? `aim ${megabytes(b.soft)}, at most ${megabytes(b.hard)}` : "no size limit";
  });
  const est = $derived(gif.estimate?.estimate ?? null);
  const loweredText = (l: { setting: string; from: unknown; to: unknown }[]) =>
    l.map((x) => `${x.setting} ${JSON.stringify(x.from)} → ${JSON.stringify(x.to)}`).join(", ");
  const measure = $derived(gif.result && gif.result.job.status === "succeeded" && gif.result.job.measure === "gif" ? gif.result.job : null);

  /** A number field: empty = the target's value. */
  function num(e: Event & { currentTarget: HTMLInputElement }): number | undefined {
    const v = e.currentTarget.value.trim();
    return v === "" ? undefined : Number(v);
  }

  function addToQueue() {
    if (!validRange) return;
    addGifToQueue(path, $state.snapshot(range));
    toast(`Queued a ${o.format === "mp4" ? "Reddit MP4" : "GIF"} of ${baseName(path)}`);
  }
</script>

<section class="controls" aria-label="GIF for Reddit">
  <div class="field">
    <label for="gif-target" class="section-title">Target</label>
    <select id="gif-target" value={o.target} onchange={(e) => setGifOptions({ target: e.currentTarget.value })}>
      {#each gif.targets?.targets ?? [] as t (t.id)}
        <option value={t.id}>{t.name}{t.verified ? "" : " (limits unverified)"}</option>
      {/each}
    </select>
    <p class="faint note">{budgetText}</p>
  </div>

  <div class="field">
    <span class="section-title" id="gif-format">Format</span>
    <div class="seg" role="group" aria-labelledby="gif-format">
      <button aria-pressed={o.format === "gif"} class:on={o.format === "gif"} onclick={() => setGifOptions({ format: "gif" })}>GIF</button>
      <button aria-pressed={o.format === "mp4"} class:on={o.format === "mp4"} title="Reddit converts GIFs to video anyway; this is much smaller" onclick={() => setGifOptions({ format: "mp4" })}>Reddit MP4 (recommended for posts)</button>
    </div>
  </div>

  <div class="field">
    <span class="section-title" id="gif-shape">Shape</span>
    <div class="seg" role="group" aria-labelledby="gif-shape">
      {#each shapes as s (s.value)}
        <button aria-pressed={o.shape === s.value} class:on={o.shape === s.value} onclick={() => setGifOptions({ shape: s.value })}>{s.label}</button>
      {/each}
    </div>
  </div>

  <div class="field">
    <span class="section-title" id="gif-framing">Framing</span>
    <div class="seg" role="group" aria-labelledby="gif-framing">
      {#each framings as f (f.value)}
        <button aria-pressed={o.framing === f.value} class:on={o.framing === f.value} title={f.hint} disabled={o.shape === "source"} onclick={() => setGifOptions({ framing: f.value })}>{f.label}</button>
      {/each}
    </div>
  </div>

  <details class="drawer" bind:open={drawer}>
    <summary>More settings</summary>
    <div class="grid">
      <label>Width <input type="number" min="64" max="1920" value={o.width ?? ""} placeholder="target's" onchange={(e) => setGifOptions({ width: num(e) })} /></label>
      <label>Frames per second <input type="number" min="1" max={o.format === "mp4" ? 60 : 50} value={o.fps ?? ""} placeholder="target's" onchange={(e) => setGifOptions({ fps: num(e) })} /></label>
      <label>Colours <input type="number" min="2" max="256" value={o.colours ?? ""} placeholder="target's" disabled={o.format === "mp4"} onchange={(e) => setGifOptions({ colours: num(e) })} /></label>
      <label>Dither
        <select value={ditherKey(o.dither)} disabled={o.format === "mp4"} onchange={(e) => setGifOptions({ dither: dithers.find((d) => d.value === e.currentTarget.value)?.dither ?? undefined })}>
          {#each dithers as d (d.value)}<option value={d.value}>{d.label}</option>{/each}
        </select>
      </label>
      <label>Lossy (gifsicle, 0–200) <input type="number" min="0" max="200" value={o.lossy ?? ""} placeholder="target's" disabled={o.format === "mp4"} onchange={(e) => setGifOptions({ lossy: num(e) })} /></label>
      <label>Plays
        <select value={o.loop === "forever" ? "forever" : String(o.loop)} onchange={(e) => setGifOptions({ loop: e.currentTarget.value === "forever" ? "forever" : Number(e.currentTarget.value) })}>
          <option value="forever">Forever</option><option value="1">Once</option><option value="2">Twice</option><option value="3">3 times</option>
        </select>
      </label>
      <label>Aim for (MB) <input type="number" min="0.07" step="0.1" value={o.budget?.soft != null ? o.budget.soft / 1e6 : ""} placeholder={o.budget === null ? "no limit" : "target's"} onchange={(e) => { const v = num(e); setGifOptions({ budget: v == null ? {} : { ...(o.budget ?? {}), soft: Math.round(v * 1e6) } }); }} /></label>
      <label>At most (MB) <input type="number" min="0.07" step="0.1" value={o.budget?.hard != null ? o.budget.hard / 1e6 : ""} placeholder="target's" disabled={o.budget === null} onchange={(e) => { const v = num(e); setGifOptions({ budget: { ...(o.budget ?? {}), hard: v == null ? undefined : Math.round(v * 1e6) } }); }} /></label>
      <label class="check"><input type="checkbox" checked={o.budget === null} onchange={(e) => setGifOptions({ budget: e.currentTarget.checked ? null : {} })} /> No size limit</label>
      <label class="check"><input type="checkbox" checked={o.allow_trim} onchange={(e) => setGifOptions({ allow_trim: e.currentTarget.checked })} /> Allow trimming to fit</label>
      <label>Engine
        <select value={o.engine} disabled={o.format === "mp4"} onchange={(e) => setGifOptions({ engine: e.currentTarget.value as typeof o.engine })}>
          <option value="auto">The target's</option><option value="builtin">Built-in</option><option value="gifski">gifski (best quality)</option>
        </select>
      </label>
      <label class="check"><input type="checkbox" checked={o.strict_engine} onchange={(e) => setGifOptions({ strict_engine: e.currentTarget.checked })} /> Fail if the engine is missing</label>
      <label class="check"><input type="checkbox" checked={o.optimise} onchange={(e) => setGifOptions({ optimise: e.currentTarget.checked })} /> Optimise with gifsicle</label>
    </div>
  </details>

  <div class="estimate" aria-live="polite">
    {#if gif.estimating}
      <span class="faint note">Estimating the size…</span>
    {:else if gif.estimateError}
      <span class="note bad" role="alert">{gif.estimateError}</span>
    {:else if est?.bytes != null}
      <span class="note"><strong>≈ {megabytes(est.bytes)}</strong>{est.lowered.length ? ` · lowers ${loweredText(est.lowered)}` : ""}</span>
      {#if est.mp4_bytes != null && o.format === "gif"}<span class="faint note">MP4 ≈ {megabytes(est.mp4_bytes)}</span>{/if}
    {/if}
  </div>

  {#if gif.result}
    <div class="result" aria-label="Result">
      <p class="note">
        {megabytes(gif.result.output.bytes)} · {gif.result.output.width}×{gif.result.output.height}{#if measure} · {measure.fps.toFixed(1)} fps{#if measure.search?.lowered.length} · lowered {loweredText(measure.search.lowered)}{/if}{/if}
      </p>
      {#if gif.result.output.kind === "image"}
        <img src={gif.result.output.url} alt="The exported GIF" />
      {:else}
        <video src={gif.result.output.url} autoplay loop muted playsinline></video>
      {/if}
    </div>
  {:else if gif.failure}
    <div class="result" role="alert">
      <p class="note bad">{gif.failure.message}</p>
      {#if gif.failure.fitsSecs != null && gif.last}
        <button class="ghost small" onclick={() => gif.last && exportNow(gif.last.path, gif.last.range, true)}>Trim to {gif.failure.fitsSecs.toFixed(1)} s</button>
      {/if}
    </div>
  {/if}

  <footer>
    <span class="faint note" class:bad={!validRange} title="Mark In and Out in the player to export part of the video">{rangeText}</span>
    <span class="spacer"></span>
    <button class="ghost" disabled={!validRange} onclick={addToQueue}>Add to queue</button>
    <button class="primary" disabled={!!blockedWhy} title={blockedWhy} aria-busy={runner.busy} onclick={() => exportNow(path, $state.snapshot(range))}>Export now</button>
  </footer>
</section>

<style>
  .controls { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-3) var(--space-4); align-items: start; }
  .field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  select, input[type="number"] { width: 100%; min-width: 0; }
  .note { margin: 0; font-size: 12px; overflow: hidden; text-overflow: ellipsis; }
  .note.bad { color: var(--danger); white-space: normal; }
  .seg { display: flex; padding: 2px; gap: 2px; border-radius: var(--radius-m); background: var(--surface-2); border: 1px solid var(--border); }
  .seg button { flex: 1; min-width: 0; height: 28px; padding: 0 6px; font-size: 12.5px; border: 0; background: transparent; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .seg button.on { background: var(--surface); color: var(--text); box-shadow: var(--shadow); font-weight: 600; }
  .drawer, .estimate, .result, footer { grid-column: 1 / -1; }
  .drawer summary { cursor: pointer; font-size: 12.5px; color: var(--text-2); }
  .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--space-2) var(--space-3); margin-top: var(--space-2); }
  .grid label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-2); min-width: 0; }
  .grid label.check { flex-direction: row; align-items: center; gap: 6px; }
  .estimate { display: flex; gap: var(--space-3); min-height: 18px; }
  .result { display: flex; flex-direction: column; gap: 6px; }
  .result img, .result video { max-height: 120px; max-width: 100%; align-self: start; border-radius: var(--radius-m); }
  footer { display: flex; align-items: center; gap: var(--space-2); }
  .spacer { flex: 1; }
</style>
