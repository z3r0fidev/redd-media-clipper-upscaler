# Data Model: Smart Cut

**Feature**: [spec.md](./spec.md) | **Research**: [research.md](./research.md)

All types live in `rmedia-core`. Additions to existing types are additive: 0.3.0 JSON, manifests
and settings keep their meaning (SC-009). Times are `Ts` in the user time base (001), unless
marked absolute (source pts).

## Mode (changed, `request.rs`)

```text
enum Mode { Fast (default), Accurate, Smart }     // serde: "fast" | "accurate" | "smart"
```

- Accepted wherever `Mode` is accepted today: `--mode`, `Settings.defaults.mode`, manifest JSON
  `mode` (run, video and clip `options`), the CSV `mode` column, and the desktop queue.
- Layering is unchanged: clip over video over run over settings.
- A job that also upscales or makes a GIF or Reddit MP4 resolves to `Accurate`, with the notice
  `SMART_NOT_APPLIED` (FR-013).

## Options.smart_fallback (new field, `request.rs`)

```text
smart_fallback: bool   // default true; PartialOptions: Option<bool>
```

- `false` is strict mode (FR-009): a job that cannot be smart-cut fails and does not fall back.
- It has no effect unless the resolved mode is `Smart`.
- Surfaces: CLI `--no-smart-fallback`; JSON `options.smart_fallback`; CSV column `smart_fallback`
  (`true`/`false`/empty); desktop checkbox "Only smart (no fallback)".

## KeyframeIndex (new, `smart/keyframes.rs`)

| Field | Type | Notes |
|---|---|---|
| `window` | `Option<Span>` | `None`: the whole file |
| `keyframes` | `Vec<KeyframeEntry>` | sorted by pts |
| `frame_duration` | `Option<Ts>` | from the packet spacing (CFR); `None` if the spacing is uneven |
| `truncated` | `bool` | a cap (20,000 keyframes or 30 s) was hit |

`KeyframeEntry { at: Ts, clean: bool }`: `clean` is false when the open-GOP rule (research R2)
finds a leading picture after this keyframe. A plan-time index also keeps every packet's (pts,
dts) inside the window, so frames can be counted; the desktop index keeps keyframes only.

## FallbackReason (new, `smart/mod.rs`)

```text
enum FallbackReason {
  EncoderMissing, Codec, Container, Profile, PixelFormat, Interlaced, Vfr, Rotation,
  NoFullGop, OpenGop, ParamsChanged, SeamCheck
}   // serde snake_case: "encoder_missing", "codec", …, "seam_check"
```

- `message()` gives a plain-language sentence, for example "the video is HEVC; smart cut handles
  H.264 only".
- `known_before_encoding()` is false only for `ParamsChanged` and `SeamCheck`.
- The values are stable once released (FR-007) and are listed in `docs/reference/` with the issue
  codes.

## SmartPlan (new, `smart/mod.rs`; part of `ClipSpec`)

| Field | Type | Notes |
|---|---|---|
| `head` | `Option<Part>` | `None` when the start is on a keyframe |
| `copy` | `Part` | the GOPs [K1, K2) |
| `tail` | `Option<Part>` | `None` when the end is on a keyframe or is the end of media |
| `copied_share` | `f64` | copy frames ÷ all frames |
| `copy_lead` | `Ts` | how far before K1 the copy is cut (research R3) |
| `to_end`, `frame`, `first_gop`, `last_gop` | | the copy runs to the end; frame spacing; frames in the first and last copied GOP (the seam checks, R7) |
| `video_index`, `zero`, `shift`, `precise` | | where the parts' seeks point, as in accurate mode |
| `strict` | `bool` | `smart_fallback: false` (FR-009) |
| `encode` | `PartEncode` | the matched encoder settings (research R4) |
| `sample_entry` | `Option<&'static str>` | `"avc1"` for mp4/m4v/mov (research R5, spike S1); `None` for mkv and ts |
| `rotation` | `Option<f64>` | the head and tail are encoded with `-noautorotate`; set again on the join's input (research R5, spike S2) |

`Part { span: Span, frames: u32 }`.

`PartEncode { profile, level, bframes, pix_fmt, colour: ColourTags, sar: Option<(u32, u32)>, width, height }`
(references are not matched, research R4)
holds no free-form strings. The argument builder (`smart/args.rs`) maps it to argv, and the argv
is snapshot-tested.

`ClipSpec` (`plan/ffargs.rs`) gains `smart: Option<SmartPlan>`. It is `Some` only when the mode
resolved to `Smart` and the check passed. On fallback, the job is an ordinary accurate
`ClipSpec`, and the reason is kept in `PlannedJob.smart` (below).

## Planned job additions (dry run, `plan/mod.rs`)

`PlannedJob` gains `smart: Option<SmartSummary>`. It is present whenever the requested mode was
`Smart`:

```text
SmartSummary {
  requested: true,
  mode: "smart" | "accurate",          // what will run
  head, copy, tail: Option<Part>,      // copy is absent on fallback
  copied_share: Option<f64>,
  fallback: Option<FallbackReason>,    // a reason known before encoding
  strict: bool,
}
```

- A fallback adds the notice `SMART_FALLBACK` (warning severity), with `detail.reason`.
- Under strict mode it adds the planning issue `SMART_UNAVAILABLE` (error severity) instead, with
  `detail.reason`, and the job is not runnable.

`Plan` gains `smart: Option<SmartTotals>`, present when any job requested smart:
`{ smart: usize, fallback: BTreeMap<FallbackReason, usize>, not_applied: usize }`.

## Issue codes (changed, `error.rs`)

| Code | Severity | When |
|---|---|---|
| `SMART_FALLBACK` | warning (notice) | the job runs in accurate mode instead; `detail.reason` |
| `SMART_UNAVAILABLE` | error | strict mode and a reason was found; `detail.reason` (plan time, or a job failure for `params_changed`/`seam_check`) |
| `SMART_NOT_APPLIED` | notice | the job also upscales or makes a GIF, so smart does not apply |

## JobMeasure::Clip (changed, `event.rs`)

```text
Clip {
  actual: Span, adjusted: bool,                    // unchanged
  smart: Option<SmartOutcome>,                     // new; absent unless smart was requested
}
SmartOutcome {
  mode_used: Mode,                                  // Smart or Accurate
  copied_frames: u32, reencoded_frames: u32,        // 0 copied on fallback
  copied_share: f64,
  fallback: Option<FallbackReason>,                 // including run-time params_changed/seam_check
}
```

## Job stages and progress (`exec/smart.rs`)

| Stage | Process | Weight (research R9) |
|---|---|---|
| `head` | ffmpeg encode, `-frames:v` | 1.0 × head frames |
| `copy` | ffmpeg stream copy | 0.02 × copy frames |
| `tail` | ffmpeg encode | 1.0 × tail frames |
| `audio` | ffmpeg audio encode | 0.05 × seconds |
| `join` | ffmpeg concat + mux | 0.02 × all frames |
| `verify` | ffprobe packet scan; ffmpeg seam windows; framemd5 | 0.3 × decoded frames |

State transitions for one smart job:

```text
planned ─► head ─► copy ─► tail ─► audio ─► join ─► verify ─┬─► commit ─► succeeded(smart)
                                                            └─► discard ─┬─► accurate run ─► succeeded(accurate, fallback)
                                                                         └─► failed(SMART_UNAVAILABLE)  [strict]
any stage ─► cancel ─► scratch removed ─► cancelled
```

## Desktop types (`apps/desktop`, generated with ts-rs)

- `KeyframeIndex` and `KeyframeEntry` as above.
- `SmartSummary` and `FallbackReason`, the same types as in the plan JSON.
