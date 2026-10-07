# Data Model: Mobile Upscaling & Smart Reframe

This document extends `specs/001-video-clipping/data-model.md`. Types live in `rmedia-core`. Types
marked (ts) are exported to the desktop app via ts-rs.

## Refactors carried in from 001 (A1–A5)

| Ref | Change |
|---|---|
| A1 | `PlannedJob` drops `mode`, `requested` and `predicted`, which move into `ClipSpec`. A new `PlannedJob.media_len: Ts` replaces `media_len()`. `JobOutcome::Succeeded { output, measure: JobMeasure }`, where `JobMeasure` is `Clip { actual: Span, adjusted: bool }` or `Upscale { span: Span, tier_used: Tier, tier_fallback: bool, fallback_ranges: Vec<Span>, no_upscale_needed: bool }`. The JSON keeps 001's `actual` and `adjusted` keys for clip jobs (flattened), so the NDJSON contract does not change. |
| A2 | `JobCtx::scratch_dir()` is a per-job folder next to the temp output (`.<run_id>-<id>.d/`). It is journaled and removed at the end. `JobCtx::spawn_*` helpers run several processes in one job. |
| A3 | Each job has stage weights, reported with `JobCtx::stage(i, weight)`. The scheduler holds a `Semaphore` per `Resource` (`Cpu` = jobs, `Gpu` = 1). |
| A4 | `Caps { encoders, filters, models: ModelSet, capability: Option<CapabilityRecord> }`. Issue codes become `IssueCode`, an enum serialised as the existing strings plus `TIER_UNAVAILABLE`, `PRESET_UNKNOWN`, `HDR_UNSUPPORTED` and `LIMIT_EXCEEDED` (a warning). |
| A5 | `Planner::claim_output(...) -> Result<(PathBuf, Disposition)>` is extracted from `plan_clip`. Clip and upscale jobs share it. |

## Probe additions

`VideoStream` gains the following fields:
- `rotation: i32`, in degrees, from the display matrix;
- `sar: (u32, u32)`;
- `field_order: Option<String>`;
- `color_space`, `color_primaries`, `color_transfer`, `color_range`, each `Option<String>`.

It also gains these helpers:
- `display_size() -> (u32, u32)`: applies rotation and SAR, rounded to even numbers;
- `is_hdr()`: true when the transfer is `smpte2084` or `arib-std-b67`;
- `is_interlaced()`.

## Preset (ts)

The file format is defined in [contracts/preset.schema.json](./contracts/preset.schema.json).

```text
Preset {
  id: String                     // kebab-case, unique; user files override built-ins by id
  version: u32
  name: String                   // "Phone fill"
  description: String
  devices: Vec<String>           // device classes shown to users (FR-002)
  width: u32, height: u32        // even, 144..=4320
  max_fps: u32                   // ≤ 60 for phone presets
  video: VideoFormat { codec: Hevc|H264, crf: u8, preset: String,
                       profile: String, level: Option<String>, tag: Option<String> /* "hvc1" */ }
  audio: AudioFormat { codec: Aac, bitrate_k: u32, copy_if_aac: bool }
  hdr_capable: bool
  limits: Option<Limits { max_bytes: Option<u64>, max_duration: Option<Ts> }>
  est_bpp: f32                   // size-estimate constant
}
```

**PresetRegistry:**
- Built-ins come first, then user presets loaded from `<config>/presets/*.json`.
- Invalid user files are reported as `doctor` problems and skipped. They are never fatal.
- `get(id)`, `list()`, `default_id() = "phone-fill"`.

**Validation:**
- Even dimensions.
- `crf` within the codec's range.
- `tag: "hvc1"` only with HEVC.
- `hdr_capable` only with HEVC.

## UpscaleOptions / PartialUpscale (ts)

```text
UpscaleOptions {
  preset: String                 // default "phone-fill"
  reframe: Reframe               // smart (default) | center | blur
  tier: TierChoice               // auto (default) | cpu | gpu | ai
  hdr: HdrMode                   // sdr (default) | keep
  motion: Motion                 // calm | normal (default) | fast
  crf: Option<u8>                // overrides preset.video.crf
}
Options.upscale: Option<UpscaleOptions>          // None = upscaling off (001 behaviour)

PartialUpscale { enabled: Option<bool>, preset, reframe, tier, hdr, motion, crf: Option<_> }
PartialOptions.upscale: Option<PartialUpscale>   // deny_unknown_fields
```

**Layering:**
- Layers apply in order defaults < source < clip, field by field.
- `enabled: Some(true)` turns upscaling on, using `UpscaleOptions::default()` for any unset field.
- `enabled: Some(false)` turns it off.
- Setting any other field implies `enabled: true`.
- `diff` emits only the nested fields that changed. This keeps desktop export round-trips exact
  (FR-022).

**Interaction with 001 options when upscaling is on:**
- `mode` is ignored, because the job always re-encodes.
- `container` is forced to `mp4`, and a warning is given if the user set another one.
- `name_template` defaults:
  - whole-file inputs: `{source}_{preset}.{ext}`;
  - clips: `{source}_{index:03}_{name-or-range}_{preset}.{ext}`.
- `output_dir` defaults:
  - whole-file inputs: `<source dir>/upscaled`;
  - clips: `<source dir>/clips`.

## Tier, TierChoice (ts)

- `Tier`: `Cpu`, `Gpu` or `Ai`.
- `TierChoice`: `Auto` or `Forced(Tier)`. It serialises as `"auto" | "cpu" | "gpu" | "ai"`.
- `tier::select(choice, &CapabilityRecord, &MediaInfo)` returns
  `Result<TierDecision { tier, reason: String, suggest: Option<Tier> }, IssueCode>`. The rules are
  in research R9.

## CapabilityRecord (ts)

The file format is defined in
[contracts/capability-record.schema.json](./contracts/capability-record.schema.json).

```text
CapabilityRecord {
  schema: 1, fingerprint: String, checked_at: String (RFC 3339)
  devices: Vec<GpuDevice { index, name, kind: Integrated|Discrete|Virtual|Cpu|Software,
                           device_id: String, usable: bool, reason: Option<String> }>
  tiers: TierStatus { cpu: TierInfo, gpu: TierInfo, ai: TierInfo }
  demoted: Vec<Demotion { tier: Tier, error: String, at: String }>
}
TierInfo { available: bool, reason: Option<String>, device: Option<u32>,
           fps: Option<f32>           // measured at the default preset size
           sec_per_mpx: Option<f32> } // AI only
```

**Lifecycle:**
1. `load()` reads the cached record.
2. `fingerprint()` runs the cheap listing.
3. If the fingerprint differs, or a recheck was requested, `self_test()` runs and `save()` writes the
   new record atomically.
4. `demote(tier, err)` is called after an automatic fallback.

A record whose fingerprint differs is treated as stale. When a fresh check replaces it, its
demotions are cleared.

## ReframeAnalysis (ts)

```text
ReframeAnalysis {
  key: String                         // cache key (research R10)
  algo_version: u32
  source_size: (u32, u32)             // displayed size, after pre-processing
  crop: (u32, u32)                    // crop window W×H in source pixels
  axis: X                             // v1 pans horizontally only
  fps: f64                            // output frame grid the path is sampled on
  shots: Vec<Shot { span: Span, kind: Tracked|Fallback|Mixed }>
  fallback_ranges: Vec<Span>          // FR-014, ascending, non-overlapping
  path: Vec<i32>                      // crop x per output frame (even, clamped)
  detections: u32, samples: u32       // statistics for reports
}
```

**Derived views:**
- `keyframes(hz)` returns `[(t, x)]` resampled at 10 Hz, for JSON and the desktop overlay.
- `to_sendcmd()` returns the command-file text (research R5).

**Invariants:**
- The path length equals the output frame count.
- Within a shot, the step between consecutive frames is at most the speed limit, and the change in
  step is at most the acceleration limit (research R6).
- `fallback_ranges` lie within the clip span.

## UpscalePlan / JobSpec::Upscale

```text
JobSpec::Upscale(UpscaleSpec { cut: ClipSpec /* accurate mode */, up: UpscalePlan })

UpscalePlan {
  preset: Preset (resolved copy)      // the plan is self-contained
  target: (u32, u32)
  geometry: Geometry { display: (u32,u32), crop: Option<(u32,u32)>, fit: (u32,u32),
                       overflow: None|Horizontal|Vertical, no_upscale_needed: bool }
  reframe: Reframe, motion: Motion
  analysis: AnalysisState             // NotNeeded | Pending(key) | Ready(ReframeAnalysis)
  tier: TierDecision
  hdr: HdrMode, colour_in: ColourTags
  fps_cap: Option<u32>
  estimate: Estimate { bytes: u64, secs: f64 }
  limit_warnings: Vec<String>
}
```

**Argv:** `PlannedJob::args()` for an upscale job returns the render argv, with `reframe.cmd`
relative to the scratch directory. Dry-run JSON shows it, with `"cwd": "<scratch>"`.

**Executor stages** (`exec/upscale.rs`):
1. Analysis, when the plan says `Pending`.
2. Render. Writes `reframe.cmd` to the scratch dir, then runs ffmpeg with that cwd.
3. Verify with ffprobe:
   - size equals the target;
   - codec and tag match the preset;
   - colour tags are bt709 (or the HDR tags);
   - duration within 1 frame + 50 ms of the span.

   If verification passes, the output is committed.

**Fallback:** an automatic `gpu` job whose render fails with `ENGINE_FAILED` goes back to stage 2
once with `Tier::Cpu`.

## Event additions (ts)

- `Event::JobStage { job_id, stage: "analyze" | "render" | "verify", index, of }` is new.
  Consumers that don't know it ignore it, so the 001 NDJSON contract stays additive.
- `Event::AnalysisProgress` is not added. Analysis progress uses `JobProgress` with the stage
  weighting.
- `JobFinished.outcome` carries `JobMeasure::Upscale` (A1).

## Settings additions (ts)

- `Settings.models_dir: Option<PathBuf>`
- `Settings.upscale_defaults: PartialUpscale`, the desktop's last-used preset, reframe and tier.
  These are separate from the clip `defaults` so that clip runs never turn on upscaling by accident.
