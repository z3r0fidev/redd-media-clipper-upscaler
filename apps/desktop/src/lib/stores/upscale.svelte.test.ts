// The upscale view's orchestration with a mocked backend: stale results never win, the still
// is cleared when the source changes or the view closes, and failures are retried.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({
  preview: [] as { path: string; at: number; resolve: (v: unknown) => void; reject: (e: unknown) => void }[],
  analyze: [] as { id: string; resolve: (v: unknown) => void; reject: (e: unknown) => void }[],
  cancelled: [] as string[],
}));

vi.mock("../api", () => ({
  errorText: (e: unknown) => String(e),
  api: {
    listPresets: async () => ({ default: "phone-fill", presets: [], problems: [] }),
    setSettings: async () => {},
    cancelAnalysis: async (id: string) => void calls.cancelled.push(id),
    analyzeReframe: (id: string) => new Promise((resolve, reject) => calls.analyze.push({ id, resolve, reject })),
    upscalePreview: (path: string, _span: unknown, at: number) =>
      new Promise((resolve, reject) => calls.preview.push({ path, at, resolve, reject })),
  },
}));

const { app } = await import("./app.svelte");
const { initUpscale, retryAnalysis, setOptions, stopView, syncView, upscale } = await import("./upscale.svelte");

const still = (name: string) => ({ before: `${name}-b`, after: `${name}-a`, size: [1080, 2340], approximate: false, crop_x: 0, in_fallback: false });
const view = (path: string, at = 1) => ({ path, range: { start: 0, end: null }, at, playing: false, duration: 10 });
const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

beforeEach(async () => {
  vi.useFakeTimers();
  app.settings = { defaults: {}, proxy_cache_max_mb: 100, upscale_defaults: { reframe: "center" } } as never;
  await initUpscale();
});

afterEach(() => {
  stopView();
  calls.preview.length = 0;
  calls.analyze.length = 0;
  calls.cancelled.length = 0;
  vi.useRealTimers();
});

describe("upscale view orchestration", () => {
  it("applies the saved choices and debounces previews; only the latest still is shown", async () => {
    expect(upscale.options.reframe).toBe("center");
    syncView(view("/a.mp4", 1));
    syncView(view("/a.mp4", 2));
    vi.advanceTimersByTime(300);
    expect(calls.preview.map((c) => c.at)).toEqual([2]);
    syncView(view("/a.mp4", 3));
    vi.advanceTimersByTime(300);
    expect(calls.preview).toHaveLength(2);
    calls.preview[1]!.resolve(still("t3"));
    await flush();
    calls.preview[0]!.resolve(still("t2"));
    await flush();
    expect(upscale.preview?.after).toBe("t3-a");
  });

  it("clears the still when the source changes or the view closes", async () => {
    syncView(view("/a.mp4"));
    vi.advanceTimersByTime(300);
    calls.preview[0]!.resolve(still("a"));
    await flush();
    expect(upscale.preview?.after).toBe("a-a");
    syncView(view("/b.mp4"));
    expect(upscale.preview).toBeNull();
    vi.advanceTimersByTime(300);
    stopView();
    calls.preview[1]!.resolve(still("b"));
    await flush();
    expect(upscale.preview).toBeNull();
    expect(upscale.analyzing).toBe(false);
  });

  it("retries a failed preview at the same time", async () => {
    syncView(view("/a.mp4"));
    vi.advanceTimersByTime(300);
    calls.preview[0]!.reject("engine busy");
    await flush();
    expect(upscale.previewError).toBe("engine busy");
    syncView(view("/a.mp4"));
    vi.advanceTimersByTime(300);
    expect(calls.preview).toHaveLength(2);
  });

  it("cancels a superseded analysis, ignores its result and can run again after an error", async () => {
    setOptions({ reframe: "smart" });
    syncView(view("/a.mp4"));
    expect(calls.analyze).toHaveLength(1);
    setOptions({ motion: "fast" });
    syncView(view("/a.mp4"));
    expect(calls.analyze).toHaveLength(2);
    expect(calls.cancelled).toEqual([calls.analyze[0]!.id]);
    calls.analyze[0]!.resolve({ analysis: { key: "old" }, note: null });
    await flush();
    expect(upscale.analysis).toBeNull();
    calls.analyze[1]!.reject("decoder failed");
    await flush();
    expect(upscale.analysisError).toBe("decoder failed");
    // The tier is not part of what is analysed, so changing it alone does not re-run.
    setOptions({ tier: "cpu" });
    syncView(view("/a.mp4"));
    expect(calls.analyze).toHaveLength(2);
    retryAnalysis();
    syncView(view("/a.mp4"));
    expect(calls.analyze).toHaveLength(3);
    calls.analyze[2]!.resolve({ analysis: null, note: "no face model" });
    await flush();
    expect(upscale.analysisNote).toBe("no face model");
    setOptions({ reframe: "center", motion: "normal", tier: "auto" });
  });
});
