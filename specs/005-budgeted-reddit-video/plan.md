# Implementation Plan: Budgeted Reddit Video

**Branch**: `005-budgeted-reddit-video` (work happens on `main`) | **Date**: 2026-11-19 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/005-budgeted-reddit-video/spec.md`

## Summary

Add **fit**: `--fit TARGET` makes a clip an MP4 with audio that is guaranteed to be at or under a
video target's size cap and length limit. Targets are data, like GIF targets; the built-in
`reddit-video` carries Reddit's limits as of 2026-11-13 (`research/10`).

**How the core does it** (each choice was measured with the bundled engine, see
[research.md](./research.md)):

- **Plan (pure arithmetic, R2–R5)**:
  - Trim to the length limit, or refuse with `RANGE_TOO_LONG`.
  - Compute the budget bitrate: `B = (cap × 0.98 − audio) × 8 / len`.
  - Below the target's quality ceiling `C = max_bpp × w × h × fps`, use **budget mode**. Otherwise
    use **quality mode** (CRF 20 capped at `C`, so a generous cap isn't filled).
  - Step the size down by 0.75 while `B` is below the minimum `min_bpp`, never below the floor.
  - Then refuse with `BUDGET_UNREACHABLE` (with the smallest workable cap), or trim.
  - The prediction needs no sample encode.
- **Encode (R1)**: budget mode is two-pass x264 with a VBV ceiling (`-b:v B -maxrate B -bufsize
  2B`). It landed at 95.6–99.7% of the cap on 9 test cases. Plain two-pass overshot by 60–72% on
  noise, and capped CRF passed the cap by up to 0.9%.
- **Guarantee (R2, R9)**: the `.part` file is measured before commit. Over the cap, pass 2 runs
  again lower, reusing pass 1's stats, at most 3 times; after that, `FIT_FAILED`. Nothing over the
  cap reaches the output name.
- **Audio (R6)**: the first track as AAC-LC, 128 kbit/s (floor 64), at most 2 channels, cut
  with 001's accurate cut. No audio track when the source has none.
- **Where (R7)**: a `FitPlan` field on `ClipSpec` and `UpscaleSpec`, plus one shared
  `exec::fit::encode`. 002's presets give the shape and reframing; the target gives the caps.
- **Reuse**: request layering, manifests, naming, scratch, journal, verify-then-commit, events,
  the dry run and exit codes from 001–004. GIF, the silent Reddit MP4, and clips without `--fit`
  are unchanged.

## Technical Context

**Language/Version**: Rust 1.94 (edition 2024); TypeScript 6 + Svelte 5 for the desktop changes.

**Primary Dependencies**:
- No new crates and no new sidecars.
- The bundled FFmpeg n9.0.2 with libx264 (two-pass, VBV) and the native `aac` encoder, as used in
  the R1/R2 measurements.

**Storage**: files only.
- Video targets: `<config>/video-targets/*.json`.
- Pass-1 stats: in the job's scratch folder (journaled, removed on cancel).

**Testing**: `cargo test`, with:
- table-driven unit tests for the pure `fit::plan` (ladder, regimes, trims, refusals, the
  invariant `predicted ≤ cap × 0.98`), the target validation and the size parser;
- lavfi fixtures (no binary fixtures), a new `testkit::Fixture` for each:
  - a busy 1080p30 source with stereo audio;
  - a video-only source;
  - 5.1 audio;
  - HDR (reusing 002's HDR fixture);
  - VFR (reusing `VfrMp4`);
  - a 9:16 case through the `reddit-916` preset;
- integration tests that check the size against the cap, ffprobe (codecs, `yuv420p`, even
  sizes, `moov` before `mdat`), duration ±1 frame, audio start offset, and a `-v error` decode;
- a forced-overshoot test (a test-only hook makes the first attempt report a size over the
  cap), so the retry path runs deterministically;
- insta snapshots of the pass-1, pass-2 and quality argv;
- a desktop `bridge.rs` test for CLI = desktop plans.

**Target Platform**: Linux x86_64 and Windows 10/11 x86_64.

**Project Type**: Rust workspace (library, CLI, Tauri desktop app, xtask). No new crate.

**Performance Goals**:

| Goal | Target |
|---|---|
| Plan and prediction | instant (arithmetic; no sample encode) |
| Budget-mode encode | about 2× a single pass at the same bitrate (measured: pass 1 ≈ pass 2, R1) |
| Retries | ≤ 5% of jobs (SC-004), each costs one pass 2 |
| Cancel | ≤ 2 s at any pass (existing kill-tree) |

**Constraints**:
- Never above the hard cap (FR-005). Sources are never modified, and every tool is called with an
  argv through `exec::process`. Works offline.
- MP4/H.264/AAC only (Reddit). Fast remains the default clip mode.

**Scale/Scope**: single user; ranges from seconds to 15 minutes (longer with a user target);
batches of tens.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-design | Post-design |
|---|---|---|---|
| I. One Core, Thin Surfaces | Targets, planning, encoding, measuring and retries in `rmedia-core` | PASS | PASS. The CLI flags and the desktop picker only build `PartialFit`; `list_fit_targets` calls core ([contracts/cli.md](./contracts/cli.md), [contracts/tauri-commands.md](./contracts/tauri-commands.md)) |
| II. CLI-First, Scriptable | `--fit`, `--json`, `--dry-run` with the prediction, existing exit codes, repeatable output | PASS | PASS. [contracts/json-output.md](./contracts/json-output.md) only adds keys. Determinism is tested (R12) |
| III. Test-First with Real Media | lavfi fixtures; ffprobe and size checks; no binary fixtures | PASS | PASS. Every SC has a test (quickstart); the planner is pure and table-tested |
| IV. Linux = Windows | No shell; kill-tree cancel; pass-1 stats in scratch with relative names | PASS | PASS. `-passlogfile` gets a relative name with cwd = scratch, as 003's palettes do |
| V. Inputs Are Sacred | `.part` → measure → verify → no-clobber commit | PASS | PASS. An over-cap `.part` is re-encoded or removed, never committed |
| VI. Graceful Degradation | No libx264/aac → fit unavailable, and `doctor` says so | PASS | PASS (`ENGINE_MISSING`, exit 4) |
| VII. Simplicity & Evidence | Choices cite `research/10`, `research/09` §5, R1–R12 | PASS | PASS. See Complexity Tracking |
| Licensing | No new components | PASS | PASS |
| Presets as data | Destination limits are data files, not code | PASS | PASS (video targets, R10) |

No unjustified violations.

## Project Structure

### Documentation (this feature)

```text
specs/005-budgeted-reddit-video/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R12, with measurements
├── data-model.md
├── quickstart.md        # checks per success criterion, desktop run, Phase 0 script
├── contracts/
│   ├── cli.md
│   ├── json-output.md
│   ├── manifest-fit.md
│   ├── tauri-commands.md
│   └── video-target.md
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
crates/rmedia-core/src/
├── fit/                      # new
│   ├── mod.rs                # FitOptions, PartialFit, FitOutcome, size parser
│   ├── target.rs             # VideoTarget, VideoTargetRegistry, validation
│   ├── builtin/reddit-video.json
│   └── plan.rs               # pure planner: trim, regimes, ladder, refusals (R2–R5)
├── data_files.rs             # new: the user-file loader shared with gif/target.rs (R10)
├── exec/fit.rs               # new: pass 1, pass 2 or quality pass, measure, retry (R1, R2, R9)
├── exec/clip.rs, exec/upscale.rs   # call exec::fit::encode when the spec has a fit
├── plan/ffargs.rs            # rate-control and audio args for a FitPlan; even scaling
├── plan/planner.rs           # resolve --fit: mode → accurate, container → mp4, FitPlan
├── plan/model.rs             # PlannedJob fit block; space_needs counts the scratch
├── request.rs                # fit layering (PartialOptions/Options)
├── input/manifest_json.rs, input/manifest_csv.rs   # fit object and columns
├── gif/target.rs             # uses data_files (no behaviour change)
├── error.rs                  # new codes, Error::FitFailed
└── doctor.rs                 # fit block in the report
crates/rmedia-core/tests/fit_us1.rs … fit_us4.rs   # integration tests per story
crates/rmedia-testkit/src/fixtures.rs              # new fixtures
crates/rmedia-cli/src/main.rs, commands/{clip,upscale,batch}.rs, render.rs   # flags, lines
apps/desktop/src-tauri/src/ops/fit.rs, commands.rs  # list_fit_targets
apps/desktop/src/lib/…                              # target picker, queue display
docs/…                                              # cli-reference, json-output, file-formats, desktop-ipc, user guides
```

**Structure Decision**: one new core module, `fit/`, follows `gif/`, plus one executor module,
`exec/fit.rs`. The clip and upscale executors stay as they are, except for one call.

## Phases (for /speckit-tasks)

1. **Setup**:
   - Generalise the GIF target loader into `data_files` (no behaviour change; the GIF target
     tests still pass).
   - Add the new issue codes.
2. **Foundational**:
   - `VideoTarget` and the registry with `reddit-video`.
   - `FitOptions`/`PartialFit` layering and the size parser.
   - The pure `fit::plan` with its tables.
   - Fixtures.
3. **US1 (P1, MVP)**: `clip --fit` with budget and quality modes, measuring and retries, audio,
   the result block. SC-001–SC-004.
4. **US2 (P2)**: length refusals and trims, the ladder, `BUDGET_UNREACHABLE` with `min_cap`. SC-005.
5. **US3 (P3)**: the dry-run text and JSON, `--list-fit-targets`, `doctor`. SC-006.
6. **US4 (P4)**: `upscale --fit`, `clip --upscale --fit`, manifests (JSON, CSV), `batch`, and the
   mode conflicts.
7. **US5 (P5)**: desktop `list_fit_targets`, the picker, prediction, queue and Settings. SC-007.
8. **Polish**:
   - docs;
   - the senior code review and the security review (paths: `-passlogfile`, target files);
   - the release notes for 0.5.0;
   - the owner check SC-008.

## Complexity Tracking

No violations. These choices were weighed and kept simple:

| Choice | Why | Simpler alternative rejected because |
|---|---|---|
| Two encoder passes in budget mode | the only measured mode that stays under the cap and above 95% (R1) | One pass with a size check passed the cap (capped CRF, 1.009×) or wasted budget (0.80×) |
| A shared `data_files` loader | GIF and video targets share their rules (R10) | Copying the GIF loader would let the two drift apart |
