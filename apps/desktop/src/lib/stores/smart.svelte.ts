// Smart cut state (Svelte 5 runes, feature 004): the selected source's keyframe index, loaded in
// the background (a change of source cancels it), and the smart plan of the marked range
// (debounced; the backend cancels an older plan, and an older answer is never shown). Pure
// layout lives in smart.ts.
import { api, errorText, type RangeArg } from "../api";
import type { KeyframeIndex } from "../bindings/KeyframeIndex";
import type { Notice } from "../bindings/Notice";
import type { PartialOptions } from "../bindings/PartialOptions";
import type { SmartSummary } from "../bindings/SmartSummary";

export const smart = $state({
  index: null as KeyframeIndex | null,
  loadingIndex: false,
  indexError: "",
  summary: null as SmartSummary | null,
  notices: [] as Notice[],
  planning: false,
  planError: "",
});

let indexRequest: string | null = null;

/** Loads `path`'s keyframes (null: clears them). Never blocks the view. */
export async function loadKeyframes(path: string | null): Promise<void> {
  if (indexRequest) void api.cancelKeyframes(indexRequest).catch(() => {});
  smart.index = null;
  smart.indexError = "";
  if (!path) {
    indexRequest = null;
    smart.loadingIndex = false;
    return;
  }
  const id = `k${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  indexRequest = id;
  smart.loadingIndex = true;
  try {
    const index = await api.keyframes(id, path);
    if (indexRequest === id) smart.index = index;
  } catch (e) {
    if (indexRequest === id) smart.indexError = errorText(e);
  } finally {
    if (indexRequest === id) {
      smart.loadingIndex = false;
      indexRequest = null;
    }
  }
}

let planKey = "";
let planTimer: ReturnType<typeof setTimeout> | undefined;
/** Bumped by every change and stop: an answer from an older call is never shown. */
let planSeq = 0;

/** Keeps the smart plan in step with the source, range and options (250 ms debounce). */
export function syncPlan(path: string, range: RangeArg, options: PartialOptions): void {
  const key = JSON.stringify([path, range, options]);
  if (key === planKey) return;
  planKey = key;
  planSeq++;
  // The old range's parts would be drawn over the new range until the answer comes.
  smart.summary = null;
  smart.notices = [];
  smart.planError = "";
  clearTimeout(planTimer);
  planTimer = setTimeout(() => void plan(path, range, options), 250);
}

/** Stops following the range (smart not selected, no range, or another view). */
export function stopPlan(): void {
  clearTimeout(planTimer);
  planKey = "";
  planSeq++;
  smart.summary = null;
  smart.notices = [];
  smart.planning = false;
  smart.planError = "";
}

async function plan(path: string, range: RangeArg, options: PartialOptions): Promise<void> {
  const seq = planSeq;
  smart.planning = true;
  try {
    const r = await api.smartPlan(path, range, options);
    if (seq !== planSeq) return;
    smart.summary = r.summary;
    smart.notices = r.notices;
    smart.planError = "";
  } catch (e) {
    if (seq !== planSeq) return;
    smart.summary = null;
    smart.notices = [];
    smart.planError = errorText(e);
  } finally {
    if (seq === planSeq) smart.planning = false;
  }
}
