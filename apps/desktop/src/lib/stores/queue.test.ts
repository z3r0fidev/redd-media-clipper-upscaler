import { describe, expect, it } from "vitest";
import { addClip, addSource, clipCount, emptyQueue, merge, removeClip, removeSource, runnable, setSourceOptions, updateClip } from "./queue";

describe("queue", () => {
  it("adds sources once and clips in order", () => {
    let q = emptyQueue();
    q = addSource(q, "/v/a.mp4");
    q = addSource(q, "/v/a.mp4");
    q = addClip(q, "/v/a.mp4", { start: 1, end: 2 });
    q = addClip(q, "/v/b.mp4", { start: 3, duration: 1, name: "x" });
    expect(q.videos.map((v) => v.input)).toEqual(["/v/a.mp4", "/v/b.mp4"]);
    expect(clipCount(q)).toBe(2);
  });

  it("keeps end and duration mutually exclusive and drops empty names", () => {
    let q = addClip(emptyQueue(), "a", { start: 1, end: 5, name: "n" });
    q = updateClip(q, "a", 0, { duration: 2 });
    expect(q.videos[0]!.clips[0]).toEqual({ start: 1, duration: 2, name: "n" });
    q = updateClip(q, "a", 0, { end: "end", name: "" });
    expect(q.videos[0]!.clips[0]).toEqual({ start: 1, end: "end" });
  });

  it("is immutable", () => {
    const q0 = addClip(emptyQueue(), "a", { start: 1 });
    const q1 = removeClip(q0, "a", 0);
    expect(q0.videos[0]!.clips).toHaveLength(1);
    expect(q1.videos[0]!.clips).toHaveLength(0);
  });

  it("sets per-source options and removes empty ones", () => {
    let q = addSource(emptyQueue(), "a");
    q = setSourceOptions(q, "a", { audio: [2], subtitles: "none" });
    expect(q.videos[0]!.options).toEqual({ audio: [2], subtitles: "none" });
    q = setSourceOptions(q, "a", { audio: undefined });
    expect(q.videos[0]!.options).toBeUndefined();
  });

  it("merges imports and only plans sources with clips", () => {
    let q = addClip(emptyQueue(), "a", { start: 1 });
    q = addSource(q, "empty");
    q = merge(q, { version: 1, videos: [{ input: "a", clips: [{ start: 5 }] }, { input: "b", clips: [{ start: 0 }] }] });
    expect(q.videos.find((v) => v.input === "a")!.clips).toHaveLength(2);
    expect(runnable(q).videos.map((v) => v.input)).toEqual(["a", "b"]);
    expect(removeSource(q, "a").videos.map((v) => v.input)).toEqual(["empty", "b"]);
  });
});
