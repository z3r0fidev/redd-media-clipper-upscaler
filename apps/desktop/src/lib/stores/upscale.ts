// Pure helpers for the upscale view (unit-tested): the crop overlay from a reframe analysis,
// timeline marks for its fallback ranges, and queue entries carrying an `upscale` object.
import type { PartialUpscale } from "../bindings/PartialUpscale";
import type { ReframeAnalysis } from "../bindings/ReframeAnalysis";
import type { UpscaleOptions } from "../bindings/UpscaleOptions";
import type { ClipEntry, Manifest } from "../manifest";

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The built-in defaults (`rmedia upscale` without flags). */
export const DEFAULT_UPSCALE: UpscaleOptions = { preset: "phone-fill", reframe: "smart", tier: "auto", hdr: "sdr", motion: "normal" };

/** The view's options: saved defaults (Settings › `upscale_defaults`) over the built-ins. */
export function resolveOptions(saved: PartialUpscale): UpscaleOptions {
  const o: UpscaleOptions = {
    preset: saved.preset ?? DEFAULT_UPSCALE.preset,
    reframe: saved.reframe ?? DEFAULT_UPSCALE.reframe,
    tier: saved.tier ?? DEFAULT_UPSCALE.tier,
    hdr: saved.hdr ?? DEFAULT_UPSCALE.hdr,
    motion: saved.motion ?? DEFAULT_UPSCALE.motion,
  };
  if (typeof saved.crf === "number") o.crf = saved.crf;
  return o;
}

/** Every field written out, so an exported manifest does not depend on anyone's settings. */
export function fullUpscale(o: UpscaleOptions): PartialUpscale {
  const p: PartialUpscale = { preset: o.preset, reframe: o.reframe, tier: o.tier, hdr: o.hdr, motion: o.motion };
  if (o.crf != null) p.crf = o.crf;
  return p;
}

const round = (x: number) => Math.round(x * 1000) / 1000;

/** A queue clip upscaling `range` (end `null` = to the end of the video). */
export function upscaleClip(o: UpscaleOptions, range: { start: number; end: number | null }): ClipEntry {
  return { start: round(range.start), end: range.end == null ? "end" : round(range.end), options: { upscale: fullUpscale(o) } };
}

export function isUpscale(c: ClipEntry): boolean {
  const u = c.options?.upscale;
  return u != null && u.enabled !== false;
}

/** Adds an upscale clip, replacing an upscale clip of the same range (the options changed). */
export function addUpscale(q: Manifest, input: string, clip: ClipEntry): { queue: Manifest; index: number } {
  const video = q.videos.find((v) => v.input === input);
  const clips = video?.clips ?? [];
  const same = clips.findIndex((c) => isUpscale(c) && c.start === clip.start && c.end === clip.end && c.duration === undefined);
  const index = same >= 0 ? same : clips.length;
  const next = same >= 0 ? clips.map((c, i) => (i === same ? clip : c)) : [...clips, clip];
  const videos = video
    ? q.videos.map((v) => (v.input === input ? { ...v, clips: next } : v))
    : [...q.videos, { input, clips: next }];
  return { queue: { ...q, videos }, index };
}

/** The queue reduced to one clip ("Upscale now" runs only this job). */
export function onlyJob(q: Manifest, input: string, index: number): Manifest {
  const v = q.videos.find((x) => x.input === input);
  const clip = v?.clips[index];
  if (!v || !clip) return { ...q, videos: [] };
  const entry = v.options ? { input, options: v.options, clips: [clip] } : { input, clips: [clip] };
  return { ...q, videos: [entry] };
}

const clipLen = (a: ReframeAnalysis) => a.path.length / a.fps;

/** The crop x at source time `t` (interpolated between frames), `fallback` inside a blurred
 * range, or `null` outside the analysed clip. */
export function cropXAt(a: ReframeAnalysis, t: number): { x: number } | { fallback: true } | null {
  if (!a.path.length || t < a.clip_start - 1e-6 || t > a.clip_start + clipLen(a) + 1e-6) return null;
  if (a.fallback_ranges.some((r) => r.start <= t && t < r.end)) return { fallback: true };
  const f = Math.max(0, (t - a.clip_start) * a.fps);
  const k0 = Math.min(Math.floor(f + 1e-9), a.path.length - 1);
  const k1 = Math.min(k0 + 1, a.path.length - 1);
  const x0 = a.path[k0]!;
  const x1 = a.path[k1]!;
  return { x: Math.round((x0 + (x1 - x0) * Math.min(1, f - k0)) * 1000) / 1000 };
}

/** Where a `media`-sized picture sits in a `box` with `object-fit: contain`. */
export function contentBox(box: { width: number; height: number }, media: [number, number]): Rect {
  const scale = Math.min(box.width / media[0], box.height / media[1]);
  const width = media[0] * scale;
  const height = media[1] * scale;
  return { left: (box.width - width) / 2, top: (box.height - height) / 2, width, height };
}

/** The crop rectangle at `t` in the stage's pixels, or `null` (none, or the blurred background). */
export function cropRect(a: ReframeAnalysis, t: number, box: { width: number; height: number }): Rect | null {
  const at = cropXAt(a, t);
  if (!at || "fallback" in at) return null;
  const c = contentBox(box, a.source_size);
  const s = c.width / a.source_size[0];
  return { left: c.left + at.x * s, top: c.top + a.crop_y * s, width: a.crop[0] * s, height: a.crop[1] * s };
}

/** Fallback ranges as timeline segments (percent of `duration`). */
export function fallbackSegments(a: ReframeAnalysis, duration: number): { left: number; width: number }[] {
  if (!(duration > 0)) return [];
  return a.fallback_ranges.map((r) => {
    const start = Math.max(0, Math.min(duration, r.start));
    const end = Math.max(start, Math.min(duration, r.end));
    return { left: (start / duration) * 100, width: ((end - start) / duration) * 100 };
  });
}
