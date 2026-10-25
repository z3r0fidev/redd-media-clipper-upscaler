import { describe, expect, it } from "vitest";
import { addGif, DEFAULT_GIF, fullGif, gifClip, isGif, megabytes, resolveGif, withTrim } from "./gif";
import { emptyQueue } from "./queue";

describe("gif options", () => {
  it("resolves the saved defaults over the built-ins", () => {
    expect(resolveGif({})).toEqual(DEFAULT_GIF);
    const o = resolveGif({ target: "reddit-comment", width: 480, budget: null, dither: null });
    expect(o.target).toBe("reddit-comment");
    expect(o.width).toBe(480);
    expect(o.budget).toBeNull();
    expect("dither" in o).toBe(false);
  });

  it("writes every field into the queue entry", () => {
    const o = { ...DEFAULT_GIF, width: 320, shape: "4:5" as const };
    const p = fullGif(o);
    expect(p).toMatchObject({ target: "reddit-post", format: "gif", shape: "4:5", width: 320, budget: {} });
    expect("fps" in p).toBe(false);
    const c = gifClip(o, { start: 1.23456, end: null });
    expect(c).toEqual({ start: 1.235, end: "end", options: { gif: p } });
    expect(isGif(c)).toBe(true);
    expect(isGif({ start: 0, end: 1, options: { gif: { enabled: false } } })).toBe(false);
  });

  it("adds to the queue, replacing the same range", () => {
    const c = gifClip(DEFAULT_GIF, { start: 0, end: 2 });
    const a = addGif(emptyQueue(), "/v/a.mp4", c);
    expect(a.index).toBe(0);
    const b = addGif(a.queue, "/v/a.mp4", gifClip({ ...DEFAULT_GIF, width: 320 }, { start: 0, end: 2 }));
    expect(b.index).toBe(0);
    expect(b.queue.videos[0]!.clips).toHaveLength(1);
    expect(b.queue.videos[0]!.clips[0]!.options?.gif?.width).toBe(320);
  });

  it("re-queues with allow_trim and shows sizes", () => {
    const c = withTrim(gifClip(DEFAULT_GIF, { start: 0, end: 2 }));
    expect(c.options?.gif?.allow_trim).toBe(true);
    expect(megabytes(2_400_000)).toBe("2.4 MB");
    expect(megabytes(68_000)).toBe("68 kB");
  });
});
