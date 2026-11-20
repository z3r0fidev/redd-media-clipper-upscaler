import { describe, expect, it } from "vitest";
import type { FitSummary } from "../bindings/FitSummary";
import type { Issue } from "../bindings/Issue";
import type { JobOutcome } from "../bindings/JobOutcome";
import type { VideoTarget } from "../bindings/VideoTarget";
import { bytes, choiceOf, clock, forApp, fitLabel, fitLine, fitNotes, fitOutcomeLabel, fitPartial, NO_FIT, refusal, withFit } from "./fit";

const reddit: VideoTarget = {
  schema: 1, id: "reddit-video", version: 1, name: "Reddit video", verified: false, max_bytes: 1e9,
  max_duration: 900, long_side: 1920, floor_short_side: 480, max_fps: 60,
  quality: { crf: 20, max_bpp: 0.1, min_bpp: 0.02 }, audio: { kbps: 128, floor_kbps: 64 },
};

describe("the target picker", () => {
  it("builds PartialFit with only the keys the user set", () => {
    expect(fitPartial(NO_FIT)).toBeNull();
    expect(fitPartial({ ...NO_FIT, target: "reddit-video" })).toEqual({ target: "reddit-video" });
    // Custom limits fill max_size and max_length (numbers stay numbers, text goes to the core).
    const custom = { ...NO_FIT, target: "reddit-video", custom: true, maxSize: "50MB", maxLength: "5:00", allowTrim: true };
    expect(fitPartial(custom)).toEqual({ target: "reddit-video", max_size: "50MB", max_length: "5:00", allow_trim: true });
    expect(fitPartial({ ...custom, maxSize: "3000000", maxLength: "90" })).toMatchObject({ max_size: 3000000, max_length: 90 });
    // Empty fields fall back to the target's limits; the fields are ignored unless "custom".
    expect(fitPartial({ ...custom, maxSize: " ", maxLength: "", allowTrim: false })).toEqual({ target: "reddit-video" });
    expect(fitPartial({ ...custom, custom: false, allowTrim: false })).toEqual({ target: "reddit-video" });
  });

  it("round-trips through a manifest unchanged", () => {
    for (const c of [
      { ...NO_FIT, target: "reddit-video" },
      { ...NO_FIT, target: "my-sub", custom: true, maxSize: "50MB", maxLength: "", allowTrim: true },
    ]) {
      const json = JSON.parse(JSON.stringify(fitPartial(c)));
      expect(choiceOf(json)).toEqual(c);
    }
    expect(choiceOf(undefined)).toEqual(NO_FIT);
    expect(choiceOf({ enabled: false })).toEqual(NO_FIT);
  });

  it("turns GIF export off for a fitted clip, and back", () => {
    const clip = { start: 1, end: 4, options: { mode: "smart" as const } };
    const fitted = withFit(clip, { target: "reddit-video" }, true);
    expect(fitted.options).toEqual({ mode: "smart", fit: { target: "reddit-video" }, gif: { enabled: false } });
    expect(withFit(clip, { target: "reddit-video" }, false).options).toEqual({ mode: "smart", fit: { target: "reddit-video" } });
    expect(withFit({ start: 0, end: 1 }, null, true)).toEqual({ start: 0, end: 1 });
    expect(withFit({ start: 0, end: 1, options: { fit: { target: "x" } } }, null, false)).toEqual({ start: 0, end: 1 });
  });

  it("shows the core's mode note with the prediction", () => {
    const notes = fitNotes([
      { code: "SMART_NOT_APPLIED", message: "smart cut does not apply: the clip is re-encoded to fit its target" },
      { code: "FIT_LOWERED", message: "size lowered to 1440x810 to fit 3.0 MB" },
      { code: "KEYFRAME_SNAP", message: "not shown" } as never,
    ]);
    expect(notes).toEqual([
      "smart cut does not apply: the clip is re-encoded to fit its target",
      "size lowered to 1440x810 to fit 3.0 MB",
    ]);
  });
});

describe("the prediction", () => {
  const f: FitSummary = {
    target: "reddit-video", target_verified: false, cap: 3e6, max_length: 900, len: 6, trimmed_from: null,
    mode: "budget", size: [1920, 1080], fps: 30, video_kbps: 3792, crf: null, audio_kbps: 128,
    predicted_bytes: 2_940_000, lowered: [], passes: 2,
  };
  it("reads like the CLI's dry run", () => {
    expect(fitLine(f, [reddit])).toBe("Reddit video: 2.9 MB predicted of 3.0 MB · limit 15:00 · 1920x1080 30 fps · 2 passes");
    expect(fitLine({ ...f, mode: "quality", passes: 1, audio_kbps: null, fps: 29.97 }, [])).toBe(
      "reddit-video: 2.9 MB at most of 3.0 MB · limit 15:00 · 1920x1080 29.97 fps · no audio · 1 pass",
    );
    expect(clock(3723)).toBe("1:02:03");
    expect([bytes(200_000), bytes(8e6), bytes(1073741824)]).toEqual(["200 kB", "8.0 MB", "1.07 GB"]);
  });

  it("a refusal disables Add, and says whether trimming helps", () => {
    const issue = (code: Issue["code"], severity: Issue["severity"] = "error"): Issue => ({ severity, code, origin: { kind: "gui" }, message: code });
    expect(refusal([])).toBeNull();
    expect(refusal([issue("FIT_LOWERED", "warning")])).toBeNull();
    expect(refusal([issue("RANGE_TOO_LONG")])).toEqual({ message: "RANGE_TOO_LONG", trimmable: true });
    expect(refusal([issue("BUDGET_UNREACHABLE")])?.trimmable).toBe(true);
    expect(refusal([issue("ENGINE_MISSING")])).toEqual({ message: "ENGINE_MISSING", trimmable: false });
    // The core's text names CLI flags; the panel names its own controls.
    expect(forApp("the range (0:25) is longer than short allows (0:10); use --fit-allow-trim to keep the first 0:10")).toBe(
      "the range (0:25) is longer than short allows (0:10); allow trimming to keep the first 0:10",
    );
    expect(forApp("6 s cannot fit 200 kB even at 854x480 (needs at least 1.2 MB); raise --fit-max-size or use --fit-allow-trim")).toBe(
      "6 s cannot fit 200 kB even at 854x480 (needs at least 1.2 MB); raise the size limit or allow trimming",
    );
  });
});

describe("queue and results", () => {
  it("labels a fitted clip and its result", () => {
    expect(fitLabel(undefined, [reddit])).toBe("");
    expect(fitLabel({ target: "reddit-video" }, [reddit])).toBe("Reddit video · 1.00 GB");
    expect(fitLabel({ target: "reddit-video", max_size: 3e6, allow_trim: true }, [reddit])).toBe("Reddit video · 3.0 MB · may trim");
    expect(fitLabel({ max_size: "50MB" }, [])).toBe("reddit-video · 50MB");
    const ok = (fit: object): JobOutcome =>
      ({ status: "succeeded", output: "/o.mp4", measure: "clip", actual: { start: 0, end: 1 }, adjusted: false, fit }) as unknown as JobOutcome;
    const fit = { bytes: 2_950_000, cap: 3e6, mode: "budget", video_kbps: 1, audio_kbps: 128, attempts: [{ video_kbps: 1, bytes: 1 }], trimmed: false, size: [1, 1], fps: 30 };
    expect(fitOutcomeLabel(ok(fit))).toBe("3.0 MB of 3.0 MB");
    expect(fitOutcomeLabel(ok({ ...fit, bytes: 2_400_000, trimmed: true, attempts: [fit.attempts[0], fit.attempts[0]] }))).toBe(
      "2.4 MB of 3.0 MB · trimmed · 2 attempts",
    );
    expect(fitOutcomeLabel({ status: "failed" } as unknown as JobOutcome)).toBe("");
  });
});
