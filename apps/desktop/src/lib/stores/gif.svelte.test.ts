// The GIF view's orchestration with a mocked backend: the 400 ms debounce, one estimate in
// flight (the old one is cancelled and never wins), and Export now opening the output.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({
  estimates: [] as { id: string; options: { width?: number }; resolve: (v: unknown) => void }[],
  cancelled: [] as string[],
  opened: [] as [string, number][],
}));

vi.mock("../api", () => ({
  errorText: (e: unknown) => String(e),
  api: {
    listGifTargets: async () => ({ default: "reddit-post", targets: [], problems: [] }),
    setSettings: async () => {},
    cancelEstimate: async (id: string) => void calls.cancelled.push(id),
    gifEstimate: (id: string, _p: string, _s: unknown, options: { width?: number }) =>
      new Promise((resolve) => calls.estimates.push({ id, options, resolve })),
    openOutput: async (runId: string, jobId: number) => {
      calls.opened.push([runId, jobId]);
      return { url: "http://127.0.0.1:1/t/x", kind: "image", bytes: 1000, width: 320, height: 180 };
    },
  },
}));

vi.mock("./runner.svelte", async () => {
  const { app } = await import("./app.svelte");
  return {
    canRun: () => true,
    runManifest: async () => {
      app.run = {
        runId: "r1",
        jobs: [],
        fraction: 1,
        done: 1,
        total: 1,
        finished: true,
        summary: { jobs: [{ id: 1, status: "succeeded", measure: "gif" }] },
      } as never;
      return true;
    },
  };
});

const { app } = await import("./app.svelte");
const { exportNow, gif, initGif, setGifOptions, stopEstimate, syncEstimate } = await import("./gif.svelte");
const range = { start: 0, end: 3 };
const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};
const estimate = (bytes: number) => ({ estimate: { bytes, from: "search", lowered: [], settings: {}, mp4_bytes: null }, notes: [], notices: [] });

beforeEach(async () => {
  vi.useFakeTimers();
  app.settings = { defaults: {}, gif_defaults: { width: 320 } } as never;
  await initGif();
});

afterEach(() => {
  stopEstimate();
  calls.estimates.length = 0;
  calls.cancelled.length = 0;
  vi.useRealTimers();
});

describe("gif view", () => {
  it("debounces, keeps one estimate in flight, and the latest wins", async () => {
    expect(gif.options.width).toBe(320);
    syncEstimate("/v/a.mp4", range);
    await vi.advanceTimersByTimeAsync(399);
    expect(calls.estimates).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(calls.estimates).toHaveLength(1);
    setGifOptions({ width: 480 });
    syncEstimate("/v/a.mp4", range);
    await vi.advanceTimersByTimeAsync(400);
    expect(calls.estimates).toHaveLength(2);
    expect(calls.cancelled).toEqual([calls.estimates[0]!.id]);
    calls.estimates[1]!.resolve(estimate(2_000_000));
    calls.estimates[0]!.resolve(estimate(9));
    await flush();
    expect(gif.estimate?.estimate.bytes).toBe(2_000_000);
  });

  it("export now runs the job and opens its output", async () => {
    await exportNow("/v/a.mp4", range);
    expect(calls.opened).toEqual([["r1", 1]]);
    expect(gif.result?.output.kind).toBe("image");
    expect(app.queue.videos[0]?.clips[0]?.options?.gif?.width).toBe(480);
  });
});
