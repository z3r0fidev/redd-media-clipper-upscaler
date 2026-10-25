// Pure helpers for the GIF view (unit-tested): its options from the saved defaults, queue
// entries carrying a `gif` object, and the manifest shape the export writes.
import type { GifOptions } from "../bindings/GifOptions";
import type { PartialGif } from "../bindings/PartialGif";
import type { ClipEntry, Manifest } from "../manifest";

/** The built-in defaults (`rmedia gif` without flags). */
export const DEFAULT_GIF: GifOptions = {
  target: "reddit-post",
  format: "gif",
  engine: "auto",
  strict_engine: false,
  optimise: true,
  loop: "forever",
  shape: "source",
  framing: "smart",
  budget: {},
  allow_trim: false,
};

/** The view's options: saved defaults (Settings › `gif_defaults`) over the built-ins. */
export function resolveGif(saved: PartialGif): GifOptions {
  const o: GifOptions = { ...DEFAULT_GIF };
  for (const key of ["target", "format", "engine", "strict_engine", "optimise", "loop", "shape", "framing", "allow_trim"] as const) {
    if (saved[key] !== undefined) (o as Record<string, unknown>)[key] = saved[key];
  }
  for (const key of ["width", "fps", "colours", "dither", "lossy"] as const) {
    const v = saved[key];
    if (v != null) (o as Record<string, unknown>)[key] = v;
  }
  if (saved.budget !== undefined) o.budget = saved.budget;
  return o;
}

/** Every field written out, so an exported manifest does not depend on anyone's settings. */
export function fullGif(o: GifOptions): PartialGif {
  const p: PartialGif = {
    target: o.target,
    format: o.format,
    engine: o.engine,
    strict_engine: o.strict_engine,
    optimise: o.optimise,
    loop: o.loop,
    shape: o.shape,
    framing: o.framing,
    budget: o.budget,
    allow_trim: o.allow_trim,
  };
  for (const key of ["width", "fps", "colours", "dither", "lossy"] as const) {
    if (o[key] != null) (p as Record<string, unknown>)[key] = o[key];
  }
  return p;
}

const round = (x: number) => Math.round(x * 1000) / 1000;

/** A queue clip making a GIF (or Reddit MP4) of `range` (end `null` = to the end). */
export function gifClip(o: GifOptions, range: { start: number; end: number | null }): ClipEntry {
  return { start: round(range.start), end: range.end == null ? "end" : round(range.end), options: { gif: fullGif(o) } };
}

export function isGif(c: ClipEntry): boolean {
  const g = c.options?.gif;
  return g != null && g.enabled !== false;
}

/** Adds a GIF clip, replacing a GIF clip of the same range (the options changed). */
export function addGif(q: Manifest, input: string, clip: ClipEntry): { queue: Manifest; index: number } {
  const video = q.videos.find((v) => v.input === input);
  const clips = video?.clips ?? [];
  const same = clips.findIndex((c) => isGif(c) && c.start === clip.start && c.end === clip.end && c.duration === undefined);
  const index = same >= 0 ? same : clips.length;
  const next = same >= 0 ? clips.map((c, i) => (i === same ? clip : c)) : [...clips, clip];
  const videos = video
    ? q.videos.map((v) => (v.input === input ? { ...v, clips: next } : v))
    : [...q.videos, { input, clips: next }];
  return { queue: { ...q, videos }, index };
}

/** The same clip, allowed to trim to fit its budget ("Trim to N s", US2 scenario 2). */
export function withTrim(c: ClipEntry): ClipEntry {
  return { ...c, options: { ...c.options, gif: { ...c.options?.gif, allow_trim: true } } };
}

/** "1.2 MB" (decimal units, as the CLI prints). */
export function megabytes(bytes: number): string {
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} kB`;
}

/** The limits a budget gives over the target's, as the core resolves them (`BudgetSpec::resolve`):
 *  `soft` alone sets `hard = soft`; `hard` alone keeps the target's soft, capped at `hard`. */
export function resolveBudget(
  b: { soft?: number; hard?: number },
  target: { soft: number; hard: number } | null,
): { soft: number; hard: number } | null {
  if (b.soft != null) return { soft: b.soft, hard: b.hard ?? b.soft };
  if (b.hard != null) return { soft: target ? Math.min(target.soft, b.hard) : b.hard, hard: b.hard };
  return target;
}
