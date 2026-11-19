# Data Model: Budgeted Reddit Video (005)

The entities for [spec.md](./spec.md) › Key Entities, with the rules from
[research.md](./research.md). Code lives in a new core module, `crates/rmedia-core/src/fit/`.

## VideoTarget (`fit/target.rs`)

The data file for a destination's limits. It is built in (`reddit-video`), or a user file at
`<config>/video-targets/<id>.json` ([contract](./contracts/video-target.md)).

| Field | Type | Rule |
|---|---|---|
| `schema` | u32 | `1` |
| `id` | string | `[a-z0-9-]{1,40}`; a user file with a built-in's id replaces it |
| `version` | u32 | ≥ 1 |
| `name`, `description` | string | printable, at most 300 characters (003's `unsafe_char` check) |
| `verified` | bool | `false` until the owner confirms the limits (FR-003) |
| `max_bytes` | u64 | 64 KiB ≤ x ≤ 4 GiB |
| `max_duration` | f64 seconds | 0 < x ≤ 14,400 |
| `long_side` | u32 | 256 ≤ x ≤ 3840, even |
| `floor_short_side` | u32 | 144 ≤ x, and below the short side of `long_side` at 16:9 |
| `max_fps` | u32 | 1 ≤ x ≤ 120 |
| `quality.crf` | u8 | 0 ≤ x ≤ 51 |
| `quality.max_bpp` | f64 | 0 < `min_bpp` < `max_bpp` ≤ 1 |
| `quality.min_bpp` | f64 | see `max_bpp` |
| `audio.kbps` | u32 | 32 ≤ `floor_kbps` ≤ `kbps` ≤ 320 |
| `audio.floor_kbps` | u32 | see `audio.kbps` |

Unknown fields are refused, as for GIF targets.

**`VideoTargetRegistry`**: the built-ins plus the user files, in a stable order. `problems()`
is shown by `doctor`. The loader is shared with `GifTargetRegistry` (`data_files::load<T>`, R10).

## FitOptions (`fit/mod.rs`, layered in `request.rs`)

What the user asks for, layered like GIF options (defaults → video → clip → CLI):

| Field | Type | Default | Notes |
|---|---|---|---|
| `enabled` | bool | false | `false` at a layer turns an inherited fit off |
| `target` | string | `reddit-video` | |
| `max_size` | u64? | target's | may only lower the target's `max_bytes`; parsed as `50MB` or `1GiB` |
| `max_length` | Ts? | target's | may only lower `max_duration` |
| `allow_trim` | bool | false | R4, R5 |

`PartialFit` is the layer form; its `diff`/`apply` work as `PartialGif` does, so a manifest
written by the desktop app reads back unchanged (FR-018).

## FitPlan (`fit/plan.rs`)

The planner's result for one job (spec › Budget plan). It's pure, made from the target, the
options, the range length, the planned picture size and frame rate, and whether there is audio.

| Field | Type | Meaning |
|---|---|---|
| `target` | VideoTarget | copied in (self-contained job) |
| `cap` | u64 | effective `max_bytes` (after `max_size`) |
| `max_length` | Ts | effective limit |
| `len` | Ts | the range length after any trim |
| `trimmed` | Option<Ts> | the original length, when trimmed |
| `size` | (u32, u32) | output size after the ladder (even) |
| `fps` | (u32, u32) | the source rate, capped at `max_fps` |
| `mode` | `Budget` / `Quality` | R1 and R3 |
| `video_kbps` | u32 | `B` in budget mode, or the ceiling `C` in quality mode |
| `crf` | Option<u8> | quality mode only |
| `audio_kbps` | Option<u32> | `None` when the output has no audio |
| `predicted_bytes` | u64 | R2 arithmetic |
| `lowered` | Vec<Lowered> | `{setting: "size" \| "audio_kbps", from, to}` |

**Planning order** (each step can only lower values):
1. Trim to `max_length`, or refuse with `RANGE_TOO_LONG`.
2. Fit the size to `long_side`, never above the source.
3. Cap the frame rate.
4. Compute `B` (R2).
5. Choose quality or budget mode (R3).
6. Run the ladder (R4).
7. Lower the audio.
8. Trim, or refuse with `BUDGET_UNREACHABLE`.

**Invariant:** `predicted_bytes ≤ cap × (1 − margin)` for every plan that isn't refused.

## FitOutcome (`fit/mod.rs`, in the job outcome)

What the run measured (spec › Fit result):

| Field | Type |
|---|---|
| `bytes` | u64, the committed size; always ≤ `cap` |
| `cap` | u64 |
| `mode` | `budget` / `quality` |
| `video_kbps`, `audio_kbps` | u32, u32?, the last attempt's |
| `attempts` | Vec<{video_kbps, bytes}>, 1 to 4 entries |
| `trimmed` | bool |
| `size`, `fps` | the output's |

## Changes to existing entities

| Entity | Change |
|---|---|
| `ClipSpec` (`plan/ffargs.rs`) | `fit: Option<FitPlan>`; with a fit, `mode` is `Accurate` and the container is `mp4` |
| `UpscaleSpec` (`upscale/mod.rs`) | `fit: Option<FitPlan>`; the preset's `limits` and `maxrate_for_limit` are replaced by the fit |
| `PartialOptions` / `Options` (`request.rs`) | `fit: Option<PartialFit>` / `Option<FitOptions>` |
| `PlannedJob` JSON | a `fit` block (dry run) ([contract](./contracts/json-output.md)) |
| `JobOutcome::Succeeded` JSON | `fit` (the `FitOutcome`) |
| `IssueCode` | new: `RANGE_TOO_LONG` (error, exit 3), `FIT_LOWERED` and `FIT_TRIMMED` (notices), `FIT_REENCODES` (notice: `--mode fast` with `--fit`), `FIT_FAILED` (run-time, exit 1). Reused: `TARGET_UNKNOWN`, `BUDGET_UNREACHABLE`, `SMART_NOT_APPLIED` |
| `Error` | `FitFailed { cap, attempts }` → `FIT_FAILED` |
| `DoctorReport` | `fit: {targets: [...], problems: [...]}` |
| `Caps` | `video_targets: VideoTargetRegistry`, loaded by the caller like the GIF targets |

## State

A fit job runs like this:

```text
plan → (refused: RANGE_TOO_LONG | BUDGET_UNREACHABLE | TARGET_UNKNOWN)
     → planned { mode, video_kbps, predicted }
run  → budget: pass1 → pass2 → measure ─┬─ ≤ cap → verify → commit (FitOutcome)
                                        └─ > cap → pass2 at B' (≤ 3 retries) → FIT_FAILED
       quality: one pass → measure ─ (> cap: budget mode at the measured size, counts as a retry)
cancel at any point → scratch and .part removed (existing)
```
