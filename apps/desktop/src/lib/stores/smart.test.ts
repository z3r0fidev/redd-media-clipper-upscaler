// Smart cut in the desktop app (feature 004, T049): the timeline's keyframe ticks and
// head/copy/tail bands, the line under the timeline, the mode a source will be cut in, and the
// run panel's label.
import { describe, expect, it } from "vitest";
import type { JobOutcome } from "../bindings/JobOutcome";
import type { KeyframeIndex } from "../bindings/KeyframeIndex";
import type { Notice } from "../bindings/Notice";
import type { SmartOutcome } from "../bindings/SmartOutcome";
import type { SmartSummary } from "../bindings/SmartSummary";
import { bands, effectiveMode, planOptions, smartLine, smartOutcomeLabel, tickPositions } from "./smart";

const index: KeyframeIndex = {
  window: null,
  keyframes: [0, 2, 4, 6, 8, 10].map((at) => ({ at, clean: true })),
  frame_duration: 0.04,
  truncated: false,
};

const eligible: SmartSummary = {
  requested: true,
  mode: "smart",
  strict: false,
  head: { span: { start: 3.32, end: 4 }, frames: 17 },
  copy: { span: { start: 4, end: 8 }, frames: 100 },
  tail: { span: { start: 8, end: 8.72 }, frames: 18 },
  copied_share: 0.7407,
  fallback: null,
};

const fallback: SmartSummary = {
  ...eligible,
  mode: "accurate",
  head: null,
  copy: null,
  tail: null,
  copied_share: null,
  fallback: "open_gop",
};

const fallbackNotice: Notice = {
  code: "SMART_FALLBACK",
  message: "smart cut not possible: the video has open GOPs (frames that depend on the previous GOP); cutting in accurate mode",
  detail: { reason: "open_gop" },
};

describe("timeline", () => {
  it("puts a tick at every keyframe inside the video", () => {
    expect(tickPositions(index, 8)).toEqual([0, 25, 50, 75, 100]);
    expect(tickPositions(null, 8)).toEqual([]);
    expect(tickPositions(index, 0)).toEqual([]);
  });

  it("draws the head, copy and tail of an eligible range", () => {
    const b = bands(eligible, 10);
    expect(b.map((x) => x.kind)).toEqual(["head", "copy", "tail"]);
    expect(b[0]!.left).toBeCloseTo(33.2);
    expect(b[0]!.width).toBeCloseTo(6.8);
    expect(b[1]).toMatchObject({ left: 40, width: 40 });
    expect(b[1]!.title).toBe("Copied unchanged: 4.00 s (100 frames)");
    expect(b[2]!.title).toBe("Re-encoded: 0.72 s (18 frames)");
    expect(bands({ ...eligible, head: null, tail: null }, 10).map((x) => x.kind)).toEqual(["copy"]);
    expect(bands(fallback, 10)).toEqual([]);
    expect(bands(null, 10)).toEqual([]);
  });

  it("says what will be copied, or why smart cut does not apply", () => {
    expect(smartLine(eligible, [])).toBe("Smart: 74% copied · 0.68 s re-encoded at the start · 0.72 s at the end");
    expect(smartLine({ ...eligible, head: null, tail: null, copied_share: 1 }, [])).toBe("Smart: 100% copied");
    expect(smartLine(fallback, [fallbackNotice])).toBe(
      "Smart cut not possible: the video has open GOPs (frames that depend on the previous GOP); cutting in accurate mode",
    );
    const notApplied: Notice = {
      code: "SMART_NOT_APPLIED",
      message: "smart cut does not apply: the clip is re-encoded for upscaling/GIF",
    };
    expect(smartLine(null, [notApplied])).toBe("Smart cut does not apply: the clip is re-encoded for upscaling/GIF");
    expect(smartLine(fallback, [])).toBe("Smart cut not possible here; the clip is cut in accurate mode");
  });
});

describe("mode", () => {
  const queue = {
    version: 1 as const,
    defaults: { mode: "accurate" as const },
    videos: [
      { input: "/v/a.mp4", options: { mode: "smart" as const, smart_fallback: false }, clips: [] },
      { input: "/v/b.mp4", clips: [] },
    ],
  };

  it("takes the video's mode, then the queue's, then the settings', then fast", () => {
    expect(effectiveMode(queue, { mode: "fast" }, "/v/a.mp4")).toBe("smart");
    expect(effectiveMode(queue, { mode: "fast" }, "/v/b.mp4")).toBe("accurate");
    expect(effectiveMode({ ...queue, defaults: {} }, { mode: "smart" }, "/v/c.mp4")).toBe("smart");
    expect(effectiveMode({ ...queue, defaults: {} }, undefined, "/v/c.mp4")).toBe("fast");
  });

  it("plans with the queue's and the video's options", () => {
    expect(planOptions(queue, "/v/a.mp4")).toEqual({ mode: "smart", smart_fallback: false });
    expect(planOptions(queue, "/v/b.mp4")).toEqual({ mode: "accurate" });
  });
});

describe("run panel", () => {
  const clip = (smart?: SmartOutcome) =>
    ({ status: "succeeded", output: "/o/x.mp4", measure: "clip", actual: { start: 0, end: 1 }, adjusted: false, smart }) as JobOutcome;

  it("labels what smart mode did", () => {
    const smart = { mode_used: "smart" as const, copied_frames: 400, reencoded_frames: 35, copied_share: 0.9195, fallback: null };
    expect(smartOutcomeLabel(clip(smart))).toBe("Smart · 92% copied");
    const fell = { mode_used: "accurate" as const, copied_frames: 0, reencoded_frames: 435, copied_share: 0, fallback: "seam_check" as const };
    expect(smartOutcomeLabel(clip(fell))).toBe("Accurate (smart fallback: seam_check)");
    expect(smartOutcomeLabel(clip(undefined))).toBe("");
  });
});
