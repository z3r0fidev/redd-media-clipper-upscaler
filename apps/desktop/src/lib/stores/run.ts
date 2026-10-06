// Event → UI-state reducer for a run (unit-tested).
import type { Event } from "../bindings/Event";
import type { JobOutcome } from "../bindings/JobOutcome";
import type { Plan } from "../bindings/Plan";
import type { RunSummary } from "../bindings/RunSummary";

export interface JobView {
  id: number;
  source: string;
  output: string;
  status: "queued" | "running" | "done" | "skipped" | "failed" | "cancelled";
  fraction: number;
  speed?: number;
  etaMs?: number;
  warnings: string[];
  outcome?: JobOutcome;
}

export interface RunView {
  runId: string;
  jobs: JobView[];
  fraction: number;
  etaMs?: number;
  done: number;
  total: number;
  finished: boolean;
  summary?: RunSummary;
}

export function initRun(plan: Plan): RunView {
  return {
    runId: plan.run_id,
    jobs: plan.jobs.map((j) => ({
      id: j.id,
      source: j.source,
      output: j.output,
      status: j.disposition.status === "skip" ? "skipped" : "queued",
      fraction: 0,
      warnings: [...j.warnings],
    })),
    fraction: 0,
    done: 0,
    total: plan.jobs.length,
    finished: false,
  };
}

const outcomeStatus: Record<JobOutcome["status"], JobView["status"]> = {
  succeeded: "done",
  skipped: "skipped",
  failed: "failed",
  cancelled: "cancelled",
};

function mapJob(r: RunView, id: number, f: (j: JobView) => JobView): RunView {
  return { ...r, jobs: r.jobs.map((j) => (j.id === id ? f(j) : j)) };
}

export function applyEvent(r: RunView, e: Event): RunView {
  switch (e.type) {
    case "run_started":
      return { ...r, runId: e.run_id, total: e.total_jobs };
    case "job_started":
      return mapJob(r, e.job_id, (j) => ({ ...j, status: "running" }));
    case "job_progress":
      return mapJob(r, e.job_id, (j) => ({
        ...j,
        status: j.status === "queued" ? "running" : j.status,
        fraction: Math.max(j.fraction, e.fraction),
        speed: e.speed,
        etaMs: e.eta_ms,
      }));
    case "job_warning":
      return mapJob(r, e.job_id, (j) =>
        j.warnings.includes(e.message) ? j : { ...j, warnings: [...j.warnings, e.message] },
      );
    case "job_finished":
      return mapJob(r, e.job_id, (j) => ({
        ...j,
        status: outcomeStatus[e.outcome.status],
        fraction: e.outcome.status === "succeeded" ? 1 : j.fraction,
        output: e.outcome.status === "succeeded" ? e.outcome.output : j.output,
        outcome: e.outcome,
        etaMs: undefined,
      }));
    case "run_progress":
      // Never move backwards (events can be coalesced/throttled).
      return { ...r, fraction: Math.max(r.fraction, e.fraction), etaMs: e.eta_ms, done: e.done, total: e.total };
    case "run_finished":
      return { ...r, finished: true, fraction: 1, etaMs: undefined, summary: e.summary };
  }
}
