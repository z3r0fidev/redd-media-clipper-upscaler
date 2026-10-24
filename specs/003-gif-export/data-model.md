# Data Model: GIF Export for Reddit

This extends `specs/001-video-clipping/data-model.md` and `specs/002-mobile-upscale/data-model.md`.
Types live in `rmedia-core`. Types marked (ts) are exported to the desktop app through ts-rs.

## Refactors carried in from 001/002 (B1–B4)

| Ref | Change |
|---|---|
| B1 | A new `Flavor::Gif` in `upscale::graph::build_with`, exposed as `pub fn frame_chain(input: &GraphInput) -> String` (research R1). The geometry is planned straight at the GIF size with `W = min(target, crop width)`, so there is one resample with the swscale lanczos scaler. It reuses `pre_chain_at` (trim, deinterlace, square pixels, tone-map, and the `fps_cap` slot with `F = min(target, source fps)`), plus crop or blur-pad and the alpha flatten. It skips denoise, CAS, zscale colour conversion and the `setsar,format` tail, and ends in an open label. `build()` and its insta snapshots are unchanged. |
| B2 | `reframe::analyze_cached` and `lookup_cached` take `&ReframeInput`. It carries the `cut` (for `input_args`), the `GraphInput` pieces `analysis_graph` reads, the detector, the cache directory, `analysis_size`, `fps_out`, the crop size and `cmd_offset` (research R16). `UpscaleSpec` and `GifSpec` each build one. The cache key comes from the same `analysis_graph` string, and a test asserts it is unchanged for a 002 job. |
| B3 | A rewrite of `exec::pipeline`: `Stages` holds `Vec<StageSpec { program, args, role: Producer \| Filter \| Consumer, progress }>` (research R7). It covers the progress source (the consumer's stdout, or a `progress.log` the producer writes), the blame rule (a later stage's failure beats a producer EPIPE, and a producer failure discards the consumer output), and cancel order (Linux process group; Windows a Job Object per child, killed consumer-first). The AI tier is a 3-stage instance and ffmpeg → gifski a 2-stage one. `us6_pipeline.rs` and the blame unit tests stay green. |
| B4 | JSON compatibility (research R16): `PlannedJob.warnings` stays `[string]`, and GIF jobs add `notices: [{code, message}]`. `Event::JobWarning` gains an optional `code`, and `ErrorInfo` an optional `detail`. `Error` gains `BudgetUnreachable { fits_secs, best_bytes, min_bytes }`. `Error::EngineFailed` gains `tool`, defaulting to `"ffmpeg"`. `PlannedJob::commands()` lists every process of a job; `args()` keeps returning the final encode. |

## Probe additions

`StreamInfo` gains:
- `r_frame_rate: Option<Rational>`;
- `nb_frames: Option<u64>`;
- `time_base: Option<Rational>`;
- `has_alpha: bool` (pix_fmt has an alpha plane: `yuva*`, `rgba`, `argb`, `bgra`, `pal8` with
  transparency, `ya*`).

It also gains the helper `is_vfr()`: `r_frame_rate` and `avg_frame_rate` differ by more than 1%.

## GifTarget (ts)

A named, versioned preset. It follows [contracts/gif-target.schema.json](./contracts/gif-target.schema.json).

| Field | Type | Rule |
|---|---|---|
| `schema` | 1 | |
| `id` | kebab-case, ≤ 40 | unique; a user file overrides a built-in |
| `version` | int ≥ 1 | |
| `name`, `description` | text | |
| `budget` | `{ soft: u64, hard: u64 }`? | `soft ≤ hard`; omitted means no budget search |
| `verified` | bool | `false` on shipped Reddit targets (FR-007) |
| `width` | `{ landscape: u32, portrait: u32 }` | 64..=1920 |
| `floor_width` | u32? | ≤ both widths; omitted means width is not lowered |
| `floor_fps` | u8? | ≤ `fps`; omitted means fps is not lowered |
| `fps` | u8 | 1..=50 |
| `colours` | u16 | 2..=256 |
| `dither` | `Dither` | see below |
| `stats_mode` | `full` \| `diff` | |
| `sharpen` | f32? | 0 < x ≤ 1.5, the `unsharp` luma amount |
| `engine` | `builtin` \| `best` | `best` means gifski when available |
| `lossy` | u16? | the default gifsicle lossy level (0..=200); omitted means 0; the ladder starts here |
| `gifski` | `{ quality, motion_quality?, lossy_quality? }`? | 1..=100 each; used by the best-quality engine (research R14) |
| `max_duration` | f64? | seconds; above it, a warning before encoding |
| `mp4` | `{ crf: u8, max_fps: u8 }` | Reddit MP4 defaults; crf 0..=51, fps 1..=60 |

`Dither` is `none`, `bayer { scale: 0..=5 }`, `floyd_steinberg` or `sierra2_4a`. Its ladder
order is fixed: error-diffusion → bayer 4 → bayer 5. `none` is not lowered further.

`GifTargetRegistry::load(config_dir) -> (registry, problems)` copies `PresetRegistry`. Untrusted-file
rules (size, count, control characters, cross-field checks, override labels) are in research R13.

## GifOptions / PartialGif (ts)

These layer like `UpscaleOptions` (defaults → source → clip). Every field is optional in
`PartialGif`; `enabled: false` turns GIF output off for that layer.

| Field | Type | Default |
|---|---|---|
| `target` | target id | `reddit-post` |
| `format` | `gif` \| `mp4` | `gif` |
| `engine` | `auto` \| `builtin` \| `gifski` | `auto`, meaning the target's engine |
| `strict_engine` | bool | false |
| `optimise` | bool | true; gifsicle is used when present |
| `loop_count` (JSON key `loop`) | `forever` \| `N` (1..=65535 plays) | `forever` |
| `shape` | `source` \| `9:16` \| `4:5` \| `1:1` | `source` |
| `framing` | `smart` \| `center` \| `blur` | `smart` (ignored for `source`) |
| `width`, `fps`, `colours`, `dither`, `lossy` | overrides | target values (ignored with a note under gifski) |
| `budget` | `{ soft?, hard? }` \| `none` | target budget; `soft` alone sets `hard = soft` (research R5) |
| `allow_trim` | bool | false |

A `PartialOptions.gif: Option<PartialGif>` field is added. When `gif` and `upscale` are both on,
the job is chained. The upscale preset contributes the shape and reframe kind, and the frame
chain applies 002's pre-processing and reframe straight at the GIF size; no upscale tier runs
(research R16, FR-014, FR-016).

**Validation**, in the planner:
- the target is known (`TARGET_UNKNOWN`, a new code that mirrors `PRESET_UNKNOWN`);
- the overrides are within the target schema ranges;
- `budget.soft ≤ budget.hard`;
- the range covers at least 2 frames at the chosen fps;
- a static floor: a budget below 64 KiB is rejected (`BUDGET_UNREACHABLE`, `min_bytes`), unless
  `allow_trim` is set. All other unreachable budgets are found by the search's floor sample,
  before the full encode (research R5). The planner never encodes.

## GifPlan

Built by `plan_gif`, carried in `GifSpec { cut: ClipSpec, frame: FramePlan, gif: GifPlan }`.

| Field | Meaning |
|---|---|
| `target` | a resolved copy of the `GifTarget` |
| `format`, `engine` (resolved), `engine_reason` | e.g. "gifski requested but not found: built-in" |
| `optimiser` | `Option<Tool>`: gifsicle when it is found and `optimise` is on |
| `start` | `Settings { width, height, fps, timing: { exact: bool, avg_cs: f32 }, colours, dither, lossy, gifski?: {…} }` |
| `floors` | `floor_width`, `floor_fps` (absent means not lowered) |
| `budget` | `Option<Budget { soft, hard }>` |
| `loop_count`, `allow_trim` | |
| `estimate` | `Estimate { bytes: Option<u64>, from: search \| sample \| pending, settings: Settings, lowered: [...], mp4_bytes: Option<u64> }`: filled by the prepare step (dry run, desktop), `pending` at plan time (research R4) |
| `notes` | e.g. "source narrower than 720 px: kept 640", "--colors is ignored by gifski", "upscale skipped: GIF width is below the source" |

`FramePlan` reuses 002's `GraphInput` pieces: geometry for the shape, the reframe kind, tone-map
and trim. `frame_chain` (B1) renders it.

## SearchRecord (ts)

| Field | Type |
|---|---|
| `steps` | `[{ n, kind: sample \| post \| full, settings: Settings, bytes, estimate: u64 }]`; `post` = a gifsicle re-run on a sample, not an encode |
| `chosen` | `Settings` |
| `lowered` | `[{ setting: lossy \| dither \| colours \| fps \| width, from, to }]` |
| `encodes` | u8: samples + full encodes (≤ 9 by design, asserted ≤ 10) |
| `fit` | `soft` \| `hard` \| `none` (no budget) |

## GifEngine status (ts, in `DoctorReport`)

`EngineStatus { name: gifsicle | gifski, path: Option<PathBuf>, source: config | bundled | path,
version: Option<String>, ok: bool, reason: Option<String> }`

`DoctorReport.gif`:
`{ gif: bool, mp4: bool, missing: [filter or encoder names], engines: [EngineStatus], target_problems: [...] }`

## JobSpec / JobMeasure

- `JobSpec::Gif(Box<GifSpec>)`. `kind()` is `"gif"`. Its resource is CPU. Scratch is always
  allocated.
- `JobMeasure::Gif`:
  `{ span, format, bytes, width, height, fps, timing, colours, dither, lossy, engine, optimiser:
  bool, loop_count, deterministic: bool, search: Option<SearchRecord>, trimmed: Option<{ from: Span, to: Span }>,
  mp4_estimate: Option<u64> }`. `deterministic` is false only for an engine that failed the Phase 0
  check (research R14).
  It is serialised with `"measure":"gif"` (additive).

## Stages and weights

| Case | Stages (weight) |
|---|---|
| no budget, built-in | palette 0.25 · encode 0.6 · optimise 0.1 (or nothing) · verify 0.05 |
| budget | analyze 0.15 (smart only) · search 0.3 · palette 0.1 · encode 0.35 · optimise 0.05 · verify 0.05 |
| gifski | analyze (smart) · search (if budget; y4m samples through gifski) · encode, a 2-stage pipe · verify |
| mp4 | analyze (smart) · search (if budget) · encode · verify |

Weights are renormalised to sum to 1 when stages are absent. A retry of the full encode uses
`restart_stage`.

## Issue codes (additive)

| Code | Kind | When |
|---|---|---|
| `TARGET_UNKNOWN` | error (exit 3) | the target id is not in the registry |
| `BUDGET_UNREACHABLE` | error (exit 1 per job; exit 3 at plan time for the static 64 KiB floor) | the floor sample, or the retried full encode, is over the hard limit and trimming is not allowed. `detail: { fits_secs, best_bytes }` or `{ min_bytes }` |
| `OVER_RECOMMENDED` | warning | the result is over the soft budget and under the hard one |
| `ENGINE_FALLBACK` | warning | gifski was requested or the target wants it, or the search wanted a lossy rung, and the engine is unavailable (research R7) |
| `TIMING_APPROXIMATE` | notice | the rate is not exact in GIF (FR-003) |
| `LONG_RANGE` | warning | the range is longer than the target's `max_duration` |

Every warning and notice appears twice in the dry run: as a plain string in `warnings` (001/002
consumers) and as `{code, message}` in `notices`. `JobWarning` events carry the `code`.

`ENGINE_MISSING` (existing) is used for `--strict-engine`, and when the gif muxer, palettegen or
paletteuse (GIF) or libx264 (MP4) is missing.

## Settings additions

`gifsicle_path`, `gifski_path` (`Option<PathBuf>`) and `gif_defaults: Option<PartialGif>`, all
`serde(default)` so older files still load. The environment variables are `RMEDIA_GIFSICLE` and
`RMEDIA_GIFSKI`.
