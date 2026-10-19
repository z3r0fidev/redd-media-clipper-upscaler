import { describe, expect, it } from "vitest";
import type { ReframeAnalysis } from "../bindings/ReframeAnalysis";
import type { UpscaleOptions } from "../bindings/UpscaleOptions";
import { addClip, emptyQueue } from "./queue";
import {
  addUpscale,
  contentBox,
  cropRect,
  cropXAt,
  fallbackSegments,
  fullUpscale,
  isUpscale,
  onlyJob,
  resolveOptions,
  upscaleClip,
} from "./upscale";

function analysis(over: Partial<ReframeAnalysis> = {}): ReframeAnalysis {
  return {
    key: "k",
    algo_version: 1,
    source_size: [1280, 720],
    crop: [332, 720],
    crop_y: 0,
    fps: 10,
    clip_start: 2,
    shots: [],
    fallback_ranges: [],
    path: Array.from({ length: 30 }, (_, k) => k * 10),
    detections: 0,
    samples: 0,
    ...over,
  };
}

const opts: UpscaleOptions = { preset: "phone-fill", reframe: "smart", tier: "auto", hdr: "sdr", motion: "normal" };

describe("crop overlay", () => {
  it("interpolates the path between frames, in clip time", () => {
    const a = analysis();
    expect(cropXAt(a, 2)).toEqual({ x: 0 });
    expect(cropXAt(a, 3)).toEqual({ x: 100 });
    expect(cropXAt(a, 3.05)).toEqual({ x: 105 });
    expect(cropXAt(a, 4.95)).toEqual({ x: 290 }); // held at the last frame
    expect(cropXAt(a, 1.9)).toBeNull();
    expect(cropXAt(a, 5.01)).toBeNull();
  });

  it("reports fallback ranges instead of a crop", () => {
    const a = analysis({ fallback_ranges: [{ start: 3, end: 3.5 }] });
    expect(cropXAt(a, 3.2)).toEqual({ fallback: true });
    expect(cropXAt(a, 3.5)).toEqual({ x: 150 });
  });

  it("fits the video's content box like object-fit: contain", () => {
    expect(contentBox({ width: 800, height: 600 }, [1280, 720])).toEqual({ left: 0, top: 75, width: 800, height: 450 });
    const wide = contentBox({ width: 800, height: 300 }, [1280, 720]);
    expect([wide.top, wide.height]).toEqual([0, 300]);
    expect(wide.left).toBeCloseTo(133.333, 3);
    expect(wide.width).toBeCloseTo(533.333, 3);
  });

  it("scales the crop rectangle onto the content box", () => {
    const r = cropRect(analysis(), 3, { width: 640, height: 460 });
    // Box: 640x360 at top 50; scale 0.5.
    expect(r).toEqual({ left: 50, top: 50, width: 166, height: 360 });
    expect(cropRect(analysis({ fallback_ranges: [{ start: 2, end: 5 }] }), 3, { width: 640, height: 460 })).toBeNull();
  });

  it("places fallback ranges on the timeline in percent", () => {
    const a = analysis({ fallback_ranges: [{ start: 2, end: 3 }, { start: 8, end: 12 }] });
    expect(fallbackSegments(a, 10)).toEqual([
      { left: 20, width: 10 },
      { left: 80, width: 20 },
    ]);
    expect(fallbackSegments(a, 0)).toEqual([]);
  });
});

describe("upscale queue entries", () => {
  it("writes every field so exports do not depend on settings", () => {
    expect(fullUpscale(opts)).toEqual({ preset: "phone-fill", reframe: "smart", tier: "auto", hdr: "sdr", motion: "normal" });
    expect(fullUpscale({ ...opts, crf: 20 }).crf).toBe(20);
  });

  it("makes a whole-video entry or a range", () => {
    expect(upscaleClip(opts, { start: 0, end: null })).toEqual({ start: 0, end: "end", options: { upscale: fullUpscale(opts) } });
    expect(upscaleClip(opts, { start: 1.23456, end: 4 })).toMatchObject({ start: 1.235, end: 4 });
  });

  it("replaces the same range instead of queueing it twice, and leaves plain clips alone", () => {
    let q = addClip(emptyQueue(), "a", { start: 0, end: "end" });
    let r = addUpscale(q, "a", upscaleClip(opts, { start: 0, end: null }));
    expect(r.index).toBe(1);
    q = r.queue;
    r = addUpscale(q, "a", upscaleClip({ ...opts, preset: "reddit-916" }, { start: 0, end: null }));
    expect(r.index).toBe(1);
    expect(r.queue.videos[0]!.clips).toHaveLength(2);
    expect(r.queue.videos[0]!.clips[1]!.options?.upscale?.preset).toBe("reddit-916");
    expect(isUpscale(r.queue.videos[0]!.clips[0]!)).toBe(false);
    expect(isUpscale(r.queue.videos[0]!.clips[1]!)).toBe(true);
  });

  it("runs one job on its own, keeping the queue defaults and video options", () => {
    let q = { ...emptyQueue({ output_dir: "/out" }) };
    q = addClip(q, "a", { start: 1, end: 2 });
    q = { ...q, videos: q.videos.map((v) => ({ ...v, options: { audio: "none" as const } })) };
    const { queue, index } = addUpscale(q, "a", upscaleClip(opts, { start: 0, end: null }));
    expect(onlyJob(queue, "a", index)).toEqual({
      version: 1,
      defaults: { output_dir: "/out" },
      videos: [{ input: "a", options: { audio: "none" }, clips: [upscaleClip(opts, { start: 0, end: null })] }],
    });
  });

  it("resolves saved defaults over the built-in ones", () => {
    expect(resolveOptions({})).toEqual(opts);
    expect(resolveOptions({ preset: "duo", tier: "cpu", crf: null })).toEqual({ ...opts, preset: "duo", tier: "cpu" });
    expect(resolveOptions({ crf: 18 }).crf).toBe(18);
  });
});
