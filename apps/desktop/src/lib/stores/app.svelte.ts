// Shared UI state (Svelte 5 runes). Logic lives in pure modules (queue.ts, run.ts).
import type { DoctorReport } from "../bindings/DoctorReport";
import type { MediaInfo } from "../bindings/MediaInfo";
import type { Settings } from "../bindings/Settings";
import type { Manifest } from "../manifest";
import { emptyQueue } from "./queue";
import type { RunView } from "./run";

export interface SourceItem {
  path: string;
  info?: MediaInfo;
  error?: string;
}

export const app = $state({
  sources: [] as SourceItem[],
  selected: null as string | null,
  queue: emptyQueue() as Manifest,
  engine: null as DoctorReport | null,
  settings: null as Settings | null,
  run: null as RunView | null,
  runIssues: [] as string[],
  showSettings: false,
  /** The centre pane: cutting clips, "Upscale for phone", or "GIF for Reddit". */
  view: "clip" as "clip" | "upscale" | "gif",
  toast: null as { kind: "info" | "error"; text: string } | null,
});

let toastTimer: ReturnType<typeof setTimeout> | undefined;
/** Info toasts fade; errors stay until dismissed. */
export function toast(text: string, kind: "info" | "error" = "info") {
  app.toast = { kind, text };
  clearTimeout(toastTimer);
  if (kind === "info") toastTimer = setTimeout(() => (app.toast = null), 3500);
}

export function infoFor(path: string): MediaInfo | undefined {
  return app.sources.find((s) => s.path === path)?.info;
}
