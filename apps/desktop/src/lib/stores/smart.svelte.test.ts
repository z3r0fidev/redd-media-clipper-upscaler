// The smart cut store with a mocked backend (feature 004, T049): keyframes load in the
// background and a change of source cancels the old request; the plan follows the range after a
// short pause, and an older answer never replaces a newer one.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({
  keyframes: [] as { id: string; path: string; resolve: (v: unknown) => void; reject: (e: unknown) => void }[],
  cancelled: [] as string[],
  plans: [] as { path: string; span: { start: number; end: number | null }; resolve: (v: unknown) => void; reject: (e: unknown) => void }[],
}));

vi.mock("../api", () => ({
  errorText: (e: unknown) => String(e),
  api: {
    keyframes: (id: string, path: string) =>
      new Promise((resolve, reject) => calls.keyframes.push({ id, path, resolve, reject })),
    cancelKeyframes: async (id: string) => void calls.cancelled.push(id),
    smartPlan: (path: string, span: { start: number; end: number | null }) =>
      new Promise((resolve, reject) => calls.plans.push({ path, span, resolve, reject })),
  },
}));

import { loadKeyframes, smart, stopPlan, syncPlan } from "./smart.svelte";

const index = (at: number[]) => ({ window: null, keyframes: at.map((t) => ({ at: t, clean: true })), frame_duration: 0.04, truncated: false });
const summary = (frames: number) => ({
  requested: true,
  mode: "smart",
  strict: false,
  head: null,
  copy: { span: { start: 4, end: 8 }, frames },
  tail: null,
  copied_share: 1,
  fallback: null,
});

beforeEach(() => {
  vi.useFakeTimers();
  calls.keyframes.length = 0;
  calls.cancelled.length = 0;
  calls.plans.length = 0;
});
afterEach(() => {
  stopPlan();
  void loadKeyframes(null);
  vi.useRealTimers();
});

describe("keyframes", () => {
  it("loads in the background; a new source cancels the old request and its answer is dropped", async () => {
    const first = loadKeyframes("/v/a.mp4");
    expect(smart.loadingIndex).toBe(true);
    const second = loadKeyframes("/v/b.mp4");
    expect(calls.cancelled).toEqual([calls.keyframes[0]!.id]);
    calls.keyframes[1]!.resolve(index([0, 2]));
    calls.keyframes[0]!.resolve(index([0, 5]));
    await Promise.all([first, second]);
    expect(smart.index?.keyframes.map((k) => k.at)).toEqual([0, 2]);
    expect(smart.loadingIndex).toBe(false);
  });

  it("reports a failure without blocking", async () => {
    const p = loadKeyframes("/v/a.mp4");
    calls.keyframes[0]!.reject("the file has no video stream");
    await p;
    expect(smart.index).toBeNull();
    expect(smart.indexError).toBe("the file has no video stream");
  });
});

describe("plan", () => {
  it("waits for the range to settle and keeps only the newest answer", async () => {
    syncPlan("/v/a.mp4", { start: 1, end: 9 }, {});
    syncPlan("/v/a.mp4", { start: 4, end: 8 }, {});
    await vi.advanceTimersByTimeAsync(300);
    expect(calls.plans.map((p) => p.span)).toEqual([{ start: 4, end: 8 }]);
    syncPlan("/v/a.mp4", { start: 4, end: 9 }, {});
    await vi.advanceTimersByTimeAsync(300);
    expect(calls.plans).toHaveLength(2);
    calls.plans[1]!.resolve({ summary: summary(125), notices: [] });
    calls.plans[0]!.reject("cancelled: a newer smart plan replaced this one");
    await vi.runAllTimersAsync();
    expect(smart.summary?.copy?.frames).toBe(125);
    expect(smart.planError).toBe("");
  });

  it("drops the old range's parts as soon as the range changes (T059 review)", async () => {
    syncPlan("/v/a.mp4", { start: 4, end: 8 }, {});
    await vi.advanceTimersByTimeAsync(300);
    calls.plans[0]!.resolve({ summary: summary(100), notices: [] });
    await vi.runAllTimersAsync();
    expect(smart.summary?.copy?.frames).toBe(100);
    syncPlan("/v/a.mp4", { start: 4, end: 9 }, {});
    expect(smart.summary).toBeNull();
  });

  it("ignores the old plan's cancel after a stop and a restart of the same range", async () => {
    syncPlan("/v/a.mp4", { start: 4, end: 8 }, {});
    await vi.advanceTimersByTimeAsync(300);
    stopPlan();
    syncPlan("/v/a.mp4", { start: 4, end: 8 }, {});
    await vi.advanceTimersByTimeAsync(300);
    calls.plans[0]!.reject("cancelled: a newer smart plan replaced this one");
    await vi.runAllTimersAsync();
    expect(smart.planError).toBe("");
    calls.plans[1]!.resolve({ summary: summary(125), notices: [] });
    await vi.runAllTimersAsync();
    expect(smart.summary?.copy?.frames).toBe(125);
  });

  it("shows an error from the newest plan, and stopping clears it", async () => {
    syncPlan("/v/a.mp4", { start: 1, end: 3 }, { smart_fallback: false });
    await vi.advanceTimersByTimeAsync(300);
    calls.plans[0]!.reject("smart cut not possible: the video has open GOPs");
    await vi.runAllTimersAsync();
    expect(smart.planError).toBe("smart cut not possible: the video has open GOPs");
    stopPlan();
    expect(smart.summary).toBeNull();
    expect(smart.planError).toBe("");
  });
});
