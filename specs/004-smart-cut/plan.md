# Implementation Plan: Smart Cut

**Branch**: `004-smart-cut` (work happens on `main`) | **Date**: 2026-11-14 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/004-smart-cut/spec.md`

## Summary

Add a third clip mode, `smart`. It is frame-exact like accurate mode, but it copies every whole
GOP in the range and re-encodes only the partial GOPs at each end.

**How the core does it** (each step was measured with the bundled engine, see
[research.md](./research.md)):
- **Keyframe index** (R1): ffprobe packet flags in a window around the range, with no decoding.
  Open GOPs are found from decode order: a packet after a keyframe with an earlier pts.
- **Eligibility** (R2): one pure check returns a `SmartPlan` or a `FallbackReason`. There are 12
  stable reasons, 10 of them known before encoding.
- **Middle** (R3): a stream copy as fast mode makes it, cut a few frames before K1 and ended by
  the exact packet count, so the copied GOPs [K1, K2) come out frame-exact and identical to the
  source. (The segment muxer, chosen first, depends on where seeks land and was dropped in US1.)
- **Head and tail** (R4): libx264 with exact seeks and exact frame counts, matched to the source's
  profile, level, references, B-frames, pixel format, colour and SAR; `stitchable=1`; written as
  MPEG-TS parts.
- **Join** (R5): the TS parts go through the concat demuxer and are muxed with the audio. mp4 and
  mov keep the `avc1` sample entry with in-band parameter sets, which may change between parts
  (spike S1: FFmpeg, GStreamer, VLC, Firefox and Chrome decode it; `avc3` fails in VLC and
  Chrome). Rotated sources: head and tail with `-noautorotate`, rotation set again in the join
  (spike S2).
- **Audio** (R6): one re-encode over the whole range, as in accurate mode.
- **Verification before commit** (R7): a packet scan, decodes of the two seam windows, and a
  framemd5 check of the copied GOPs at the seams. A failure re-runs the job in accurate mode, or
  fails it in strict mode.
- **Reuse**: inputs, layering, naming, commit, journal, scratch, events, dry run and exit codes
  from 001–003. Upscale and GIF are untouched; clips that request them run as accurate.

## Technical Context

**Language/Version**: Rust 1.94 (edition 2024); TypeScript 6 + Svelte 5 for the desktop changes.

**Primary Dependencies**:
- No new crates and no new sidecars.
- Bundled FFmpeg n9.0.2 with libx264, the concat demuxer and mpegts (all used
  in the R1–R8 measurements and spikes S1–S3).

**Storage**: files only.
- Parts go in the job's scratch folder (002 convention, journaled).
- The desktop keyframe index is cached in memory per source.

**Testing**: `cargo test`, with:
- new `rmedia-testkit` fixtures:
  - H.264 with closed GOPs in mp4, mov, mkv and ts;
  - no B-frames (Baseline);
  - open GOP;
  - High 10 (10-bit);
  - HEVC;
  - a resolution change mid-stream;
  - a source made with different encoder settings (preset slow, `ref=4`);
- existing fixtures: `Gop50Mp4`, `Hd60Mp4`, `VfrMp4`, `OffsetTs`, `MislabelledTs`, `Rotated90`,
  `Anamorphic`, the interlaced fixture, `MultiMkv`;
- checks with ffprobe packet scans, `-v error` decodes, framemd5 against the source, and
  `testkit::vmaf()` for SC-006;
- insta snapshots of the head, copy, tail, audio and join argv.

**Target Platform**: Linux x86_64 and Windows 10/11 x86_64.

**Project Type**: Rust workspace (library, CLI, Tauri desktop app, xtask). No new crate.

**Performance Goals**:

| Goal | Target |
|---|---|
| 60 s range of 1080p | ≤ ⅓ of accurate mode's time (SC-005; measured 27–38%, R8) |
| Keyframe markers, 10-min source | ≤ 2 s (SC-007; measured 0.10–0.35 s, R1) |
| Seam verification | decodes ≤ 2 GOPs per seam (R7) |
| Cancel | ≤ 2 s at any stage (SC-008) |

**Constraints**:
- Sources are never modified, and every tool is called with an argv through `exec::process`.
- Works offline.
- Fast stays the default mode.
- Frame-exact within one frame (FR-002), and nothing is committed before verification (FR-010).

**Scale/Scope**: single user; clips from seconds to hours (copy cost is I/O-bound); batches of
tens.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-design | Post-design |
|---|---|---|---|
| I. One Core, Thin Surfaces | Index, eligibility, part planning, argv, verification in `rmedia-core` | PASS | PASS. The CLI flags and the Tauri `keyframes`/`smart_plan` commands call core ([contracts/cli.md](./contracts/cli.md), [contracts/tauri-commands.md](./contracts/tauri-commands.md)) |
| II. CLI-First, Scriptable | `--mode smart`, `--no-smart-fallback`, `--json`, `--dry-run` showing the split, stable exit codes, repeatable output | PASS | PASS. [contracts/json-output.md](./contracts/json-output.md) is additive. Output is deterministic per machine, as in accurate mode (R10) |
| III. Test-First with Real Media | lavfi fixtures; ffprobe and decode checks; no binary fixtures | PASS | PASS. Every rule in R2 has a fixture (SC-004) |
| IV. Linux = Windows | No shell; kill-tree cancel; scratch cleanup; paths as `file:` | PASS | PASS. Concat list entries are relative names with cwd = scratch (as 003's palettes), so no path escaping is needed |
| V. Inputs Are Sacred | `.part` → verify → no-clobber commit; parts in scratch | PASS | PASS. A failed verification discards the `.part` file before any fallback |
| VI. Graceful Degradation | No libx264 → smart unavailable, jobs fall back, `doctor` says so | PASS | PASS |
| VII. Simplicity & Evidence | Choices cite research/01 §1.4 and R1–R11 | PASS | PASS. See Complexity Tracking |
| Licensing | No new components | PASS | PASS |

No unjustified violations.

## Project Structure

### Documentation (this feature)

```text
specs/004-smart-cut/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R11, with measurements
├── data-model.md
├── quickstart.md        # checks per success criterion, manual run, Phase 0 spikes
├── contracts/
│   ├── cli.md
│   ├── json-output.md
│   ├── manifest-smart.md
│   └── tauri-commands.md
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
crates/rmedia-core/src/
├── request.rs            # Mode::Smart; Options.smart_fallback
├── error.rs              # SMART_FALLBACK, SMART_UNAVAILABLE, SMART_NOT_APPLIED
├── event.rs              # JobMeasure::Clip.smart (SmartOutcome)
├── doctor.rs             # smart availability
├── smart/                # NEW
│   ├── mod.rs            # SmartPlan, FallbackReason, check() (R2)
│   ├── keyframes.rs      # KeyframeIndex, open-GOP rule (R1, R2)
│   └── args.rs           # head/tail/copy/audio/join argv (R3–R6), snapshot-tested
├── plan/mod.rs           # mode resolution, SmartSummary, Plan.smart totals
├── plan/ffargs.rs        # ClipSpec.smart
├── exec/smart.rs         # NEW: stages, verification (R7), run-time fallback
├── exec/clip.rs          # dispatch to exec/smart.rs when ClipSpec.smart is Some
└── input/manifest_csv.rs # `smart` mode value, `smart_fallback` column
crates/rmedia-core/tests/
├── smart_us1.rs … smart_us4.rs   # NEW, one per user story
├── cancel.rs, bench.rs, us3_batch.rs, snapshots_ffargs.rs   # extended
crates/rmedia-testkit/src/lib.rs  # new fixtures (see Testing)
crates/rmedia-cli/src/            # --mode smart, --no-smart-fallback, dry-run lines, doctor line
apps/desktop/src-tauri/src/       # keyframes, smart_plan commands; index cache
apps/desktop/src/lib/components/  # Player markers and bands; Queue, RunPanel, Settings
docs/                             # user guide (modes), reference (codes), architecture, releases/0.4.0.md
```

**Structure Decision**: all of the smart logic sits in a new core module, `smart/`, beside `gif/`
and `upscale/`. Execution goes in `exec/smart.rs` and is dispatched from the clip job, so
naming, collisions, the journal, commit and events are the clip job's own. There is no new
crate.

## Phasing (maps to user stories)

| Phase | Stories | Exit criteria |
|---|---|---|
| 0 Foundation | — | `Mode::Smart` and `smart_fallback` parse everywhere (JSON, CSV, CLI, settings), still planning as accurate. New fixtures. `KeyframeIndex` with the open-GOP rule (table tests on recorded packet JSON, then real fixtures). Spikes S1 (players), S2 (rotation), S3 (ts copy seek) recorded in research.md. All 001–003 tests and snapshots green |
| 1 MVP | US1 (P1) | `check()` for eligible sources; head, copy, tail, audio and join; R7 verification; `SmartOutcome`; stages, progress and cancel. SC-001, SC-002, SC-003, SC-005, SC-006, SC-008 |
| 2 | US2 (P2) | Every fallback reason with a fixture; run-time fallback after a failed verification; strict mode and its exit codes; `SMART_NOT_APPLIED`. SC-004 |
| 3 | US3 (P3) | Dry-run split lines and JSON `smart` blocks; batch totals |
| 4 | US4 (P4) | Manifest JSON/CSV `mode: smart` and `smart_fallback`; export round trip; CLI ↔ desktop parity. SC-009 |
| 5 | US5 (P5) | `keyframes` and `smart_plan` commands; Player markers and bands; Queue, RunPanel and Settings. SC-007 |
| 6 Polish | — | Docs (user guide, reference codes, architecture), release notes 0.4.0, roadmap; `doctor` line; owner checks: playback on phones and a Reddit upload of a smart mp4 |

## Requirement traceability

| Requirements | Where designed |
|---|---|
| FR-001 mode everywhere | data-model › Mode, Options.smart_fallback; [cli.md](./contracts/cli.md); [manifest-smart.md](./contracts/manifest-smart.md) |
| FR-002 frame-exact, FR-003 copy middle | R3, R4 (exact seeks, `-frames:v`), SmartPlan |
| FR-004 parameter matching | R4, `PartEncode`, R5 (in-band parameter sets, spike S1) |
| FR-005 audio and streams | R6 |
| FR-006 H.264 in mp4/mov/mkv/ts | R2 `codec`, `container` |
| FR-007/008 fallback reasons, not an error | R2, FallbackReason, `SMART_FALLBACK` |
| FR-009 strict | `smart_fallback=false`, `SMART_UNAVAILABLE`, exit codes 3/1 |
| FR-010 seam verification | R7 |
| FR-011/012 plan and result reporting | data-model › SmartSummary, SmartTotals, SmartOutcome; [json-output.md](./contracts/json-output.md) |
| FR-013 upscale/GIF untouched | `SMART_NOT_APPLIED`; mode resolution in `plan/mod.rs` (already forces accurate) |
| FR-014 manifests | [manifest-smart.md](./contracts/manifest-smart.md) |
| FR-015/016 desktop, bounded index | R1, R11, [tauri-commands.md](./contracts/tauri-commands.md) |
| FR-017 diagnostics | `doctor` smart line |
| FR-018/019 001 guarantees, parity | R9, R10; scratch, commit and journal reuse |
| SC-001…009 | Phases 1–5 exit criteria; checks in [quickstart.md](./quickstart.md) |

## Risks

| Risk | Mitigation |
|---|---|
| Players that mishandle parameter-set changes inside one file (R5) | Spike S1 done on the desktop players (all decode `avc1` correctly); WebView2, phones and a Reddit upload remain owner checks |
| Sources whose parameter sets cannot be matched by x264 (other encoders' SPS) | Parameter sets travel in-band, so they need not be identical; the seam decode (R7) catches incompatibilities and falls back |
| Rotation lost in the join (R5) | Spike S2: `-noautorotate` parts and `-display_rotation` on the join keep it in mp4, mov and mkv; ts outputs fall back with `rotation` |
| ts sources with imprecise copy seeks | A coarse seek 10 s before K1 and an output-side cut (R3); the copied frame count and the copied frames are verified (R7) |

## Complexity Tracking

| Component | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| Multi-step job (head, copy, tail, audio, join, verify) in `exec/smart.rs` | FR-002/003: exact ends with a copied middle need separate encode and copy passes | One ffmpeg command cannot copy and encode the same video stream; filter-based cutting re-encodes everything (that is accurate mode) |
| MPEG-TS intermediate parts | R5: joining mp4 parts broke DTS monotonicity at seams (measured) | Joining mp4 or mkv parts directly failed or needs timestamp rewriting |
| In-band parameter sets that change between parts | R5: the head, copy and tail carry different parameter sets | A single `avcC` cannot describe several parameter sets with FFmpeg's muxer; identical parameter sets cannot be guaranteed (R4); `avc3` is less compatible (spike S1) |
| Three new issue codes and the `smart_fallback` option | FR-007/008/009: scriptable fallbacks and strict mode | Plain warning strings are not stable to scripts; a mode-only switch could not express strict mode |
