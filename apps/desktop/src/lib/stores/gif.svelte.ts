// GIF view state (Svelte 5 runes): targets, the chosen options (saved as Settings ›
// gif_defaults), the live size estimate (debounced, one in flight), and the result card of the
// last export. Pure logic lives in gif.ts.
import { api, errorText, type GifEstimateResult, type GifTargetList, type OutputInfo, type RangeArg } from "../api";
import type { GifOptions } from "../bindings/GifOptions";
import type { JobResult } from "../bindings/JobResult";
import { app } from "./app.svelte";
import { addGif, fullGif, gifClip, resolveGif, withTrim } from "./gif";
import { canRun, runManifest } from "./runner.svelte";
import { onlyJob } from "./upscale";

export interface GifResult {
  output: OutputInfo;
  /** The run's result for the job (its `measure: "gif"` block: size, lowered settings…). */
  job: JobResult;
}

export const gif = $state({
  targets: null as GifTargetList | null,
  options: resolveGif({}) as GifOptions,
  ready: false,
  estimate: null as GifEstimateResult | null,
  estimating: false,
  progress: 0,
  estimateError: "",
  result: null as GifResult | null,
  /** The last export failed: its message, and the length that fits when the budget could not. */
  failure: null as { message: string; fitsSecs: number | null } | null,
  /** What the last export was, for "Trim to N s". */
  last: null as { path: string; range: RangeArg } | null,
});

let loading = false;

/** Targets and the saved choices, once per app start (retried until settings are known). */
export async function initGif(): Promise<void> {
  if (gif.ready || loading || !app.settings) return;
  loading = true;
  try {
    gif.options = resolveGif(app.settings.gif_defaults ?? {});
    gif.targets = await api.listGifTargets();
    gif.ready = true;
  } catch (e) {
    gif.estimateError = errorText(e);
  } finally {
    loading = false;
  }
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
/** Changes the options and remembers them for next time (Settings › gif_defaults). */
export function setGifOptions(patch: Partial<GifOptions>): void {
  gif.options = { ...gif.options, ...patch };
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    if (!app.settings) return;
    const settings = { ...$state.snapshot(app.settings), gif_defaults: fullGif($state.snapshot(gif.options)) };
    try {
      await api.setSettings(settings);
      app.settings = settings;
    } catch {
      // Remembering the choice is a convenience; the view keeps working.
    }
  }, 600);
}

let estimateKey = "";
let estimateId: string | null = null;
let estimateTimer: ReturnType<typeof setTimeout> | undefined;

/** Keeps the estimate in step with the source, range and options (400 ms debounce). */
export function syncEstimate(path: string, range: RangeArg): void {
  if (!gif.ready) return;
  const key = JSON.stringify([path, range, gif.options]);
  if (key === estimateKey) return;
  estimateKey = key;
  clearTimeout(estimateTimer);
  estimateTimer = setTimeout(() => void estimate(path, range, key), 400);
}

/** Stops the estimate (leaving the view, or a run starting). */
export function stopEstimate(): void {
  clearTimeout(estimateTimer);
  if (estimateId) void api.cancelEstimate(estimateId).catch(() => {});
  estimateId = null;
  estimateKey = "";
  gif.estimating = false;
}

async function estimate(path: string, range: RangeArg, key: string): Promise<void> {
  if (estimateId) void api.cancelEstimate(estimateId).catch(() => {});
  const id = `g${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  estimateId = id;
  gif.estimating = true;
  gif.progress = 0;
  gif.estimateError = "";
  try {
    const r = await api.gifEstimate(id, path, range, $state.snapshot(gif.options), (e) => {
      if (estimateId === id && e.type === "progress") gif.progress = e.fraction;
    });
    if (estimateKey !== key) return;
    gif.estimate = r;
  } catch (e) {
    if (estimateKey === key) {
      gif.estimate = null;
      gif.estimateError = errorText(e);
    }
  } finally {
    if (estimateId === id) {
      gif.estimating = false;
      estimateId = null;
    }
  }
}

/** Adds this GIF to the queue. */
export function addGifToQueue(path: string, range: RangeArg): void {
  app.queue = addGif(app.queue, path, gifClip($state.snapshot(gif.options), range)).queue;
}

/** "Export now": queues the GIF and runs only it, then shows the result. `trim` re-queues it
 * allowed to trim to fit (the budget could not be reached). */
export async function exportNow(path: string, range: RangeArg, trim = false): Promise<void> {
  if (!canRun()) return;
  stopEstimate();
  gif.result = null;
  gif.failure = null;
  gif.last = { path, range };
  let clip = gifClip($state.snapshot(gif.options), range);
  if (trim) clip = withTrim(clip);
  const { queue, index } = addGif(app.queue, path, clip);
  app.queue = queue;
  if (!(await runManifest(onlyJob(queue, path, index)))) return;
  const run = app.run;
  const job = run?.summary?.jobs[0];
  if (!run || !job) return;
  if (job.status === "succeeded") {
    try {
      const output = await api.openOutput(run.runId, job.id);
      gif.result = { output, job };
    } catch (e) {
      gif.failure = { message: errorText(e), fitsSecs: null };
    }
  } else if (job.status === "failed") {
    gif.failure = { message: job.error.message, fitsSecs: job.error.detail?.fits_secs ?? null };
  } else if (job.status === "skipped") {
    gif.failure = { message: `Not exported: ${job.reason}`, fitsSecs: null };
  } else {
    gif.failure = { message: "Export cancelled", fitsSecs: null };
  }
}
