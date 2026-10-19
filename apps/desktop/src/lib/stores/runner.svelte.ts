// Plans and runs a manifest, feeding the run panel. Shared by the queue's Run and the upscale
// view's "Upscale now" (which runs one job on its own).
import { api, errorText } from "../api";
import type { Manifest } from "../manifest";
import { app, toast } from "./app.svelte";
import { applyEvent, initRun } from "./run";

export const runner = $state({ busy: false });

/** True when a run can start (none planning or running). */
export function canRun(): boolean {
  return !runner.busy && (app.run == null || app.run.finished);
}

/** Plans `manifest` and runs it. Plan errors are listed under the queue; returns whether it ran. */
export async function runManifest(manifest: Manifest): Promise<boolean> {
  if (!canRun()) return false;
  runner.busy = true;
  app.runIssues = [];
  try {
    const plan = await api.plan(manifest);
    const errors = plan.issues.filter((i) => i.severity === "error");
    if (errors.length) {
      app.runIssues = errors.map((i) => i.message);
      app.run = null;
      return false;
    }
    app.run = initRun(plan);
    const summary = await api.run(plan.run_id, (e) => {
      if (app.run) app.run = applyEvent(app.run, e);
    });
    if (app.run) app.run = { ...app.run, finished: true, summary };
    return true;
  } catch (e) {
    toast(errorText(e), "error");
    return false;
  } finally {
    runner.busy = false;
  }
}
