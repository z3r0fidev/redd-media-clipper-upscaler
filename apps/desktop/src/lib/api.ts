// Typed wrappers over the Tauri commands (contracts/tauri-commands.md). Types in ./bindings are
// generated from rmedia-core by `cargo xtask gen-bindings`.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DoctorReport } from "./bindings/DoctorReport";
import type { Event } from "./bindings/Event";
import type { MediaInfo } from "./bindings/MediaInfo";
import type { Plan } from "./bindings/Plan";
import type { Preset } from "./bindings/Preset";
import type { ProxyCodec } from "./bindings/ProxyCodec";
import type { RunSummary } from "./bindings/RunSummary";
import type { ScanEvent } from "./bindings/ScanEvent";
import type { ScanOptions } from "./bindings/ScanOptions";
import type { ScanResult } from "./bindings/ScanResult";
import type { ReframeAnalysis } from "./bindings/ReframeAnalysis";
import type { Settings } from "./bindings/Settings";
import type { UpscaleOptions } from "./bindings/UpscaleOptions";
import type { Manifest } from "./manifest";

export interface PreviewInfo {
  info: MediaInfo;
  direct_url: string | null;
  time_offset: number;
}

export type PickKind = "video" | "cliplist" | "manifest";

/** A clip range in seconds of source time (`end: null` = to the end). */
export interface RangeArg {
  start: number;
  end: number | null;
}

export interface PresetList {
  default: string;
  presets: Preset[];
  problems: { file: string; message: string }[];
}

/** The analysis behind the crop overlay, or why there is none (e.g. no face model). */
export interface ReframeResult {
  analysis: ReframeAnalysis | null;
  note: string | null;
}

export type AnalysisEvent = { type: "progress"; fraction: number } | { type: "done" };

/** Before/after stills as local media-server URLs. */
export interface UpscalePreview {
  before: string;
  after: string;
  size: [number, number];
  approximate: boolean;
  crop_x: number | null;
  in_fallback: boolean;
}

export const api = {
  doctor: () => invoke<DoctorReport>("doctor"),
  doctorRecheck: () => invoke<DoctorReport>("doctor_recheck"),
  takeOpened: () => invoke<string[]>("take_opened"),
  pickFiles: (kind: PickKind) => invoke<string[]>("pick_files", { kind }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  pickSavePath: (defaultName: string) => invoke<string | null>("pick_save_path", { defaultName }),
  probe: (path: string) => invoke<MediaInfo>("probe", { path }),
  scan(scanId: string, roots: string[], options: Partial<ScanOptions>, onEvent: (e: ScanEvent) => void) {
    const ch = new Channel<ScanEvent>();
    ch.onmessage = onEvent;
    const full: ScanOptions = {
      no_recursive: false,
      include: [],
      exclude: [],
      hidden: false,
      follow_symlinks: false,
      exclude_dir_names: [],
      ...options,
    };
    return invoke<ScanResult>("scan", { scanId, roots, options: full, onEvent: ch });
  },
  cancelScan: (scanId: string) => invoke<void>("cancel_scan", { scanId }),
  parseTime: (text: string) => invoke<number>("parse_time", { text }),
  parseEnd: (text: string) => invoke<number | string>("parse_end", { text }),
  loadCliplist: (path: string, sources: string[]) => invoke<Manifest>("load_cliplist", { path, sources }),
  loadManifest: (path: string) => invoke<Manifest>("load_manifest", { path }),
  exportManifest: (manifest: Manifest, path: string) => invoke<void>("export_manifest", { manifest, path }),
  plan: (manifest: Manifest) => invoke<Plan>("plan", { manifest }),
  run(runId: string, onEvent: (e: Event) => void) {
    const ch = new Channel<Event>();
    ch.onmessage = onEvent;
    return invoke<RunSummary>("run", { runId, onEvent: ch });
  },
  cancelRun: (runId: string, jobId?: number) => invoke<void>("cancel_run", { runId, jobId: jobId ?? null }),
  previewSource: (path: string) => invoke<PreviewInfo>("preview_source", { path }),
  ensureProxy(path: string, codec: ProxyCodec, onProgress: (f: number) => void) {
    const ch = new Channel<number>();
    ch.onmessage = onProgress;
    return invoke<string>("ensure_proxy", { path, codec, onProgress: ch });
  },
  async frameAt(path: string, t: number, height: number): Promise<Blob> {
    const bytes = await invoke<ArrayBuffer>("frame_at", { path, t, height });
    return new Blob([bytes], { type: "image/jpeg" });
  },
  thumbnails: (path: string, n: number) => invoke<string[]>("thumbnails", { path, n }),
  /** Choose ffmpeg with the native dialog (reset = back to automatic). */
  pickEngine: (reset: boolean) => invoke<DoctorReport>("pick_engine", { reset }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  listPresets: () => invoke<PresetList>("list_presets"),
  analyzeReframe(analysisId: string, path: string, span: RangeArg | null, options: UpscaleOptions, onEvent: (e: AnalysisEvent) => void) {
    const ch = new Channel<AnalysisEvent>();
    ch.onmessage = onEvent;
    return invoke<ReframeResult>("analyze_reframe", { analysisId, path, span, options, onEvent: ch });
  },
  cancelAnalysis: (analysisId: string) => invoke<void>("cancel_analysis", { analysisId }),
  upscalePreview: (path: string, span: RangeArg | null, at: number, options: UpscaleOptions) =>
    invoke<UpscalePreview>("upscale_preview", { path, span, at, options }),
  /** Choose the models folder with the native dialog (reset = back to the default search). */
  pickModelsDir: (reset: boolean) => invoke<Settings>("pick_models_dir", { reset }),
  onDropped: (f: (paths: string[]) => void): Promise<UnlistenFn> =>
    listen<string[]>("rmedia://dropped", (e) => f(e.payload)),
};

/** Human message for a rejected command. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
