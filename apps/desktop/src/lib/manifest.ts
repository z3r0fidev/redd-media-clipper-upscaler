// The queue is a v1 batch manifest (contracts/manifest.schema.json): the same document the CLI's
// `rmedia batch` runs, so "Export" is lossless and CLI/app results are identical.
import type { PartialOptions } from "./bindings/PartialOptions";

export type TimeValue = number | string;

export interface ClipEntry {
  start: TimeValue;
  end?: TimeValue;
  duration?: TimeValue;
  name?: string;
  options?: PartialOptions;
}

export interface VideoEntry {
  input: string;
  options?: PartialOptions;
  clips: ClipEntry[];
}

export interface Manifest {
  version: 1;
  defaults?: PartialOptions;
  videos: VideoEntry[];
}
