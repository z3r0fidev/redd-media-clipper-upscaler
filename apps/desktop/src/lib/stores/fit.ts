// Budgeted video in the desktop app (feature 005): the clip panel's target picker, its
// prediction line, refusals, queue labels and the run's result. The plan comes from the core
// (`fit_plan`, `plan`); this only builds `PartialFit` and lays the answers out.
import type { FitSummary } from "../bindings/FitSummary";
import type { Issue } from "../bindings/Issue";
import type { JobOutcome } from "../bindings/JobOutcome";
import type { Notice } from "../bindings/Notice";
import type { PartialFit } from "../bindings/PartialFit";
import type { VideoTarget } from "../bindings/VideoTarget";
import type { ClipEntry } from "../manifest";

/** "200 kB", "8.0 MB", "1.07 GB": decimal units, as the CLI prints them (`human_bytes`). */
export function bytes(b: number): string {
  if (b >= 1e9) return `${(b / 1e9).toFixed(2)} GB`;
  if (b >= 1e6) return `${(b / 1e6).toFixed(1)} MB`;
  return `${Math.round(b / 1e3)} kB`;
}

/** The core's refusal text names CLI flags; the app names its own controls. */
export function forApp(message: string): string {
  return message
    .replace(/use --fit-allow-trim to keep/g, "allow trimming to keep")
    .replace(/raise --fit-max-size or use --fit-allow-trim/g, "raise the size limit or allow trimming")
    .replace(/--fit-allow-trim/g, "Allow trimming")
    .replace(/--fit-max-size/g, "the size limit");
}

/** The picker's value: no fit, a target, or a target with the user's own stricter limits. */
export interface FitChoice {
  /** `null`: "None". */
  target: string | null;
  /** "Custom limits…": the two fields below are sent. */
  custom: boolean;
  /** As typed ("50MB", "8MiB", bytes); empty = the target's. */
  maxSize: string;
  /** As typed ("5:00", seconds); empty = the target's. */
  maxLength: string;
  allowTrim: boolean;
}

export const NO_FIT: FitChoice = { target: null, custom: false, maxSize: "", maxLength: "", allowTrim: false };

/** The clip's `fit` object: only the keys the user set, so an exported manifest stays short and
 *  follows the target's limits if they change. `null`: no fit. */
export function fitPartial(c: FitChoice): PartialFit | null {
  if (!c.target) return null;
  const p: PartialFit = { target: c.target };
  if (c.custom) {
    const size = c.maxSize.trim();
    const length = c.maxLength.trim();
    if (size) p.max_size = /^\d+$/.test(size) ? Number(size) : size;
    if (length) p.max_length = /^\d+(\.\d+)?$/.test(length) ? Number(length) : length;
  }
  if (c.allowTrim) p.allow_trim = true;
  return p;
}

/** The picker's value back from a clip's `fit` (a loaded manifest, or a queued clip). */
export function choiceOf(p: PartialFit | undefined | null): FitChoice {
  if (!p || p.enabled === false) return { ...NO_FIT };
  const size = p.max_size == null ? "" : String(p.max_size);
  const length = p.max_length == null ? "" : String(p.max_length);
  return {
    target: p.target ?? "reddit-video",
    custom: size !== "" || length !== "",
    maxSize: size,
    maxLength: length,
    allowTrim: p.allow_trim === true,
  };
}

/** The clip's options with the fit (or without, for `null`). A fit turns GIF export off for the
 *  clip (`gifOn`: the queue or video makes GIFs), since the core refuses both together. */
export function withFit(c: ClipEntry, fit: PartialFit | null, gifOn: boolean): ClipEntry {
  const options = { ...c.options };
  delete options.fit;
  if (fit) {
    options.fit = fit;
    if (gifOn) options.gif = { enabled: false };
  }
  const { options: _old, ...bare } = c;
  return Object.keys(options).length ? { ...bare, options } : bare;
}

/** "15:00", "1:02:03". */
export function clock(secs: number): string {
  const s = Math.round(secs);
  const [h, m, r] = [Math.floor(s / 3600), Math.floor((s % 3600) / 60), s % 60];
  const two = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${two(m)}:${two(r)}` : `${m}:${two(r)}`;
}

/** The prediction under the timeline, e.g.
 *  `Reddit video: 2.9 MB predicted of 3.0 MB · limit 15:00 · 1920x1080 30 fps · 2 passes`. */
export function fitLine(f: FitSummary, targets: VideoTarget[]): string {
  const t = targets.find((x) => x.id === f.target);
  const name = t?.name ?? f.target;
  const fps = Number.isInteger(f.fps) ? f.fps : f.fps.toFixed(2);
  const word = f.mode === "budget" ? "predicted" : "at most";
  const audio = f.audio_kbps == null ? " · no audio" : "";
  return `${name}: ${bytes(f.predicted_bytes)} ${word} of ${bytes(f.cap)} · limit ${clock(f.max_length)} · ${f.size[0]}x${f.size[1]} ${fps} fps${audio} · ${f.passes} pass${f.passes === 1 ? "" : "es"}`;
}

/** The notices the panel shows with the prediction (lowered settings, trims, mode changes). */
export function fitNotes(notices: Notice[]): string[] {
  const shown = new Set(["FIT_LOWERED", "FIT_TRIMMED", "SMART_NOT_APPLIED"]);
  return notices.filter((n) => shown.has(n.code)).map((n) => n.message);
}

export interface Refusal {
  message: string;
  /** Allowing trimming would fit the range (a too-long range, or an unreachable budget). */
  trimmable: boolean;
}

/** Why the clip cannot be queued as it is (`null`: it can). Any error refuses; only these two
 *  are solved by trimming. */
export function refusal(issues: Issue[]): Refusal | null {
  const e = issues.find((i) => i.severity === "error");
  if (!e) return null;
  return { message: forApp(e.message), trimmable: e.code === "RANGE_TOO_LONG" || e.code === "BUDGET_UNREACHABLE" };
}

/** The queue's label for a clip's fit: `Reddit video · 3.0 MB` (the cap when the user set one). */
export function fitLabel(p: PartialFit | undefined, targets: VideoTarget[]): string {
  if (!p || p.enabled === false) return "";
  const t = targets.find((x) => x.id === (p.target ?? "reddit-video"));
  const name = t?.name ?? p.target ?? "reddit-video";
  const cap = typeof p.max_size === "number" ? bytes(p.max_size) : (p.max_size ?? (t ? bytes(t.max_bytes) : ""));
  return [name, cap, p.allow_trim ? "may trim" : ""].filter(Boolean).join(" · ");
}

/** The run panel's result for a fitted job: `2.9 MB of 3.0 MB · trimmed`. */
export function fitOutcomeLabel(o: JobOutcome): string {
  if (o.status !== "succeeded" || (o.measure !== "clip" && o.measure !== "upscale") || !o.fit) return "";
  const f = o.fit;
  const retries = f.attempts.length > 1 ? ` · ${f.attempts.length} attempts` : "";
  return `${bytes(f.bytes)} of ${bytes(f.cap)}${f.trimmed ? " · trimmed" : ""}${retries}`;
}
