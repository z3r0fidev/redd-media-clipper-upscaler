// Pure queue operations (unit-tested). The UI keeps a Manifest in $state and replaces it with
// the results of these functions.
import type { PartialOptions } from "../bindings/PartialOptions";
import type { ClipEntry, Manifest, VideoEntry } from "../manifest";

export function emptyQueue(defaults: PartialOptions = {}): Manifest {
  return { version: 1, defaults, videos: [] };
}

export function addSource(q: Manifest, input: string): Manifest {
  if (q.videos.some((v) => v.input === input)) return q;
  return { ...q, videos: [...q.videos, { input, clips: [] }] };
}

export function removeSource(q: Manifest, input: string): Manifest {
  return { ...q, videos: q.videos.filter((v) => v.input !== input) };
}

function mapVideo(q: Manifest, input: string, f: (v: VideoEntry) => VideoEntry): Manifest {
  return { ...q, videos: q.videos.map((v) => (v.input === input ? f(v) : v)) };
}

export function addClip(q: Manifest, input: string, clip: ClipEntry): Manifest {
  return mapVideo(addSource(q, input), input, (v) => ({ ...v, clips: [...v.clips, clip] }));
}

export function updateClip(q: Manifest, input: string, index: number, patch: Partial<ClipEntry>): Manifest {
  return mapVideo(q, input, (v) => ({
    ...v,
    clips: v.clips.map((c, i) => {
      if (i !== index) return c;
      const next: ClipEntry = { ...c, ...patch };
      // end and duration are mutually exclusive (schema): setting one clears the other.
      if ("end" in patch && patch.end !== undefined) delete next.duration;
      if ("duration" in patch && patch.duration !== undefined) delete next.end;
      if (next.name === "") delete next.name;
      return next;
    }),
  }));
}

export function removeClip(q: Manifest, input: string, index: number): Manifest {
  return mapVideo(q, input, (v) => ({ ...v, clips: v.clips.filter((_, i) => i !== index) }));
}

export function setSourceOptions(q: Manifest, input: string, options: PartialOptions): Manifest {
  return mapVideo(q, input, (v) => {
    const cleaned = Object.fromEntries(Object.entries(options).filter(([, val]) => val !== undefined)) as PartialOptions;
    const next: VideoEntry = { ...v, options: cleaned };
    if (Object.keys(cleaned).length === 0) delete next.options;
    return next;
  });
}

/** Merges an imported manifest (clip list / manifest file) into the queue. */
export function merge(q: Manifest, incoming: Manifest): Manifest {
  let out = q;
  for (const v of incoming.videos) {
    out = addSource(out, v.input);
    out = mapVideo(out, v.input, (cur) => ({
      ...cur,
      options: v.options ?? cur.options,
      clips: [...cur.clips, ...v.clips],
    }));
  }
  return out;
}

export function clipCount(q: Manifest): number {
  return q.videos.reduce((n, v) => n + v.clips.length, 0);
}

/** Only sources with clips are sent for planning. */
export function runnable(q: Manifest): Manifest {
  return { ...q, videos: q.videos.filter((v) => v.clips.length > 0) };
}
