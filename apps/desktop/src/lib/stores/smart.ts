// Smart cut in the desktop app (feature 004): the timeline's keyframe ticks and head/copy/tail
// bands, the line under the timeline, the mode a source will be cut in, and the run panel's
// label. The plan itself comes from the core (`smart_plan`); this only lays it out.
import type { JobOutcome } from "../bindings/JobOutcome";
import type { KeyframeIndex } from "../bindings/KeyframeIndex";
import type { Mode } from "../bindings/Mode";
import type { Notice } from "../bindings/Notice";
import type { PartialOptions } from "../bindings/PartialOptions";
import type { SmartPart } from "../bindings/SmartPart";
import type { SmartSummary } from "../bindings/SmartSummary";
import type { Manifest } from "../manifest";

/** Percent positions of the keyframes inside a timeline `duration` seconds long. */
export function tickPositions(index: KeyframeIndex | null, duration: number): number[] {
  if (!index || !(duration > 0)) return [];
  return index.keyframes.filter((k) => k.at >= 0 && k.at <= duration).map((k) => (k.at / duration) * 100);
}

export interface Band {
  kind: "head" | "copy" | "tail";
  /** Percent of the timeline. */
  left: number;
  width: number;
  title: string;
}

const seconds = (p: SmartPart) => p.span.end - p.span.start;

/** The parts of a smart range on the timeline (none on a fallback). */
export function bands(s: SmartSummary | null, duration: number): Band[] {
  if (!s?.copy || !(duration > 0)) return [];
  const band = (kind: Band["kind"], p: SmartPart): Band => ({
    kind,
    left: (p.span.start / duration) * 100,
    width: (seconds(p) / duration) * 100,
    title: `${kind === "copy" ? "Copied unchanged" : "Re-encoded"}: ${seconds(p).toFixed(2)} s (${p.frames} frames)`,
  });
  return [s.head && band("head", s.head), band("copy", s.copy), s.tail && band("tail", s.tail)].filter(
    (b): b is Band => !!b,
  );
}

const capital = (t: string) => t.charAt(0).toUpperCase() + t.slice(1);

/** The line under the timeline: the copied share, or why smart cut does not apply here. */
export function smartLine(s: SmartSummary | null, notices: Notice[]): string {
  if (s?.copy) {
    const parts = [`Smart: ${Math.round((s.copied_share ?? 0) * 100)}% copied`];
    if (s.head) parts.push(`${seconds(s.head).toFixed(2)} s re-encoded at the start`);
    if (s.tail) parts.push(`${seconds(s.tail).toFixed(2)} s${s.head ? "" : " re-encoded"} at the end`);
    return parts.join(" · ");
  }
  const why = notices.find((n) => n.code === "SMART_FALLBACK" || n.code === "SMART_NOT_APPLIED");
  return why ? capital(why.message) : "Smart cut not possible here; the clip is cut in accurate mode";
}

/** The video's options over the queue's (what `smart_plan` should plan with). An option the
 *  video reset to the default is stored as `undefined`; it must not hide the queue's value. */
export function planOptions(queue: Manifest, path: string): PartialOptions {
  const video = queue.videos.find((v) => v.input === path);
  const set = Object.entries(video?.options ?? {}).filter(([, v]) => v !== undefined);
  return { ...queue.defaults, ...Object.fromEntries(set) };
}

/** The mode a source's new clips are cut in: the video's, the queue's, the settings', else fast. */
export function effectiveMode(queue: Manifest, settings: PartialOptions | undefined, path: string): Mode {
  return planOptions(queue, path).mode ?? settings?.mode ?? "fast";
}

/** The run panel's word on a clip cut in smart mode ("" for other jobs). */
export function smartOutcomeLabel(o: JobOutcome): string {
  if (o.status !== "succeeded" || o.measure !== "clip" || !o.smart) return "";
  const s = o.smart;
  return s.fallback ? `Accurate (smart fallback: ${s.fallback})` : `Smart · ${Math.round(s.copied_share * 100)}% copied`;
}
