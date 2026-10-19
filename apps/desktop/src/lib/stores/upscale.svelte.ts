// Upscale view state (Svelte 5 runes): presets, the chosen options (saved as Settings ›
// upscale_defaults), the smart-reframe analysis behind the crop overlay, and the before/after
// still. Pure logic lives in upscale.ts.
import { api, errorText, type PresetList, type RangeArg, type UpscalePreview } from "../api";
import type { ReframeAnalysis } from "../bindings/ReframeAnalysis";
import type { UpscaleOptions } from "../bindings/UpscaleOptions";
import { app } from "./app.svelte";
import { fullUpscale, resolveOptions } from "./upscale";

export const upscale = $state({
  presets: null as PresetList | null,
  options: resolveOptions({}) as UpscaleOptions,
  /** The analysis of the current source/range/options, once finished. */
  analysis: null as ReframeAnalysis | null,
  analyzing: false,
  progress: 0,
  analysisError: "",
  preview: null as UpscalePreview | null,
  previewBusy: false,
  previewError: "",
});

let loaded = false;

/** Presets and the saved choices, once per app start. */
export async function initUpscale(): Promise<void> {
  if (loaded) return;
  loaded = true;
  if (app.settings) upscale.options = resolveOptions(app.settings.upscale_defaults);
  try {
    upscale.presets = await api.listPresets();
  } catch (e) {
    loaded = false;
    upscale.previewError = errorText(e);
  }
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
/** Changes the options and remembers them for next time (Settings › upscale_defaults). */
export function setOptions(patch: Partial<UpscaleOptions>): void {
  upscale.options = { ...upscale.options, ...patch };
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    if (!app.settings) return;
    const settings = { ...$state.snapshot(app.settings), upscale_defaults: fullUpscale($state.snapshot(upscale.options)) };
    try {
      await api.setSettings(settings);
      app.settings = settings;
    } catch {
      // Remembering the choice is a convenience; the view keeps working.
    }
  }, 600);
}

export interface ViewInput {
  path: string;
  range: RangeArg;
  /** Playhead (source seconds). */
  at: number;
  playing: boolean;
  duration: number;
}

let analysisKey = "";
let analysisId: string | null = null;
let previewTimer: ReturnType<typeof setTimeout> | undefined;
let previewSeq = 0;
let lastPreviewKey = "";

/** Keeps the analysis and the still in step with the player (called from an effect). */
export function syncView(v: ViewInput): void {
  const o = upscale.options;
  // The analysis depends on what is framed, not on the tier or quality.
  const aKey = JSON.stringify([v.path, v.range, o.preset, o.reframe, o.motion, o.hdr]);
  if (aKey !== analysisKey) {
    analysisKey = aKey;
    void analyze(v, aKey);
  }
  if (v.playing) return;
  const last = Math.max(v.range.start, (v.range.end ?? v.duration) - 0.05);
  const at = Math.min(Math.max(v.at, v.range.start), last);
  const pKey = JSON.stringify([aKey, o.tier, o.crf ?? null, Math.round(at * 1000), upscale.analysis?.key ?? null]);
  if (pKey === lastPreviewKey) return;
  lastPreviewKey = pKey;
  clearTimeout(previewTimer);
  previewTimer = setTimeout(() => void preview(v.path, v.range, at), 250);
}

/** Stops the analysis (leaving the view or the window). */
export function stopView(): void {
  if (analysisId) void api.cancelAnalysis(analysisId).catch(() => {});
  analysisId = null;
  analysisKey = "";
  lastPreviewKey = "";
  clearTimeout(previewTimer);
}

async function analyze(v: ViewInput, key: string): Promise<void> {
  if (analysisId) void api.cancelAnalysis(analysisId).catch(() => {});
  upscale.analysis = null;
  upscale.analysisError = "";
  upscale.progress = 0;
  if (upscale.options.reframe !== "smart") {
    upscale.analyzing = false;
    analysisId = null;
    return;
  }
  const id = `a${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  analysisId = id;
  upscale.analyzing = true;
  try {
    const a = await api.analyzeReframe(id, v.path, v.range, $state.snapshot(upscale.options), (e) => {
      if (analysisId === id && e.type === "progress") upscale.progress = e.fraction;
    });
    if (analysisKey !== key) return;
    upscale.analysis = a;
    // The still can now follow the path instead of the centred guess.
    lastPreviewKey = "";
  } catch (e) {
    if (analysisKey === key) upscale.analysisError = errorText(e);
  } finally {
    if (analysisId === id) {
      upscale.analyzing = false;
      analysisId = null;
    }
  }
}

async function preview(path: string, range: RangeArg, at: number): Promise<void> {
  const seq = ++previewSeq;
  upscale.previewBusy = true;
  try {
    const p = await api.upscalePreview(path, range, at, $state.snapshot(upscale.options));
    if (seq !== previewSeq) return;
    upscale.preview = p;
    upscale.previewError = "";
  } catch (e) {
    if (seq === previewSeq) upscale.previewError = errorText(e);
  } finally {
    if (seq === previewSeq) upscale.previewBusy = false;
  }
}
