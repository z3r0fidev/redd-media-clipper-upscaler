---
description: "Task list for 005 Budgeted Reddit Video"
---

# Tasks: Budgeted Reddit Video

**Input**: Design documents from `specs/005-budgeted-reddit-video/`

**Prerequisites**: plan.md, spec.md, research.md (R1–R12), data-model.md, contracts/,
quickstart.md

**Status (2026-11-19)**: T001–T022 done: Phases 1–2 and US1, the MVP. `clip --fit` makes an MP4
with audio at or under the cap, frame-exact, repeatable, with bounded retries. Changes found while
implementing:
- T014 found that x264's VBV with frame threads isn't repeatable, so pass 1 runs on one thread and
  the outputs use sliced threads (research R13).
- T012 found that quality mode can pass `rate × length` by its VBV buffer, so it needs `B·len ≥
  C·(len + 2 s)` (`VBV_SECS`).
- T013 found that a retry must stop at the target's minimum bitrate (x264 refuses very low
  two-pass rates).
- T017 dropped `FIT_REENCODES`: fast is the default mode and is ignored silently, as for upscale
  and GIF.
- `--fit` with upscaling is refused until T037.

**Tests**: REQUIRED. Constitution Principle III (test-first with real media) is non-negotiable.
- Within each story, write the test tasks first and confirm they FAIL before implementing.
- Media comes from `rmedia-testkit` lavfi fixtures. No binary fixtures or engine binaries are
  committed.

**Organization**:
- Grouped by user story, in priority order US1 → US5.
- Phase 2 makes `fit` a valid option everywhere (targets, layering, the pure planner) before any
  encode exists.

## Format: `[ID] [P?] [Story] Description`
- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Paths are relative to the repo root.
- Shorthand:

| Short | Path |
|---|---|
| `core/` | `crates/rmedia-core/src/` |
| `core-tests/` | `crates/rmedia-core/tests/` |
| `cli/` | `crates/rmedia-cli/src/` |
| `cli-tests/` | `crates/rmedia-cli/tests/` |
| `desk/` | `apps/desktop/src-tauri/src/` |
| `ui/` | `apps/desktop/src/lib/` |
| `kit/` | `crates/rmedia-testkit/src/` |

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Record the 0.4.0+ baseline in a new `specs/005-budgeted-reddit-video/checklists/validation.md`:
  - run `cargo test --workspace` (bundled engine, `RMEDIA_REQUIRE_FFMPEG=1`), the desktop tests
    (`apps/desktop/src-tauri`: `cargo test`; `apps/desktop`: `npm test`) and the MinGW Windows
    check;
  - note the counts (workspace 581, desktop 34, vitest 48 on 2026-11-19) and the insta snapshot
    list.

  Every phase must keep them green, with no snapshot changes for jobs without a fit (FR-022).
- [X] T002 Scaffold and register the empty modules:
  - `core/fit/{mod,target,plan}.rs` and `core/fit/builtin/reddit-video.json`
    (`pub mod fit` in `core/lib.rs`);
  - `core/exec/fit.rs` (in `core/exec/mod.rs`);
  - `core/data_files.rs`.

  Test files are created with their stories' first tests, so no empty test binaries are built.
- [X] T003 [P] Add the new issue codes to `core/error.rs`, with `as_str` tests:
  - `RangeTooLong` (`RANGE_TOO_LONG`), `FitLowered`, `FitTrimmed`, `FitFailed`;
  - `Error::FitFailed { cap, attempts }`, mapped to exit 1.

  Document them in `docs/api/json-output.md` (codes table only; the rest comes in US3).

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: targets, options and the pure planner: everything US1–US5 read.

- [X] T004 Test first, `core/data_files.rs` unit tests (moved from `core/gif/target.rs`):
  - the 64-file cap and its "N files ignored" problem;
  - capped reads and symlinks;
  - duplicate user ids;
  - a user file replaces a built-in in place;
  - user-only targets are sorted.

  Then extract `data_files::load<T: DataFile>(dir_name, builtins)` from
  `GifTargetRegistry::load` and switch the GIF registry to it. Every existing GIF target test
  passes unchanged (R10).
- [X] T005 [P] Test first, then write `core/fit/target.rs`:
  - `VideoTarget` (schema 1) with `from_json`;
  - validation exactly as data-model.md › VideoTarget: ranges, even `long_side`,
    `min_bpp < max_bpp`, `floor_kbps ≤ kbps`, unknown fields refused, printable text;
  - `VideoTargetRegistry::{builtin, load, get, ids, problems, label}` on `data_files`, reading
    `<config>/video-targets/`;
  - `core/fit/builtin/reddit-video.json` from contracts/video-target.md (`verified: false`).

  The tests are table-driven: one bad value per rule, and the built-in parses.
- [X] T006 [P] Test first, then write the size and length parser in `core/fit/mod.rs`:
  `parse_size("50MB" | "8MiB" | "1GB" | "123")`, decimal and binary units, refusing 0,
  negatives, overflow and junk. Lengths reuse `core/time.rs`'s duration parser. The tests are
  table-driven (Principle III, malformed input).
- [X] T007 Test first, then add `FitOptions`/`PartialFit` in `core/fit/mod.rs`, layered through
  `PartialOptions.fit`/`Options.fit` in `core/request.rs`:
  - `apply` and `diff`, as `PartialGif`;
  - `enabled: false` turns an inherited fit off;
  - any other key turns it on;
  - `max_size`/`max_length` may only lower the target's limits; the check runs at plan time,
    since the target isn't known while layering.

  Tests go next to the `gif_layers_*` tests in `core/request.rs`.
- [X] T008 Test first, then write the pure planner `core/fit/plan.rs`:
  `plan_fit(target, opts, len, source_size, display_aspect, fps, has_audio) ->
  Result<FitPlan, Problem>`, in data-model.md's planning order. Unit tables cover:
  - budget against quality mode at the ceiling `C` boundary (R3);
  - the ladder, ×0.75 long side, even, stopping at the floor and at the source size (R4);
  - the audio lowered to its floor;
  - `BUDGET_UNREACHABLE` with `min_cap` as R4's formula;
  - the trim formulas for the length (R5) and the budget (R4);
  - no audio, so the whole budget goes to the video;
  - fps capped at `max_fps`;
  - an override above the target refused;
  - a property check over a grid of caps, lengths and sizes: `predicted_bytes ≤ cap × 0.98`
    for every accepted plan.
- [X] T009 [P] Add the testkit fixtures in `kit/fixtures.rs`:
  - `FitBusy` (1920×1080 30 fps, 60 s, `testsrc2` plus `noise=alls=20:allf=t+u`, stereo sine,
    libx264 `-crf 12` mp4);
  - `FitNoAudio` (as `FitBusy`, with no audio);
  - `FitSurround` (as `FitBusy`, with 5.1 audio);
  - `FitShort` (2 s).

  Reuse `VfrMp4`, the HDR fixture and the 4K or portrait fixtures that exist. Add
  `kit/inspect.rs` helpers:
  - `moov_before_mdat(path)`, from the top-level boxes;
  - `audio_start_offset(path) -> f64` (the audio start minus the video start).
- [X] T010 Add `video_targets: VideoTargetRegistry` to `Caps` (`core/plan/model.rs`). Load it
  where the callers load the GIF targets: CLI commands, the desktop `ops/mod.rs` engine cache,
  and `core-tests/common/mod.rs`. The core never reads the settings itself.

**Checkpoint**: `fit` parses, layers and plans in unit tests. Nothing encodes yet.

---

## Phase 3: User Story 1 - A clip with sound that Reddit accepts (Priority: P1) 🎯 MVP

**Goal**: `rmedia clip --fit TARGET` makes an MP4 (H.264 + AAC) at or under the cap, frame-exact,
with its audio in sync.

**Independent Test**: quickstart SC-001 to SC-004 on `FitBusy` with `reddit-video` and with a
small cap.

### Tests for User Story 1 (write first; they must fail)

- [X] T011 [P] [US1] `core-tests/fit_us1.rs`: `FitBusy` 0:05–0:50 with a 20 MB cap. Check:
  - `bytes ≤ cap` and `≥ 0.9 × cap`;
  - `mode = budget`;
  - ffprobe shows h264 High yuv420p and aac LC with 2 channels;
  - even sizes;
  - `moov_before_mdat`;
  - duration 45 s ± 1 frame;
  - `audio_start_offset` within 21.3 ms;
  - a `-v error` decode is clean.
- [X] T012 [P] [US1] `core-tests/fit_us1.rs`: `FitShort` with `reddit-video`, so `mode =
  quality`, one attempt, and the size well under `C × len` (FR-007). `FitNoAudio` gives an
  output with no audio stream that uses the whole budget for the video.
- [X] T013 [P] [US1] `core-tests/fit_us1.rs`: the retry path. Under the `test-support` feature,
  a hook `fit::force_overshoot(n)` makes the first `n` measurements report `cap + 1`. Check:
  - one forced overshoot gives two attempts, the second at a lower `video_kbps` (R2's formula);
  - pass 1 runs once (its stats are reused) [K → verified here];
  - four forced overshoots give `FIT_FAILED` (exit 1), no output file, and no `.part` or scratch
    files left.
- [X] T014 [P] [US1] `core-tests/fit_us1.rs`: the same job run twice gives identical bytes (R12).
  Cancelling during pass 1 and during pass 2 removes the scratch and the `.part` (the existing
  cancel helpers).
- [X] T015 [P] [US1] `core-tests/snapshots_fit_args.rs`: insta snapshots of the argv for pass 1
  (`-pass 1 -passlogfile fit -an -f null -`), pass 2 (`-b:v`, `-maxrate`, `-bufsize`,
  `-c:a aac -b:a`, `-ac 2`, `-movflags +faststart`, `-tag:v avc1`, `-pix_fmt yuv420p`), and the
  quality pass (`-crf 20 -maxrate C -bufsize 2C`). Every path is `file:`; the stats file is a
  relative name.
- [X] T016 [P] [US1] `cli-tests/fit.rs`:
  - `rmedia clip src --fit reddit-video --fit-max-size 20MB` exits 0, with the result line
    `… MB of 20 MB (…%) · … · 2 passes`;
  - `--json` carries `fit` in `job_finished`;
  - `--fit-max-size` without `--fit` exits 2;
  - `--fit` with `--gif` exits 2;
  - `--fit unknown` exits 3 with `TARGET_UNKNOWN`.

### Implementation for User Story 1

- [X] T017 [US1] `core/plan/planner.rs`: resolve a fit in `plan_clip`:
  - the mode becomes accurate, with the `SMART_NOT_APPLIED` notice for an explicit smart mode
    (contracts/cli.md; fast, the default, is ignored silently);
  - the container becomes mp4, and `-c` with another container is refused (exit 3);
  - the first default audio track is kept, with a notice for dropped extra tracks (R6);
  - call `fit::plan::plan_fit`, then store `ClipSpec.fit`;
  - give the job a scratch folder, as GIF jobs have;
  - `space_needs` (`core/plan/model.rs`) counts the cap twice (scratch stats plus `.part`).
- [X] T018 [US1] `core/plan/ffargs.rs`:
  - `fit_pass1_args` and `fit_pass2_args(spec, kbps)`, plus the quality-pass args, built on
    `clip_args`'s input and stream parts;
  - a lanczos scale when the plan's size differs from the source, otherwise the existing
    even-crop;
  - AAC with `-ac 2` for more than 2 channels;
  - the R8 flags.
- [X] T019 [US1] `core/exec/fit.rs`: `encode(ctx, spec_args, plan) -> FitOutcome`:
  - **Budget mode:** stages `pass1`, `pass2`, `verify`; pass 1 runs with cwd = scratch.
  - **Quality mode:** one pass.
  - **Measuring:** the `.part` size is checked after each attempt, with the R2 retry formula, at
    most 3 retries, then `Error::FitFailed`.
  - **Progress:** from both passes (`restart_stage` on a retry).
  - **Spawning:** every process goes through `exec::process`.
  - **Test hook:** the `test-support` overshoot hook for T013.
- [X] T020 [US1] `core/exec/clip.rs`: when `spec.fit` is set, call `exec::fit::encode` instead
  of the single encode, then the existing verification plus R9's audio-offset check. Attach
  `FitOutcome` to `JobOutcome::Succeeded` (`fit`, skipped when `None`) in `core/event.rs`.
- [X] T021 [US1] `cli/main.rs`:
  - a `FitFlags` group (`--fit`, `--fit-max-size`, `--fit-max-length`, `--fit-allow-trim`),
    flattened into `clip`, with the exit-2 rules from contracts/cli.md;
  - `to_partial()` into `PartialOptions.fit`.

  `cli/render.rs` prints the result line.
- [X] T022 [US1] Make T011–T016 pass. Record the measured SC-001, SC-002 and SC-004 numbers in
  `checklists/validation.md`.

**Checkpoint**: MVP. `clip --fit` is guaranteed under the cap, with sound.

---

## Phase 4: User Story 2 - Length and resolution limits (Priority: P2)

**Goal**: too-long ranges and unreachable budgets are refused before any encode, or trimmed.
Sizes step down to the floor, and the picture is never enlarged.

**Independent Test**: quickstart's SC-005 rows and its "Trim, lowering" commands.

### Tests for User Story 2 (write first)

- [X] T023 [P] [US2] `core-tests/fit_us2.rs`, plan level (no encode), with a user target written
  to a temp config folder:
  - 3:00 against a 1:00 limit gives `RANGE_TOO_LONG` with `detail {len, max_length}`;
  - the same with `allow_trim` gives a plan of 60 s with `FIT_TRIMMED`;
  - an unreachable cap gives `BUDGET_UNREACHABLE` with `detail.min_cap` (R4's formula);
  - with `allow_trim` it trims to `len'`;
  - a tight cap gives `FIT_LOWERED` 1920×1080 → 1440×810.
- [X] T024 [P] [US2] `core-tests/fit_us2.rs`, a run: a trimmed range is the first 60 s (start ±1
  frame; `fit.trimmed = true`), and a lowered size encodes at the planned size. A 640×360 source
  is never enlarged, and a 4K source is fitted to 1920 on the long side.
- [X] T025 [P] [US2] `core-tests/fit_us2.rs`, sources:
  - HDR is tone-mapped to SDR `bt709` 8-bit (002's chain);
  - `VfrMp4` is capped at `max_fps` with the duration ±1 frame;
  - `FitSurround` gives 2 channels and the "extra channels" notice;
  - an audio-only source is refused (`PROBE_FAILED` "has no video stream", as for every clip; the
    probe refuses it before the fit is planned).
- [X] T026 [P] [US2] `cli-tests/fit.rs`: exit 3 for `RANGE_TOO_LONG` and `BUDGET_UNREACHABLE`,
  with the contracts/cli.md messages, and nothing written. An override above the target exits 2.

### Implementation for User Story 2

- [X] T027 [US2] `core/plan/planner.rs`: map `plan_fit`'s problems to issues:
  - `RANGE_TOO_LONG` and `BUDGET_UNREACHABLE` as errors with their `detail`;
  - `FIT_LOWERED` and `FIT_TRIMMED` as notices;
  - a trim shortens the `ClipSpec` (requested end, duration or trim end, `verify_video`), as
    `GifSpec::trimmed` does.
- [X] T028 [US2] Plain-clip HDR and fps:
  - `core/plan/ffargs.rs` adds 002's tone-map chain (`upscale::graph`) and `fps=` when the plan
    needs them;
  - `doctor`'s fit availability needs libx264, aac, and `zscale`/`tonemap` only for HDR sources
    (Principle VI, `ENGINE_MISSING` exit 4).
- [X] T029 [US2] Make T023–T026 pass (`core-tests/fit_us2.rs`, `cli-tests/fit.rs`).

---

## Phase 5: User Story 3 - Know the size before running (Priority: P3)

**Goal**: the dry run shows the target, its limits, the settings and the prediction, in text and
JSON. `--list-fit-targets` and `doctor` report the targets.

### Tests for User Story 3 (write first)

- [X] T030 [P] [US3] `cli-tests/fit.rs`:
  - `--dry-run` prints the contracts/cli.md line for budget mode, quality mode and a lowered
    size, and writes nothing;
  - `--dry-run --json` has `fit` exactly as contracts/json-output.md (the exact key set; the
    repo has no `jsonschema` setup);
  - `predicted_bytes`, compared with a real run, is within SC-006's 15% (measured: within 5%).
- [X] T031 [P] [US3] `cli-tests/fit.rs`: `clip --list-fit-targets` (text and `--json`), with a
  user target and a broken file; `doctor --json` has `fit.targets` and `fit.problems`, and its
  `status` is unchanged.

### Implementation for User Story 3

- [X] T032 [US3] `core/plan/model.rs`: add the `PlannedJob` `fit` JSON block (`FitPlan::to_json`).
  `cli/render.rs` prints the dry-run line, the `verified` flag, and the lowered and trimmed
  notes.
- [X] T033 [US3] `cli/main.rs` and `cli/commands/clip.rs`: `--list-fit-targets`. `core/doctor.rs`
  gets a `fit` block and the text line. Make T030–T031 pass.

---

## Phase 6: User Story 4 - Device shapes and batches (Priority: P4)

**Goal**: `upscale --fit`, `clip --upscale --fit`, JSON/CSV manifests and `batch`, with
per-level targets.

### Tests for User Story 4 (write first)

- [ ] T034 [P] [US4] `core-tests/fit_us4.rs`: `--preset reddit-916` with `reddit-video` gives
  1080×1920, reframed as 002 does, within a 15 MB cap. The preset's `limits` and
  `maxrate_for_limit` are not applied. On the GPU tier, when available (`require_engine!`), the
  two passes give the same size class.
- [ ] T035 [P] [US4] `core/input/manifest_json.rs` and `core/input/manifest_csv.rs` unit tests:
  - `fit` at every level, including `enabled: false` and `max_size` as a string or a number;
  - unknown keys refused;
  - `fit` with `gif` on one clip refused (exit 3);
  - the CSV columns `fit`, `fit_max_size`, `fit_max_length` and `fit_allow_trim`;
  - `fit = none`;
  - CSV and JSON give equal plans.
- [ ] T036 [P] [US4] `core-tests/fit_us4.rs`: a batch of 4 clips with the target at the top
  level, one override and one `enabled: false`. Every fitted output is within its own cap, and
  the unfitted one is unchanged. With `--mode smart`, `SMART_NOT_APPLIED` is noted and the cut is
  accurate.

### Implementation for User Story 4

- [ ] T037 [US4] `core/plan/planner.rs` and `core/upscale/`:
  - put a fit on `UpscaleSpec` (`UpscaleSpec.fit`), with the preset's size and fps as the
    planner's input;
  - `core/plan/ffargs.rs` `upscale_args` takes the fit's rate control and audio;
  - `core/exec/upscale.rs` calls `exec::fit::encode`;
  - the AI tier runs the pipeline in both passes (R7), and the dry-run time estimate doubles.
- [ ] T038 [US4] Manifests:
  - `core/input/manifest_json.rs`: the `fit` object;
  - `core/input/manifest_csv.rs`: the four columns;
  - `cli/commands/{upscale,batch}.rs`: `FitFlags`.

  Make T034–T036 pass.

---

## Phase 7: User Story 5 - Budgeted video in the desktop app (Priority: P5)

**Goal**: pick a target, see the prediction, queue, and see the result. The settings are
identical to the CLI's.

### Tests for User Story 5 (write first)

- [ ] T039 [P] [US5] `apps/desktop/src-tauri/tests/bridge.rs`:
  - `list_fit_targets` equals the CLI's JSON;
  - for the same manifest, `plan` gives a `fit` block equal to the core plan's (SC-007);
  - a run gives `fit` in the outcome;
  - `RANGE_TOO_LONG` comes back as a plan issue.
- [ ] T040 [P] [US5] `apps/desktop/src/**/*.test.ts` (vitest):
  - the target picker builds `PartialFit`, and "custom limits…" fills `max_size` and
    `max_length`;
  - choosing a target turns GIF off and shows the mode note;
  - a refusal disables "Add to queue";
  - the manifest round trip keeps only the changed keys.

### Implementation for User Story 5

- [ ] T041 [US5] `desk/ops/fit.rs` (a new module in `ops/`, re-exported): `list_fit_targets`.
  `desk/commands.rs` registers it; add it to the capability and permission files.
- [ ] T042 [US5] `ui/`:
  - a target picker in the clip panel, with the prediction (cap, limit, predicted size, notices)
    and the Allow-trimming toggle;
  - the queue shows `bytes of cap` and "trimmed", and the unverified badge;
  - **Settings › Targets** lists the video targets and their problems.

  TS types come from `ts-rs` (`VideoTarget`, `PartialFit`, `FitPlan`, `FitOutcome`). Make T039
  and T040 pass.
- [ ] T043 [US5] A manual desktop check on Xvfb `:99` (quickstart › Desktop): record the steps
  and screenshots in `checklists/validation.md`.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T044 [P] Docs:
  - `docs/api/cli-reference.md` (`--fit*`, `--list-fit-targets`, exit codes);
  - `docs/api/json-output.md` (the fit blocks);
  - `docs/reference/file-formats.md` (the manifest `fit` and video target files);
  - `docs/api/desktop-ipc.md`;
  - `docs/user/{cli-guide,desktop-guide}.md` (a "Fit a Reddit video" section);
  - `docs/administration/configuration.md` (the `video-targets/` folder);
  - `docs/architecture/domains.md` (a 005 section);
  - `docs/security/security-model.md` (target files, `-passlogfile` in scratch).
- [ ] T045 [P] Run quickstart.md end to end on Linux, and the Windows checks through CI and the
  MinGW type-check; record every SC result in `checklists/validation.md`.
- [ ] T046 Reviews, with findings recorded in `specs/005-budgeted-reddit-video/progress.md`:
  - a senior code review of `core/fit/`, `core/exec/fit.rs` and the planner changes;
  - a security review (target files read capped, ids validated, scratch-relative pass files, no
    shell).

  Fix the highs and mediums.
- [ ] T047 Release prep:
  - version 0.5.0 in the manifests;
  - `docs/releases/0.5.0.md`;
  - `docs/operations/fit-owner-checks.md` for SC-008 (a real upload with sound of a
    `reddit-video` output and a `reddit-916` output; the result sets `verified` in
    `reddit-video.json`, per roadmap item 3);
  - update the roadmap, CLAUDE.md and the Obsidian notes.

---

## Dependencies & Execution Order

- **Phase 1 → Phase 2 → US1** (MVP).
- **US2 and US3** need only US1's planner and executor. They can run in either order or in
  parallel, since they touch different test files; T027 and T032 both edit `planner.rs` and
  `model.rs`, so serialise those two.
- **US4** needs US1, plus US2's trims for manifests that trim.
- **US5** needs US3 (the plan JSON) and US4 (manifests).
- **Phase 8** comes last.

Within a story: tests (fail) → planner → args → executor → surfaces → tests pass.

## Parallel Examples

- **Phase 2:** T005, T006 and T009 together (different files); T004 before T005, since T005 uses
  `data_files`.
- **US1:** T011–T016 (all test files) together; then T017 → T018 → T019 → T020, with T021 in
  parallel with T019.
- **US2:** T023–T026 together.
- **US4:** T034–T036 together.

## Implementation Strategy

1. **MVP first:** Phases 1–3 give a guaranteed-size `clip --fit` with sound. Stop and validate
   SC-001 to SC-004 before going on.
2. **Then:**
   - US2 (refusals and trims) makes the guarantee complete;
   - US3 makes it visible;
   - US4 and US5 bring it to batches and the desktop app.
3. Each story ends with its tests green, the workspace and desktop suites green, no snapshot
   changes outside fit, and the MinGW Windows check clean.
