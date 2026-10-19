import { describe, expect, it } from "vitest";
import type { Plan } from "../bindings/Plan";
import { applyEvent, initRun, statusLabel, upscaleDetail } from "./run";

const plan: Plan = {
  run_id: "r1",
  cancelled: false,
  issues: [],
  jobs: [1, 2].map((id) => ({
    id,
    source: "/v/a.mp4",
    output: `/o/${id}.mp4`,
    on_exists: "skip",
    disposition: id === 2 ? { status: "skip", reason: "exists" } : { status: "run" },
    requested: { start: 0, end: 1 },
    warnings: [],
  })),
};

describe("run reducer", () => {
  it("tracks a job from queued to done", () => {
    let r = initRun(plan);
    expect(r.jobs.map((j) => j.status)).toEqual(["queued", "skipped"]);
    r = applyEvent(r, { type: "job_started", job_id: 1 });
    r = applyEvent(r, { type: "job_progress", job_id: 1, fraction: 0.5, out_time: 0.5, speed: 2, eta_ms: 250 });
    expect(r.jobs[0]).toMatchObject({ status: "running", fraction: 0.5, etaMs: 250 });
    r = applyEvent(r, { type: "job_stage", job_id: 1, stage: "render", index: 2, of: 3 });
    expect(r.jobs[0]!.stage).toBe("render");
    r = applyEvent(r, { type: "job_warning", job_id: 1, message: "w" });
    r = applyEvent(r, { type: "job_warning", job_id: 1, message: "w" });
    expect(r.jobs[0]!.warnings).toEqual(["w"]);
    r = applyEvent(r, {
      type: "job_finished",
      job_id: 1,
      outcome: { status: "succeeded", output: "/o/1_1.mp4", measure: "clip", actual: { start: 0, end: 1 }, adjusted: false },
    });
    expect(r.jobs[0]).toMatchObject({ status: "done", fraction: 1, output: "/o/1_1.mp4" });
  });

  it("never moves run progress backwards and finishes with the summary", () => {
    let r = initRun(plan);
    r = applyEvent(r, { type: "run_progress", done: 1, total: 2, fraction: 0.6 });
    r = applyEvent(r, { type: "run_progress", done: 1, total: 2, fraction: 0.4 });
    expect(r.fraction).toBe(0.6);
    const summary = { succeeded: 1, skipped: 1, failed: 0, cancelled: 0, exit_code: 0, jobs: [] };
    r = applyEvent(r, { type: "run_finished", summary });
    expect(r.finished).toBe(true);
    expect(r.summary).toEqual(summary);
  });

  it("maps failures and cancellations", () => {
    let r = initRun(plan);
    r = applyEvent(r, { type: "job_finished", job_id: 1, outcome: { status: "failed", error: { code: "ENGINE_FAILED", message: "x" } } });
    expect(r.jobs[0]!.status).toBe("failed");
    r = applyEvent(initRun(plan), { type: "job_finished", job_id: 1, outcome: { status: "cancelled" } });
    expect(r.jobs[0]!.status).toBe("cancelled");
  });
});

describe("job labels", () => {
  const base = { id: 1, source: "a", output: "b", fraction: 0, warnings: [], requestedStart: 0 };
  it("names the stage of a running upscale and keeps clip labels", () => {
    expect(statusLabel({ ...base, status: "running", stage: "analyze" })).toBe("Finding faces");
    expect(statusLabel({ ...base, status: "running", stage: "render" })).toBe("Upscaling");
    expect(statusLabel({ ...base, status: "running" })).toBe("Cutting");
    expect(statusLabel({ ...base, status: "done", stage: "verify" })).toBe("Done");
  });

  it("summarises an upscale outcome", () => {
    const span = { start: 0, end: 1 };
    const o = (extra: object) =>
      ({ status: "succeeded", output: "b", measure: "upscale", span, tier_used: "cpu", tier_fallback: false, fallback_ranges: [], no_upscale_needed: false, ...extra }) as never;
    expect(upscaleDetail(o({}))).toBe("CPU tier");
    expect(upscaleDetail(o({ tier_fallback: true, fallback_ranges: [span, span] }))).toBe("CPU tier · GPU failed, redone on the CPU · 2 blurred ranges");
    expect(upscaleDetail({ status: "succeeded", output: "b", measure: "clip", actual: span, adjusted: false } as never)).toBe("");
  });
});
