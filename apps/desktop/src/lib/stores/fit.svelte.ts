// Budgeted video state (Svelte 5 runes, feature 005): the video targets, the clip panel's
// choice, and the prediction for the marked range (debounced; the backend cancels an older
// plan, and an older answer is never shown). Pure logic lives in fit.ts.
import { api, errorText, type FitTargetList, type RangeArg } from "../api";
import type { FitSummary } from "../bindings/FitSummary";
import type { Issue } from "../bindings/Issue";
import type { Notice } from "../bindings/Notice";
import type { PartialOptions } from "../bindings/PartialOptions";
import { type FitChoice, NO_FIT } from "./fit";

export const fit = $state({
  targets: null as FitTargetList | null,
  targetsError: "",
  choice: { ...NO_FIT } as FitChoice,
  summary: null as FitSummary | null,
  notices: [] as Notice[],
  issues: [] as Issue[],
  planning: false,
  planError: "",
});

let loading = false;

/** The targets, once per app start (again after a failure, or when asked: Settings › Targets). */
export async function loadFitTargets(force = false): Promise<void> {
  if ((fit.targets && !force) || loading) return;
  loading = true;
  try {
    fit.targets = await api.listFitTargets();
    fit.targetsError = "";
  } catch (e) {
    fit.targetsError = errorText(e);
  } finally {
    loading = false;
  }
}

let planKey = "";
let planTimer: ReturnType<typeof setTimeout> | undefined;
let planSeq = 0;

function clear(): void {
  fit.summary = null;
  fit.notices = [];
  fit.issues = [];
  fit.planError = "";
}

/** Keeps the prediction in step with the source, range and options (250 ms debounce). */
export function syncFitPlan(path: string, range: RangeArg, options: PartialOptions): void {
  const key = JSON.stringify([path, range, options]);
  if (key === planKey) return;
  planKey = key;
  planSeq++;
  clear();
  fit.planning = true;
  clearTimeout(planTimer);
  planTimer = setTimeout(() => void plan(path, range, options), 250);
}

/** Stops following the range (no target, no range, or another view). */
export function stopFitPlan(): void {
  clearTimeout(planTimer);
  planKey = "";
  planSeq++;
  clear();
  fit.planning = false;
}

async function plan(path: string, range: RangeArg, options: PartialOptions): Promise<void> {
  const seq = planSeq;
  try {
    const r = await api.fitPlan(path, range, options);
    if (seq !== planSeq) return;
    fit.summary = r.fit;
    fit.notices = r.notices;
    fit.issues = r.issues;
  } catch (e) {
    if (seq !== planSeq) return;
    clear();
    fit.planError = errorText(e);
  } finally {
    if (seq === planSeq) fit.planning = false;
  }
}
