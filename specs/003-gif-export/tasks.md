---
description: "Task list for 003 GIF Export for Reddit"
---

# Tasks: GIF Export for Reddit

**Input**: Design documents from `specs/003-gif-export/`

**Prerequisites**: plan.md, spec.md, research.md (R1–R16), data-model.md (B1–B4), contracts/,
quickstart.md

**Status (2026-10-25)**: T001–T044 done: Phases 1 and 2, and US1 with its checkpoint (quickstart,
code and security reviews: `checklists/validation.md`). Next: US2 (T045–). Roadmap: `docs/roadmap.md`.

**Tests**: REQUIRED. Constitution Principle III (test-first with real media) is non-negotiable.
- Within each story, write the test tasks first and confirm they FAIL before implementing.
- Media comes from `rmedia-testkit` lavfi fixtures. No binary fixtures or engine binaries are
  committed.

**Organization**:
- Grouped by user story, in priority order US1 → US6.
- Phase 2 carries the engine supply chain and the 001/002 refactors B1–B4, which every story
  depends on.

## Format: `[ID] [P?] [Story] Description`
- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Paths are relative to the repo root.
- `core/` is shorthand for `crates/rmedia-core/src/`, `core-tests/` for
  `crates/rmedia-core/tests/`, and `cli-tests/` for `crates/rmedia-cli/tests/`.

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Record the 002 baseline in `specs/003-gif-export/checklists/validation.md`:
  - run `cargo test --workspace` (bundled engine, `RMEDIA_REQUIRE_FFMPEG=1`) and the desktop
    tests;
  - note the passing counts (300 / 10 ignored, desktop 16) and the insta snapshot list.

  The Phase 2 refactors must keep them green.
- [X] T002 Scaffold empty modules and register them in `core/lib.rs`:
  - `core/gif/{mod,target,timing,graph,budget,estimate,engines,verify}.rs`;
  - `core/gif/builtin/` (empty for now);
  - `core/exec/gif.rs`;
  - `crates/rmedia-cli/src/commands/gif.rs`, wired as a hidden `Gif` command until T044.
- [X] T003 [P] Add testkit fixtures in `crates/rmedia-testkit/src/lib.rs`:
  - `Land6` (testsrc2 1280×720 30 fps, 6 s);
  - `Busy10` (mandelbrot 1280×720 30 fps, 10 s, the US2 budget case);
  - `FlatUi` (white + `drawgrid` + `drawtext`-free bars, 6 s);
  - `Static6` (a single colour plus one small moving 8×8 block, 6 s);
  - `Alpha` (VP9 `yuva420p` WebM with a transparent margin, 3 s);
  - `Narrow` (640×360, 4 s).

  Reuse `MovingMarker`, `VfrMp4`, `Rotated90` and `HdrTagged`. Derived fixtures hash their
  parent's spec.
- [X] T004 [P] Add testkit helpers in `crates/rmedia-testkit/src/lib.rs`:
  - `gif_delays(path) -> Vec<u32>` (ffprobe `packet=duration`, time base 1/100);
  - `gif_packet_sizes`;
  - `gif_loop(path) -> Option<u16>` (NETSCAPE2.0 repeat count, `None` when absent);
  - `gif_opaque(path) -> bool` (the first frame decoded to rgba has no alpha < 255);
  - `mp4_faststart(path) -> bool`;
  - `require_tool!(gifsicle|gifski)`, which skips, but panics under
    `RMEDIA_REQUIRE_GIF_TOOLS=1`.

  Each helper gets a self-check test.
- [X] T005 [P] Write engine probes in `core-tests/gif_engine_checks.rs`. They pass immediately
  and guard against engine regressions (research R1, R3, R4, R15):
  - (a) two-pass palette output is byte-identical across runs;
  - (b) 15 fps gives only 6/7 cs delays, and 20/25/50 fps give exact delays;
  - (c) `-loop 0` writes NETSCAPE repeat 0, `-loop 2` writes 2, `-loop -1` writes no block;
  - (d) per-window `-ss` inputs joined with `concat` plus per-window `sendcmd` files with
    offsets move a `crop@trk` at the expected frames;
  - (e) the overlay-on-black flatten of `Alpha` is opaque;
  - (f) ffmpeg with cwd = scratch resolves `p-<key>.png`.

  Exact-delay and determinism asserts are gated to the bundled engine.

---

## Phase 2: Foundational (Blocking Prerequisites)

**⚠️ No user-story work starts until this phase is complete. All 001/002 tests and snapshots stay
green after every task.**

### Bundled engines: supply chain (contracts/gif-tools-lock.md)

- [X] T006 Write xtask tests, then implement the `xtask/gif-tools.lock.json` reader and
  `check-pins` rules in `xtask/src/main.rs`:
  - hex and https checks;
  - the `release` pattern, `build_run`, `build_commit`;
  - built-binary URLs under `releases/download/<release>/`;
  - a mirror for every source and for the vendor archive;
  - `--online` sends a HEAD request to every URL.

  Extend `all_pins`, `pins_of`, `archive_all` and `mirror` to include the new sources (the
  same-bytes rule for existing assets applies).
- [X] T007 Add `.github/workflows/gif-tools.yml` with the four jobs `vendor`, `build` (matrix),
  `attest` and `publish`, exactly as the contract describes:
  - permissions per job, commit-SHA-pinned actions, `concurrency`, timeouts;
  - https-only curl, verifying archives before extraction, refusing unsafe members;
  - refusing `.cargo/` and `rust-toolchain*`, and checking the lock hash;
  - `cargo vendor` in `vendor` only; the build runs `--frozen --offline` with a fresh
    `CARGO_HOME`;
  - hardening flags, and smoke tests on self-generated GIFs;
  - hashes passed through job `outputs`.

  If nmake fails, switch Windows gifsicle to the MSYS2/MinGW static route and record it in
  research R8.
- [X] T008 Implement `cargo xtask pin-gif-tools <run-id>` in `xtask/src/main.rs`:
  - check the run's workflow, success and `head_sha` on main;
  - run `gh attestation verify --signer-workflow` per asset;
  - compare the digests with the build job's outputs and with the downloads;
  - compare against the crates.io index checksum, and Debian's orig tarball when available;
  - run `cargo deny` (`deny-gifski.toml`) and `cargo audit` on gifski's lock;
  - write `cargo_lock_sha256`, generate `licenses/GIFSKI-DEPS.md`, and write the lock file.

  Unit-test the parsing and the rejection paths against recorded API fixtures (JSON text, not
  binaries).
- [X] T009 **Owner-approved step.** Run `gif-tools.yml` on `main`, which publishes a
  `gif-tools-<date>` release on the public repo. Then:
  - run `cargo xtask pin-gif-tools <run-id>` and `cargo xtask mirror`;
  - commit the lock;
  - check that `cargo xtask check-pins --online` passes.

  Ask before dispatching the workflow, because it publishes a release.

  Done 2026-10-25: run 40211502841 → `gif-tools-2026-10-25.40211502841` (the release is the
  mirror, so no separate `cargo xtask mirror`). On the way: MSYS tar needs POSIX paths, the
  smoke test must not pipe into `head` under pipefail, Windows gifsicle prints
  `LCDF Gifsicle 1.98 (Windows)`, Debian's orig tarball is a repository snapshot (compared by
  tree: 62 shared files identical), gifski's lock moves crossbeam-epoch 0.9.18 → 0.9.20
  (RUSTSEC-2026-0204, `lock_updates`), and imagequant (GPL-3.0-or-later) is allowed with the
  gate marker `commercial-gifski-imagequant` (owner decisions 2026-10-25).
- [X] T010 Stage the engines:
  - `fetch-tools` writes `binaries/rmedia-{gifsicle,gifski}-<triple>[.exe]` (verified, cached,
    stamped) and gains `--no-gifski`;
  - add both to `externalBin` in `apps/desktop/src-tauri/tauri.conf.json`, and to the
    CLI-archive copy list in `.github/workflows/release.yml`;
  - in `.github/workflows/ci.yml`, set `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI` and
    `RMEDIA_REQUIRE_GIF_TOOLS=1` for the bundled-engine test jobs, and leave them unset in
    `minimum-engine`.

  Done 2026-10-25. `release.yml` gains a `gifski` dispatch input (tag pushes bundle it, so the
  gate decides); without it, `fetch-tools --no-gifski` and a `tauri build --config` merge patch
  that drops the sidecar and its licences (Tauri merges by RFC 7396, so `null` removes a key).
  Both engines' licence files are bundled resources.
- [X] T011 [P] Add the notices:
  - `licenses/GIFSICLE-NOTICE.md`, `GIFSICLE-COPYING.txt`, `GIFSKI-NOTICE.md`,
    `GIFSKI-LICENSE.txt`, with the version, licence, upstream, mirrored corresponding source,
    the build command, and the separate-program statement;
  - `check-pins` rules for them and for `GIFSKI-DEPS.md`;
  - `cargo xtask notices` links all three from `THIRD_PARTY_NOTICES.md`.
- [X] T012 [P] Write xtask tests, then implement `cargo xtask check-release-gate`. It fails when
  `binaries/rmedia-gifski*` is staged unless both of these exist:
  - a root `LICENSE`;
  - `[workspace.metadata.rmedia] app_licence` from the allowlist.

  Call it first and again after staging in every `release.yml` job. Add a CI step that runs it
  against a temp tree with a fake staged file and expects failure (research R10).
- [X] T013 Run the Phase 0 measurements (after T010) and record the results in research R7 and
  R14:
  - gifski determinism (encode twice, compare);
  - gifski accepting 4:4:4 y4m;
  - gifsicle `--lossy` determinism;
  - x264 determinism at a fixed thread count.

  If gifski is not deterministic, apply the R14 fallback (`deterministic: false`, and narrow
  the byte-identity tests).

  Done 2026-10-25: every engine gave identical bytes on repeated runs (R14), and gifski accepts
  4:4:4 (R7). `deterministic` is now true for gifski, with a byte-identity test in
  `gif_gifski.rs`.

### Refactors B1–B4 (data-model.md › Refactors)

- [X] T014 Write tests for B4 in `cli-tests/contract_clip_json.rs` and `core/event.rs` tests:
  - the existing clip and upscale JSON snapshots are unchanged;
  - `JobWarning` without a code serialises as before;
  - `ErrorInfo` without `detail` serialises as before;
  - `PlannedJob::commands()` for a clip job equals `[args()]`;
  - the AI job's `pipeline` is unchanged.
- [X] T015 Implement B4:
  - `notices` on `PlannedJob`;
  - an optional `code` on `Event::JobWarning`;
  - `ErrorInfo.detail`;
  - `Error::BudgetUnreachable { fits_secs, best_bytes, min_bytes }`;
  - `Error::EngineFailed.tool`, with a `"ffmpeg"` constructor default;
  - `PlannedJob::commands()`.

  Files: `core/event.rs`, `core/error.rs`, `core/plan/mod.rs`, `crates/rmedia-cli/src/render.rs`.
  Then `cargo xtask gen-bindings`.
- [X] T016 Write B1 tests in `core-tests/snapshots_gif_graph.rs`, insta snapshots of
  `frame_chain` for:
  - centre, blur and smart framing;
  - tone-map;
  - alpha flatten;
  - a narrower source (W = crop width);
  - the fps cap at `min(target, source)`.

  Assert every `build()` snapshot from `snapshots_ffargs.rs` and the upscale tests is
  unchanged.
- [X] T017 Implement B1 in `core/upscale/graph.rs`: `Flavor::Gif` in `build_with`, plus
  `pub fn frame_chain` (research R1). It has one resample, no denoise, CAS, zscale or format
  tail, and an open output label.
- [X] T018 Write B2 tests in `core-tests/us2_reframe.rs` (extend):
  - the cache key of a 002 smart job is identical before and after;
  - `analyze_cached` and `lookup_cached` through `ReframeInput` return the same analysis as
    today.
- [X] T019 Implement B2:
  - `ReframeInput` in `core/upscale/reframe/mod.rs` with the fields from research R16;
  - `UpscaleSpec::reframe_input()`;
  - update `core/upscale/preview.rs` and `apps/desktop/src-tauri/src/ops.rs` callers.
- [X] T020 Write B3 tests in `core-tests/us6_pipeline.rs` (extend) and `core/exec/pipeline.rs`
  unit tests. They use small fake programs built in the test (no shell): a producer that writes
  y4m-like bytes, a consumer that reads stdin.
  - the 2-stage happy path;
  - consumer failure beats producer EPIPE;
  - producer failure discards the consumer's exit 0;
  - progress read from `progress.log`;
  - cancel kills both within 2 s on Linux and Windows.

  All existing 3-stage tests stay green.
- [X] T021 Implement B3: rewrite `core/exec/pipeline.rs` around
  `Vec<StageSpec { program, args, role, progress }>`, with the role-based blame rule, the
  progress sources and the kill order (research R7). The AI tier in `core/exec/upscale.rs`
  becomes a 3-stage instance.

### Probe, targets, timing, engines, options, verify

- [X] T022 [P] Write probe tests in `core-tests/probe_003.rs`:
  - `r_frame_rate`, `nb_frames`, `time_base`;
  - `has_alpha` (`Alpha` fixture);
  - `is_vfr()` (`VfrMp4` true, `Land6` false).
- [X] T023 Implement the probe additions in `core/probe.rs` (`RawStream` fields, helpers).
- [X] T024 [P] Write target registry tests in `core/gif/target.rs`:
  - built-ins validate against `specs/003-gif-export/contracts/gif-target.schema.json`
    (jsonschema dev-dependency);
  - user overrides by id are labelled "user override";
  - bad files go to `problems`, never fatal;
  - symlinks, files over 64 KiB and the 65th file are refused;
  - control characters in `name` are refused;
  - `soft ≤ hard ≤ 1 GiB`, and each floor ≤ the widths/fps.
- [X] T025 Implement `core/gif/target.rs` and `core/gif/builtin/{reddit-post,reddit-comment,max-quality,flat-ui}.json`
  (research R2 table, schema examples), reusing `read_preset_file`.
- [X] T026 [P] Write table-driven timing tests in `core/gif/timing.rs`:
  - exact rates 10/20/25/50;
  - approximate 12/15/24/30 with their average cs;
  - rejection above 50;
  - the predicted delay sequence equals the T005(b) measurement for 15 fps;
  - fps capped at the source rate.
- [X] T027 Implement `core/gif/timing.rs`.
- [X] T028 [P] Write engine lookup tests in `core/gif/engines.rs`:
  - the order explicit → env → bundled → PATH, with PATH only when nothing is bundled;
  - a missing or bad explicit path makes the engine unavailable with a reason;
  - with strict mode, `ENGINE_MISSING`;
  - the banner regex, the 5 s timeout, bounded output;
  - paths inside scratch, the input folder or the cwd are refused;
  - children get a cleared environment.

  The fake engines are small test programs.
- [X] T029 Implement `core/gif/engines.rs`, making `tools::find_tool` `pub(crate)` and adding
  an optional lookup mode in `core/tools.rs`.
- [X] T030 Write tests, then add:
  - `Settings` fields `gifsicle_path`, `gifski_path`, `gif_defaults` (backward-compatible load
    test) in `core/settings.rs`;
  - `KEY_FILTERS`/`KEY_ENCODERS` entries for palettegen, paletteuse, the gif muxer and libx264;
  - the `DoctorReport.gif` block in `core/doctor.rs`. GIF availability does not change
    `status` (contracts/cli.md).

  Regenerate the bindings.
- [X] T031 Write layering tests in `core/request.rs` (`#[cfg(test)]`):
  - `PartialGif` apply/diff over defaults → source → clip;
  - `enabled:false`;
  - `budget.soft` alone sets hard;
  - unknown keys rejected;
  - the name-template constants `GIF_NAME_TEMPLATE`, `GIF_CLIP_NAME_TEMPLATE` and
    `GIF_MULTI_CLIP_NAME_TEMPLATE`.

  Then implement `GifOptions`/`PartialGif` in `core/gif/mod.rs` and `PartialOptions.gif` in
  `core/request.rs`.
- [X] T032 [P] Write tests, then implement `core/gif/verify.rs`:
  - the GIF89a header;
  - the loop table (R15), including no block for 1 play;
  - delays ≥ 2, frame count ±1, dimensions, size ≤ hard;
  - for MP4: streams, faststart, dimensions, duration.

  The test inputs are generated by ffmpeg in the test.

**Checkpoint**: Foundation done. Run fmt, clippy and the full test suite (001/002 green), and a
security-auditor pass over T006–T012, T020–T021 and T028–T029 (supply chain, process spawning,
lookup).

---

## Phase 3: User Story 1 — A GIF that fits a Reddit post (Priority: P1) 🎯 MVP

**Goal**: `rmedia gif` with the built-in engine (plus gifski as the best-quality engine), with
dry run and estimate, progress, cancel and verify.

**Independent Test**: `Land6` with the defaults gives a looping GIF ≤ 8 MB, ≤ 720 px wide, at
15 fps, with every delay a whole number of cs ≥ 2.

### Tests for User Story 1 ⚠️ (write first, confirm they fail)

- [X] T033 [P] [US1] Write `core-tests/gif_us1.rs`, one test per acceptance scenario:
  - `Land6` defaults: ≤ 8 MB, 720×405, only 6/7 cs delays, loop forever, the outcome reports
    size, width, fps and duration;
  - `Narrow` keeps 640 with a note;
  - `--fps 30` is approximate (3–4 cs), and 51 is rejected;
  - a 1–3 s range is within one frame;
  - plays 1/3/forever follow the loop table;
  - `Alpha` gives an opaque output;
  - `HdrTagged` is tone-mapped;
  - `Rotated90` is rotated before scaling;
  - `VfrMp4` duration holds;
  - a range shorter than 2 frames is rejected;
  - a range over the target's `max_duration` plans with a `LONG_RANGE` notice (code and message)
    and still encodes;
  - a collision follows `--on-exists`.
- [X] T034 [P] [US1] Write cancel tests in `core-tests/cancel.rs` (extend): cancel during the
  palette and the encode stages. Within 2 s: no `.part`, no `.d`, no ffmpeg process. Linux and
  Windows.
- [X] T035 [P] [US1] Write CLI tests in `cli-tests/gif.rs`:
  - human and `--json` dry-run snapshots (the `gif` block, `notices`, `argv`/`pipeline`,
    "limits unverified");
  - a dry run writes nothing, and `--no-sample` gives `pending`;
  - `--list-targets` (human and JSON);
  - usage errors (`--mode`, `--container`, `--audio`, fps > 50, bad sizes);
  - exit codes 0/1/2/3/4/130, including `--budget 10k --no-sample` rejected at plan time with
    `BUDGET_UNREACHABLE` and `"detail":{"min_bytes":65536}` (exit 3, nothing written);
  - a GIF job whose predicted output exceeds free space warns per output folder, using
    `estimate::check_space` with a fake free-space function as
    `upscale/estimate.rs::free_space_shortfall_warns_per_folder` does (written in
    `core-tests/gif_us1.rs` over the new `plan::space_needs`: the CLI cannot fake free space);
  - `doctor` lines, with and without engines.
- [X] T036 [P] [US1] Write estimate tests in `core/gif/estimate.rs` and
  `core-tests/gif_estimate.rs`:
  - the window layout (10/50/90%, separate seeks);
  - ranges of 4 s or less are encoded whole and reused;
  - the first-frame + delta model against the real size on `Land6`, `FlatUi`, `Static6` and
    `Busy10` (record the error; the SC-004 assertion is in T053). Recorded 2026-10-25: +1.2%,
    −1.0%, +0.0%, −6.0%;
  - the sample scratch is removed.
- [X] T037 [P] [US1] Write the SC-005 test in `core-tests/quality.rs` (extend): the default
  `reddit-post` GIF of `Land6` scores ≥ a plain one-pass `ffmpeg -vf fps,scale` GIF of the same
  width, fps and similar bytes, on VMAF against the source frames.
  Done 2026-10-25 without the same-size part: 49.98 vs 48.81 VMAF, ours 1.56× the bytes (Land6
  is now tagged BT.709, and `vmaf_gif` compares both sides through RGB). T053 repeats it with
  the budget set to the plain GIF's size.
- [X] T038 [P] [US1] Write gifski tests in `core-tests/gif_gifski.rs`, behind `require_tool!(gifski)`:
  - `max-quality` uses gifski, through the 2-stage pipe;
  - a missing gifski falls back with `ENGINE_FALLBACK`, and `--strict-engine` exits 4;
  - `--colors` with gifski gives a note;
  - cancel during the pipe;
  - SC-010: VMAF(gifski) > VMAF(built-in) at the same width and fps on `Land6` and `Busy10`.

  Done 2026-10-25 with the pinned gifski 1.34.0. The cancel test lives in `core-tests/cancel.rs`
  (`cancelling_the_gifski_pipe_is_fast_and_clean`), with the other child-process checks; the
  strict-engine exit 4 is the CLI test in `cli-tests/gif.rs`. SC-010 at 480 px, 20 fps: Land6
  99.98 vs 98.80 VMAF, Busy10 84.63 vs 83.50.

### Implementation for User Story 1

- [X] T039 [US1] Implement the planning side:
  - `GifPlan`, `GifSpec` and `check_gif`/`plan_gif` in `core/gif/mod.rs`;
  - `JobSpec::Gif` arms everywhere (`core/plan/mod.rs` `args`/`commands`/`pipeline`/`resource`/
    `kind`/`mode`/`predicted`, `check_free_space`, `core/upscale/mod.rs` `prepare_plan`,
    `core/exec/mod.rs` `run_job`);
  - scratch allocation and accurate mode;
  - `TARGET_UNKNOWN`, the 2-frame minimum, the static 64 KiB floor;
  - notes and notices (`TIMING_APPROXIMATE`, `LONG_RANGE`, narrower source).
- [X] T040 [US1] Implement `core/gif/graph.rs`: the palette and encode argv on top of
  `frame_chain`, with `-f gif -fflags +bitexact`, the loop table, `-an -sn -dn -map_metadata -1`
  and relative names. Add insta snapshots of the argv.
- [X] T041 [US1] Implement `core/gif/estimate.rs`:
  - the sample builder (per-window `-ss` inputs, concat, per-window sendcmd hooks for US4);
  - the size model;
  - `predict()` for the no-budget case;
  - the dry-run prepare step that fills `GifPlan.estimate` in a temporary scratch folder
    (makes T036 pass).
- [X] T042 [US1] Implement `core/exec/gif.rs`:
  - stages palette → encode → verify with weights (data-model.md);
  - the size watchdog;
  - `JobMeasure::Gif`;
  - classified failures with tool attribution.

  Also `JobMeasure::Gif` in `core/event.rs` (makes T033/T034 pass).
- [X] T043 [US1] Implement the gifski engine in `core/exec/gif.rs`: a 2-stage pipe (ffmpeg y4m
  → gifski) through B3, the R14 parameters, fallback and strict mode, notes (makes T038 pass).
  Done 2026-10-25: BT.709 full-range 4:4:4 y4m with `--y4m-color-override bt709` (R7), progress
  from ffmpeg's `-progress file:`, and for 1 play the executor strips gifski's NETSCAPE block
  (R15). Samples for the estimate run through the same pipe. Running with gifsicle staged also
  showed that its `-O3` merges identical frames, so verify checks the display time after the
  post-pass (R11).
- [X] T044 [US1] Implement the CLI in `crates/rmedia-cli/src/commands/gif.rs` and `main.rs`:
  - the `GifFlags` group (reusable, without the range), with gif-only `-s/-e/-d`;
  - `--list-targets`, `--gifsicle`/`--gifski`;
  - render the plan block, summary and NDJSON in `render.rs`;
  - `doctor` lines in `commands/doctor.rs`.

  Un-hide the command (makes T035 pass).

**Checkpoint**: run `quickstart.md` › US1 and Doctor. Then fmt, clippy and tests on both OSes,
senior-code-reviewer, and security-auditor (process spawning, scratch, pipe).

---

## Phase 4: User Story 2 — Hit a size budget automatically (Priority: P2)

**Goal**: the bounded, deterministic budget search with the gifsicle post-pass, unreachable
handling and consented trimming.

**Independent Test**: `Busy10` to 3 MB: ≤ 3 MB, the lowered settings listed, ≤ 10 encodes.

### Tests for User Story 2 ⚠️

- [X] T045 [P] [US2] Write unit tests in `core/gif/budget.rs` against a fake `Sampler` with
  scripted sizes:
  - the ladder order (lossy → dither → colours → fps → width);
  - lossy rungs counted as `post`, not encodes;
  - ≤ 9 encodes in every scripted scenario (property test over random monotone size models);
  - the floor sample and `BUDGET_UNREACHABLE` with `fits_secs` (no double margin);
  - trimming with `allow_trim`;
  - interpolation;
  - a retry when the full encode misses soft;
  - `OVER_RECOMMENDED`;
  - skipped rungs at their floors;
  - determinism (the same script gives the same record).
- [X] T046 [P] [US2] Write gifsicle tests in `core-tests/gif_gifsicle.rs`, behind
  `require_tool!(gifsicle)`:
  - `-O3` and `--lossy` shrink the output;
  - the loop block is kept;
  - only generated names reach the argv;
  - a missing gifsicle skips the post-pass with `optimiser:false`, and `ENGINE_FALLBACK` only
    when a lossy rung was wanted.
- [X] T047 [P] [US2] Write `core-tests/gif_us2.rs`:
  - `Busy10 --budget 3MB` gives ≤ 3 MB, the `lowered` list in ladder order, ≤ 10 encodes;
  - two runs are byte-identical, with identical `search.steps`;
  - an unreachable budget writes nothing, fails the job (exit 1) and gives `fits_secs`;
  - `--allow-trim` trims and reports it;
  - soft/hard behaviour (`OVER_RECOMMENDED`);
  - `--budget` alone sets hard;
  - cancel during `search` is clean;
  - the dry-run estimate equals the predicted search result and lists the lowered settings.

### Implementation for User Story 2

- [X] T048 [US2] Implement `core/gif/budget.rs` (research R5), making T045 pass.
  Done 2026-10-25: `search(&SearchInput, &mut impl Sampler) -> Found` (async; the trait has
  `sample`, `post`, `full`). Reading of R5 where it is open: each rung lowers one step; s7 runs
  only when the floors fit soft; the retry takes the first rung after the last one used, else
  width to its floor. The property test runs 2000 seeded random models.
- [X] T049 [US2] Implement the gifsicle post-pass in `core/exec/gif.rs` (`run_lines_in`, cwd =
  scratch, `--` before the input, cleared environment, bounded stderr), making T046 pass.
- [X] T050 [US2] Wire the search into `core/exec/gif.rs`:
  - a `search` stage, plus `optimise`;
  - samples through the real `Sampler` (`core/gif/estimate.rs`), with palette reuse by key;
  - `BUDGET_UNREACHABLE` through `Error::BudgetUnreachable`;
  - the trim path;
  - `predict()` with a budget for dry runs.

  Makes T047 pass.

  Done 2026-10-25. The executor's `JobSampler` runs samples and post-passes in the scratch
  folder and full encodes through `Encoder` (on `GifSpec::trimmed` when the search trims); a
  retry restarts the encode stage. The dry run runs the same search with a predicting sampler
  (its "full encode" is the prediction made for those settings). gifski jobs take R14's
  `lossy_quality` rungs instead of dither and colours. Verify counts frames at the chosen rate.
  Land6 → 1 MB: 8 encodes, predicted 810 900 B, real 812 795 B; Busy10 → 3 MB: 2.74 MB.
- [X] T051 [US2] Update the CLI summary and JSON for `search`/`lowered`/`trimmed` in
  `crates/rmedia-cli/src/render.rs`, with snapshot updates in `cli-tests/gif.rs`.
- [X] T052 [US2] Add the SC-003 bench to `core-tests/bench.rs` (`#[ignore]`, asserted only when
  `available_parallelism() >= 12`): `Busy10` to the post budget in ≤ 3× the clip duration.
- [X] T053 [US2] Calibration tests in `core-tests/gif_calibration.rs`:
  - **SC-004:** estimate within 25% for ≥ 90% of a 10-clip lavfi set (fixtures × ranges);
  - **SC-002:** 100% ≤ hard, and ≥ 90% ≤ soft when achievable;
  - **SC-009:** the default `reddit-post` with gifsicle is ≥ 15% smaller, and loses ≤ 1 VMAF
    point;
  - **SC-005 at the same size:** the T037 test with `--budget` set to the plain GIF's bytes.

  Tune the `estimate.rs` constants and the targets' default `lossy` until these pass, and
  record the values in research R2/R4.

  Done 2026-10-25 (benches, `cargo test --release -- --ignored bench`):
  - SC-003 (T052): Busy10 → reddit-post in 23.8 s on 12 threads (bar 30 s), after the post-pass
    moved from `-O3` to `-O2` (50.6 s before; R2).
  - SC-004: 9/10 within 25% (MovingMarker +48%, the rest within ±5.2%).
  - SC-002: 20/20 under soft, none over hard (reddit-post and reddit-comment).
  - SC-005 at the same size: 98.32 vs a plain GIF's 66.23 VMAF (1.37 MB vs 1.68 MB).
  - **SC-009 not met on this set:** gifsicle saves 6.4% over the set at the default lossy 30,
    10.4% at 120, 12.3% at the maximum 200, never losing more than 0.76 VMAF. The bench asserts
    the VMAF bound and a provisional 5% floor; the 15% figure waits for the owner (validation.md).
  - The testkit's `vmaf_gif` now retimes GIF frames by their timestamps (gifsicle's merged
    frames had misaligned the comparison) and compares in 4:4:4; scores moved (T037: 99.63 vs
    98.47; SC-010: 99.98 vs 98.80, 84.63 vs 83.50).

**Checkpoint**: run `quickstart.md` › US2. Then fmt, clippy, tests and senior-code-reviewer.

---

## Phase 5: User Story 3 — Reddit MP4 (Priority: P3)

**Goal**: the silent, faststart H.264 output with the same range, framing and budget handling.

**Independent Test**: `Land6 --format mp4` is silent, faststart, playable, and smaller than its
GIF.

- [X] T054 [P] [US3] Write `core-tests/gif_us3.rs`:
  - video stream only, faststart, `avc1`, even dimensions, duration within one frame;
  - the source rate is kept up to `mp4.max_fps`;
  - budget: the CRF ladder runs before any width drop;
  - SC-006: ≥ 5× smaller than the GIF at the same width and fps on `Land6` and `Busy10`;
  - a `reddit-post` GIF dry run and outcome carry `mp4_bytes`/`mp4_estimate`, once per run;
  - without libx264, MP4 is `ENGINE_MISSING` while GIF still works (simulated through caps);
  - the reverse: without `palettegen`/`paletteuse`, GIF is `ENGINE_MISSING` while MP4 still
    works (simulated through caps).
- [X] T055 [US3] Implement the MP4 path in `core/gif/graph.rs` (encode argv), `core/gif/budget.rs`
  (the CRF rung list) and `core/exec/gif.rs` (stages), plus the MP4 hint sample in
  `core/gif/estimate.rs`.
- [X] T056 [US3] Update the CLI `--format mp4` rendering and the "MP4 would be smaller" line
  (snapshots).

  Done 2026-10-25. The MP4 runs through the same job: `GifChain.mp4` ends the frame chain in
  BT.709 limited `yuv420p`, `Settings.crf`, the search's CRF rung (+3 ×3, rate kept), one
  `encode` stage and `check_mp4` + fast start. The hint is target data (`mp4.hint`, on for
  `reddit-post`): one MP4 sample at the GIF's start size and rate, in the dry run and the
  outcome; the CLI prints it once per run. SC-006: 6.1× (Land6), 18.2× (Busy10) at 640 px,
  15 fps.

**Checkpoint**: run `quickstart.md` › US3. Then fmt, clippy, tests and senior-code-reviewer.

---

## Phase 6: User Story 4 — Portrait and phone-friendly GIFs via smart reframe (Priority: P4)

**Goal**: shapes and framing through 002's reframe, downscaled only.

**Independent Test**: `MovingMarker` as a 4:5 smart GIF keeps the marker in frame in ≥ 95% of
frames.

- [X] T057 [P] [US4] Write `core-tests/gif_us4.rs`, using `MarkerDetector` (the `test-support`
  feature):
  - 9:16, 4:5 and 1:1 with centre and blur give the right shapes and the portrait widths;
  - smart: marker in frame in ≥ 95% of frames (SC-004 of 002, measured as in `us2_reframe.rs`);
  - fps lowered by the search reuses the same analysis (cache hit count);
  - sample windows' crops match the full encode's at the same timestamps;
  - the no-subject fallback is reported;
  - an upscaled 1080×2340 input is downscaled to 360/480 and never enlarged.
- [X] T058 [US4] Implement in `core/gif/mod.rs` and `core/exec/gif.rs`:
  - shape and framing → `FramePlan` (geometry at the GIF size);
  - the `analyze` stage via `ReframeInput`;
  - sendcmd files for the full encode and for each sample window (offsets);
  - fallback reporting;
  - `--crop-path`.

  Done 2026-10-25. `FramePlan.smart` carries 002's analysis setup (face model, cache, analysis
  size, the whole-video command offset); the `analyze` stage (or the dry run, or `--crop-path`)
  runs `analyze_cached` on `GifSpec::reframe_input()` once, and every sample and encode reuses
  it. Samples get `sample.cmd` on the windows' joined timeline (`joined_analysis`). No subject:
  the crop stays centred (002's behaviour) with a note; nothing that fits: the blurred
  background, the analysis kept. Every command file starts with the opening crop (`sendcmd`
  refuses an empty file). MovingMarker 4:5: the marker inside in 100% of 150 frames.

**Checkpoint**: run `quickstart.md` › US4. Then fmt, clippy, tests and senior-code-reviewer.

---

## Phase 7: User Story 5 — Many GIFs at once and straight from a clip (Priority: P5)

**Goal**: manifests, CSV, `clip --gif`, `batch --gif`, and chained jobs in one pass.

**Independent Test**: a manifest with two videos and three clips, one of them `reddit-comment`,
gives three GIFs with the right budgets and leaves no intermediates.

- [X] T059 [P] [US5] Write JSON manifest tests in `core/input/manifest_json.rs` tests:
  - `gif` at the defaults, video and clip levels;
  - layering, `enabled:false`, unknown keys reported with the line;
  - a whole-video GIF without clips;
  - `container` ignored;
  - chained `gif` + `upscale`;
  - the writer round-trip.
- [X] T060 [P] [US5] Write CSV tests in `core/input/manifest_csv.rs` tests:
  - each `gif_*` column, `gif_target=off` exclusivity;
  - size syntax, booleans, enum error text with the row number;
  - optional `start` with `gif_*`;
  - the writer emits `gif_*` only when needed.
- [X] T061 [US5] Implement the manifest JSON/CSV readers and writers (`core/input/manifest_json.rs`,
  `core/input/manifest_csv.rs`), making T059/T060 pass.
- [ ] T062 [P] [US5] Write CLI tests in `cli-tests/gif_clip_batch.rs`:
  - `clip -s/-e --gif`, `clip -l list --gif` (two clips, two files via the clip template);
  - "--target needs --gif" (exit 2);
  - `clip --gif --upscale` gives one job, the note "upscale skipped", and no intermediate
    `.mp4`;
  - `batch --gif`, with `batch --dry-run` defaulting to no sample.
- [ ] T063 [US5] Implement `--gif` on `clip`/`batch` in `crates/rmedia-cli/src/main.rs` and
  `commands/{clip,batch}.rs`, and chained planning in `core/gif/mod.rs` (research R16), making
  T062 pass.
- [ ] T064 [US5] Write the integration test `core-tests/gif_us5.rs`, the spec's independent test:
  - 2 videos, 3 clips, one comment override: budgets correct;
  - one failure doesn't stop the others;
  - no leftovers.

**Checkpoint**: run `quickstart.md` › US5. Then fmt, clippy, tests and senior-code-reviewer.

---

## Phase 8: User Story 6 — Preview and tune in the desktop app (Priority: P6)

**Goal**: the GIF view with a live estimate, playback, the queue, and export parity.

**Independent Test**: open a video, pick a range and "Reddit comment"; an estimate appears, and
after encoding the GIF plays with its real size within the stated margin.

- [ ] T065 [P] [US6] Write media server tests in `apps/desktop/src-tauri/tests/media_server.rs`:
  `.gif` is served as `image/gif` with `nosniff`; ranges work.
- [ ] T066 [P] [US6] Write bridge tests in `apps/desktop/src-tauri/tests/bridge.rs`:
  - `list_gif_targets`;
  - `gif_estimate`: single flight (a second call kills the first), cancel, scratch removed on
    every path, stale scratch cleaned at startup, options re-clamped;
  - `open_output`: only this run's committed outputs, symlinks refused;
  - `set_settings` ignores page-supplied `gifsicle_path`/`gifski_path`;
  - `pick_engine` kinds;
  - `plan`/`run` with `options.gif`.
- [ ] T067 [US6] Implement in `apps/desktop/src-tauri/src/{commands,ops,media_server,lib}.rs`:
  `list_gif_targets`, `gif_estimate`, `cancel_estimate`, `open_output`, the `pick_engine` kinds,
  the `set_settings` copy-back, the gif content type, and pausing estimates during runs. Then
  `cargo xtask gen-bindings`.
- [ ] T068 [P] [US6] Write pure store tests in `apps/desktop/src/lib/stores/gif.test.ts`:
  options → `PartialGif`, the queue entry, the manifest export shape, the defaults from
  `gif_defaults`.
- [ ] T069 [US6] Implement `apps/desktop/src/lib/stores/gif.ts` and `gif.svelte.ts`:
  - the 400 ms debounce;
  - one estimate in flight, cancel;
  - `partial` estimates;
  - the trim confirmation, which re-queues with `allow_trim`;
  - "Export now" and "Add to queue".

  Add `gif.svelte.test.ts` with a mocked api.
- [ ] T070 [US6] Implement `apps/desktop/src/lib/components/GifView.svelte`:
  - the `"gif"` view in `stores/app.svelte.ts` and a tab in `Player.svelte`;
  - the target picker with "(limits unverified)";
  - format, shape and framing;
  - the overrides drawer with full CLI parity;
  - the estimate, and the result card (`<img>`/`<video>` from `open_output`, size, lowered
    settings, Trim to N s).
- [ ] T071 [US6] Write the parity test in `apps/desktop/src-tauri/tests/bridge.rs`: a desktop
  run, then `export_manifest`, then a CLI `batch` of that manifest gives byte-identical outputs
  (built-in engine; gifski per R14). This is SC-007's output half.
- [ ] T072 [US6] SC-007 interaction check under a private Xvfb (`:99`, never the user's display):
  choose the GIF tab, keep the default target, click Export now. That is 3 interactions, and the
  output appears. Record the result in `checklists/validation.md`.

**Checkpoint**: run `quickstart.md` › US6 (under Xvfb). Then npm check and test, the desktop
tests, senior-code-reviewer, and security-auditor (new IPC, media server, settings).

---

## Phase 9: Polish & Cross-Cutting Concerns

- [ ] T073 [P] Update the docs:
  - `docs/api/cli-reference.md`: `rmedia gif`, `--gif` on `clip`/`batch`, `doctor`, the
    lookup order;
  - `docs/api/json-output.md`;
  - `docs/api/file-formats.md`: the manifest `gif` block, `gif_*` columns, target files.
- [ ] T074 [P] Update the docs:
  - `docs/administration/configuration.md`: settings and `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI`;
  - `docs/api/desktop-ipc.md`;
  - `docs/user/{cli-guide,desktop-guide,troubleshooting}.md`.
- [ ] T075 [P] Update the docs:
  - `docs/architecture/{overview,domains}.md`: the GIF job kind and the N-stage pipe;
  - `docs/reference/glossary.md`: target, budget, sample, ladder, post-pass;
  - `docs/developer/{development-guide,testing}.md`: the new fixtures, `require_tool!`,
    `RMEDIA_REQUIRE_GIF_TOOLS`.
- [ ] T076 [P] Update the docs:
  - `docs/operations/build-and-release.md`: `gif-tools.yml`, `pin-gif-tools`, the gate,
    `--no-gifski`;
  - `docs/security/security-model.md`: the supply chain and engine spawning;
  - `docs/administration/installation.md`: licensing notices.
- [ ] T077 Write the release notes `docs/releases/0.3.0-dev.md`, update the `README.md` feature
  list, `docs/README.md` and `docs/roadmap.md`, and mark the Reddit limits "unverified" in the
  user docs (FR-007).
- [ ] T078 Run `cargo xtask notices` (no diff), `check-pins --online` and `check-release-gate`
  (expected to fail until the owner decides the licence; document it). Then run a
  `release.yml` `workflow_dispatch` with `--no-gifski` staging and record the artifacts for
  Linux and Windows.
- [ ] T079 Run all of `quickstart.md` on Linux, record the results, and the Windows results from
  CI and release artifacts, in `checklists/validation.md`. Owner checks stay open: Reddit's
  limits, a real Reddit upload, the licence decision.
- [ ] T080 Final senior-code-reviewer and security-auditor passes over all 003 changes, fixing
  the high and medium findings. Then:
  - mark the tasks `[X]`;
  - write `specs/003-gif-export/progress.md`;
  - update `docs/roadmap.md` and the `CLAUDE.md` active feature.

  Stop for review.

---

## Dependencies & Execution Order

- **Setup (T001–T005)** → **Foundational (T006–T032)** → user stories.
- **Inside Foundational:**
  - The supply chain T006 → T007 → T008 → T009 (owner approval) → T010 → T013. T011 and T012
    run in parallel after T006.
  - The refactors T014 → T015 → T016 → T017 → T018 → T019 → T020 → T021, sequential because
    they share `plan/mod.rs`, `graph.rs` and `exec/`.
  - T022–T032 can run alongside the refactors (separate files). T031 needs T015, and T032 needs
    T023.
  - The built-in engine work (US1 without gifski) does not need T009. Only T038/T043 and
    T046/T049 need the staged engines (T010). US1's built-in tasks may start once the refactors
    and T022–T032 are done.
- **US1 (T033–T044)** is the base for every story.
- **US2** needs US1. **US3** needs US2 (it reuses the search). **US4** needs US1 (it can follow
  US3, to keep `exec/gif.rs` edits sequential).
- **US5** needs US1–US4. **US6** needs US1–US5 (export parity covers every option).
- **Polish** after US6 (docs for US1–US5 may start after US5).

```
Setup → Foundational ─┬─ refactors + core types ─► US1 → US2 → US3 → US4 → US5 → US6 → Polish
                      └─ supply chain (T009 owner OK) ─► gifski/gifsicle tasks in US1/US2
```

## Parallel Examples
- Setup: T003, T004 and T005 together.
- Foundational: T022, T024, T026, T028 and T032 (tests) together while T014–T021 run in
  sequence; T011 and T012 alongside T007.
- US1 tests: T033–T038 together; then T039 → T040 → T041 → T042 → T043/T044.
- US2: T045–T047 together; then T048 → T049 → T050 → T051; then T052 and T053.
- US5: T059, T060 and T062 together.
- US6: T065, T066 and T068 together.

## Implementation Strategy
1. **MVP** = Setup + Foundational + US1 → `rmedia gif` with the built-in engine (gifski once the
   engines are pinned), verified and cancellable. Stop and validate against quickstart.md › US1.
2. Add US2 (the budget search, the feature's differentiator), then US3 and US4, each an
   independently testable CLI increment.
3. US5 batch and clip integration, then the US6 desktop view.
4. Every checkpoint: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace` green on both OSes, CI green on `main`, senior-code-reviewer, and
   security-auditor where noted.
5. Owner-gated items:
   - T009: publishing the engine release;
   - the app licence, which unblocks releasing with gifski;
   - confirming Reddit's limits;
   - a real Reddit upload.
