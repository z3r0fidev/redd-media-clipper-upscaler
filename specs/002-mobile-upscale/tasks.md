---
description: "Task list for 002 Mobile Upscaling & Smart Reframe"
---

# Tasks: Mobile Upscaling & Smart Reframe

**Input**: Design documents from `specs/002-mobile-upscale/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: REQUIRED. Constitution Principle III (test-first with real media) is non-negotiable.
Within each story, write the test tasks first and confirm they FAIL before implementing. Media comes
from `rmedia-testkit` lavfi fixtures; no binary fixtures or model files are committed.

**Organization**: Grouped by user story, in priority order US1 → US6. Phase 2 also carries the 001
refactors A1–A5, which every story depends on.

## Format: `[ID] [P?] [Story] Description`
- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Paths are relative to the repo root (see plan.md › Project Structure)
- `core/` below is shorthand for `crates/rmedia-core/src/`; `core-tests/` for
  `crates/rmedia-core/tests/`

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Record the 001 baseline: run `cargo test --workspace` with `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` set to the bundled engine and note the passing test count in `specs/002-mobile-upscale/checklists/validation.md` (the Phase 2 refactors must keep it green)
- [X] T002 Add workspace dependencies `tract-onnx = "0.23"` and `fs4 = "1"` to `Cargo.toml` `[workspace.dependencies]` and to `crates/rmedia-core/Cargo.toml`, and `jsonschema` as a rmedia-core dev-dependency (schema checks in T019/T063); update `apps/desktop/src-tauri/Cargo.lock`; regenerate `THIRD_PARTY_NOTICES.md` with `cargo xtask notices` so the CI notices check stays green; confirm `cargo deny check` passes for both lockfiles
- [X] T003 [P] Scaffold empty modules and register them in `core/lib.rs`: `core/preset/mod.rs`, `core/upscale/{mod,geometry,tier,capability,graph,encode,estimate}.rs`, `core/upscale/reframe/{mod,decode,cuts,detect,select,path,sendcmd,cache}.rs`, `core/exec/upscale.rs`; add the `test-support = []` feature to `crates/rmedia-core/Cargo.toml` plus a dev-dependency on itself with that feature (`rmedia-core = { path = ".", features = ["test-support"] }`) so integration tests in `core-tests/` can use `MarkerDetector`
- [X] T004 [P] Add `fetch-models` to `xtask/src/main.rs`: reads `xtask/models.lock.json` (name, `media.githubusercontent.com` URL, SHA-256, licence), downloads YuNet 2026may ONNX and a public-domain test face photo into `models/` (temp + rename, idempotent, SHA verified; research R14); add `models/` to `.gitignore` — *note: YuNet pinned to opencv_zoo commit 26cc381; the public-domain test face is added with its only user, the `#[ignore]` real-YuNet test (T050)*
- [X] T005 [P] Add `licenses/models/yunet-MIT.txt` and the public-domain notice for the test face; make `cargo xtask notices` include `licenses/models/` in `THIRD_PARTY_NOTICES.md` — *note: notices list each model in `models.lock.json` with its licence file; the public-domain notice comes with T050*
- [X] T006 [P] Add a `cargo xtask fetch-models` step (cached by lockfile hash) before tests in `.github/workflows/ci.yml` on both OSes; make 002 tests skip with a message through `testkit::require_engine(&[filters/encoders])` when the engine lacks them (the `minimum-engine` job on distro ffmpeg 6.1), while `RMEDIA_REQUIRE_FFMPEG=1` with the bundled engine turns a skip into a failure

---

## Phase 2: Foundational (Blocking Prerequisites)

**⚠️ No user-story work starts until this phase is complete. All 001 tests stay green after every task.**

### Refactors A1–A5 (data-model.md › Refactors)

- [X] T007 Write a contract test that pins 001's NDJSON/JSON output for clip jobs (dry-run job object and `job_finished` outcome with `actual`/`adjusted`) as insta snapshots, driven through the CLI (`rmedia clip … --dry-run --json` and a real run's NDJSON, since `job_doc()` is built in `crates/rmedia-cli/src/render.rs`) in `crates/rmedia-cli/tests/contract_clip_json.rs`, so A1 can prove the clip JSON is unchanged apart from the added `"measure":"clip"`
- [X] T008 A1: move `requested` and `predicted` from `PlannedJob` into `ClipSpec` (which already has `mode`; drop the duplicate on `PlannedJob`); drop the `Eq` derive from `ClipSpec` and `JobSpec` (`Span` and the upscale plan hold floats); define the `Tier` enum (`Cpu|Gpu|Ai`, serde + ts-rs) in `core/upscale/tier.rs` now, because `JobMeasure::Upscale` uses it; add `PlannedJob.media_len: Ts`; replace `JobOutcome::Succeeded { actual, adjusted }` with `Succeeded { output, measure: JobMeasure }` (`JobMeasure::Clip` flattened in JSON, `Upscale` variant per data-model.md) in `core/plan/mod.rs`, `core/event.rs`, `core/exec/clip.rs`, and update CLI/Tauri consumers in `crates/rmedia-cli/src/render.rs` and `apps/desktop/src-tauri/src/ops.rs`; regenerate bindings with `cargo xtask gen-bindings` (CI diff check) — *note: `requested` stays on `PlannedJob` (all job kinds have a span); `media_len()` stays a method over it; desktop `RunPanel` narrows on `measure === "clip"`*
- [X] T009 A5: extract `Planner::claim_output(source, template, vars, dir, on_exists) -> Result<(PathBuf, Disposition)>` from `plan_clip` in `core/plan/mod.rs`; existing collision tests keep passing
- [X] T010 Write tests for per-job scratch dirs in `core-tests/scratch.rs`: `JobCtx::scratch_dir()` is created as `.<stem>.rmedia-<run>-<job>.d/` (the `.part` naming with a `.d` suffix) next to the temp output, journaled, removed on success, failure and cancel, and removed by journal recovery after a simulated crash; recovery never follows a symlink planted at that name and never removes a directory that fails the naming check
- [X] T011 A2: in `core/exec/process.rs` add a `cwd` parameter to spawning and `run_stream(argv, consume: FnOnce(ChildStdout))` for uncapped binary stdout (today only the 16 MiB `run_capture` and text `run_lines` exist); implement `JobCtx::scratch_dir()` and `JobCtx::spawn_in(cwd, argv)` in `core/exec/mod.rs`; extend `journal::is_own_temp` to accept `.d` scratch dirs (same naming check, `symlink_metadata` must be a real directory) in `core/exec/journal.rs`, and add symlink-safe recursive removal to `commit::remove_with_retry` in `core/exec/commit.rs` (makes T010 pass)
- [X] T012 A3: add stage weighting (`JobCtx::stage(index, of, weight)` emitting `Event::JobStage`, with `job_progress.fraction` covering the whole job) and a `Resource` semaphore map (`Cpu` = `--jobs`, `Gpu` = 1) in `core/exec/mod.rs`, `core/exec/progress.rs` and `core/event.rs`; unit-test the weighted fraction and that two `Gpu` jobs never overlap
- [X] T013 A4: extend `Caps` with `filters: BTreeSet<String>` (from `-filters`), `models: ModelSet` (stub struct in `core/models.rs`, filled by T021) and `capability: Option<CapabilityRecord>` (placeholder type until US3); introduce `IssueCode` enum serialised as the existing strings plus `TIER_UNAVAILABLE`, `PRESET_UNKNOWN`, `HDR_UNSUPPORTED`, `LIMIT_EXCEEDED` in `core/plan/mod.rs`, `core/doctor.rs`, `core/error.rs`; doctor reports the 002 filters (zscale, cas, hqdn3d, bwdif, libplacebo, sendcmd, scdet, gblur, tonemap) and `libx265`

### Probe, fixtures, engine checks

- [X] T014 [P] Add testkit fixtures in `crates/rmedia-testkit/src/lib.rs`: `CleanPortrait` (1080×1920 testsrc2 + sine, near-lossless) and, derived from that file by `scale` + x264 CRF 35 so the content is identical, `DegradedPortrait` (480×854) and `DegradedPortrait360` (360×640); `Portrait480Long` (480×854, 60 s 30 fps, for the SC-003 bench); `ChaptersMp4` (two chapters and a title tag); `MovingMarker` (1280×720 gray + 80×80 magenta moving by `t`, hard cut at 6 s to a second shot; research R7), `FarMarkers` (two markers 1000 px apart), `Rotated90` (display-matrix rotation), `Anamorphic` (720×576 SAR 64:45), `Interlaced` (tff), `HdrTagged` (10-bit HEVC tagged smpte2084/bt2020), `Landscape1080` (60 s 1080p30 for benches), `TinySource` (320×240), `HighFps120` (120 fps, 5 s) — *note: `Landscape1080` reuses the existing `Hd60Mp4` (1920x1080, 30 fps, 60 s); derived fixtures hash their parent's spec*
- [X] T015 [P] Add `testkit::vmaf(distorted, reference) -> f64` (libvmaf through the bundled ffmpeg; asserts both files have the same displayed aspect within 1%, then scales the reference to the distorted size with bicubic) and `testkit::bicubic_baseline(src, w, h) -> PathBuf` in `crates/rmedia-testkit/src/lib.rs`, and `testkit::require_engine(&[&str]) -> bool` (skip guard, see T006), with a self-check test (identical files score ≥ 99) — *note: `require_engine` landed with T006; `bicubic_baseline` takes an output path*
- [X] T016 Write probe tests against T014 fixtures for `rotation`, `sar`, `field_order`, colour tags, `display_size()`, `is_hdr()`, `is_interlaced()` in `core/probe.rs` `#[cfg(test)]`; confirm they fail
- [X] T017 Implement the probe additions (display matrix side data, `sample_aspect_ratio`, `field_order`, `color_*`) and helpers in `core/probe.rs`
- [X] T018 [P] Write engine probes in `core-tests/engine_checks.rs` (they check engine behaviour the design relies on, so they pass immediately and guard against engine regressions rather than failing first): (a) a `sendcmd` file toggling `overlay@fb enable` samples red/blue/red at 1/3/5 s (research R5); (b) the same with `VfrMp4` and with a leading `trim,setpts`; (c) `crop@trk x` commands move the crop; (d) ffmpeg spawned with cwd = scratch dir resolves `f=reframe.cmd`

### Presets and settings

- [X] T019 [P] Write preset tests in `core/preset/mod.rs` `#[cfg(test)]`: every built-in validates against `contracts/preset.schema.json`; validation table (odd size, crf out of range, `hvc1` with H.264, `hdr_capable` with H.264); user file overrides a built-in by `id`; an invalid user file is reported as a problem and skipped; `default_id() == "phone-fill"`
- [X] T020 Add the seven built-in presets from research R1 as `core/preset/builtin/{phone-fill,phone-fill-hd,pixel-tall,reddit-916,reddit-feed-45,fold-inner,duo}.json`, and implement `Preset`, `PresetRegistry::load(config_dir)`, `get`, `list`, `default_id` in `core/preset/mod.rs`
- [X] T021 [P] Add `Settings.models_dir` and `Settings.upscale_defaults: PartialUpscale` (stub type until T031) and a `models::locate(name, &Settings) -> Option<PathBuf>` lookup (settings → bundled `models/` beside the exe → `<cache root>/models`; research R14) in `core/settings.rs` and a new `core/models.rs`, with tests for the lookup order; add `cache_root()` with an `RMEDIA_CACHE_DIR` override (`preview/`, `reframe/` and `models/` live beneath it; today's `cache_dir()` becomes `cache_root()/preview`) in `core/settings.rs` (`RMEDIA_CONFIG_DIR` and `RMEDIA_STATE_DIR` already exist), and make every test that touches presets, the capability record or caches use temp directories through them — *note: the `UpscaleOptions`/`PartialUpscale` data types land here (Settings stores one); layering (`apply`/`diff`) is T031*

**Checkpoint**: 001 test count from T001 still passing; contract snapshot from T007 unchanged except `measure`; `rmedia doctor` lists the new filters.

---

## Phase 3: User Story 1 — Crisp, full-screen portrait output (P1) 🎯 MVP

**Goal**: `rmedia upscale IN` turns a soft portrait video into a verified 1080×2340 HEVC `hvc1` BT.709 file on the CPU tier, with center/blur reframe, estimates, dry run and cancel.

**Independent Test**: quickstart.md › US1. `DegradedPortrait` → 1080×2340, hvc1, bt709/tv, source unchanged, VMAF ≥ bicubic + 3.

### Tests for US1 (write first, confirm they fail)

- [X] T022 [P] [US1] Geometry table tests in `core/upscale/geometry.rs` `#[cfg(test)]`: 480×854 → phone-fill (horizontal overflow crop), 1920×1080 → portrait crop window, 9:21 source (vertical overflow), square, rotated 90°, anamorphic SAR, source ≥ target (`no_upscale_needed`), all dimensions even; aspect tolerance: a difference under 1% (480×854 vs 9:16) is a match, handled by a central crop of the rounding pixels with reframe `not_needed`
- [X] T023 [P] [US1] Encode-args snapshot tests in `core/upscale/encode.rs` `#[cfg(test)]`: phone-fill HEVC (research R2 argv incl. `-tag:v hvc1`, x265 colour params, explicit `-colorspace/-color_primaries/-color_trc/-color_range`), reddit-916 H.264 level 4.2 + `+faststart`, taller H.264 levels 5.0/5.1, `--crf` override, `hdr=keep` Main10 tags, `-maxrate/-bufsize` when a size limit applies, pinned `-threads`/`pools=` (R11)
- [X] T024 [P] [US1] Filter-graph snapshot tests (insta) in `core/upscale/graph.rs` `#[cfg(test)]` for the CPU tier: center crop, whole-video blur-pad, no-upscale (plain downscale, no `cas`), leading trim, interlaced (`bwdif`), anamorphic (`setsar`), HDR→SDR tone-map, untagged ≤ 576 lines (BT.601 `matrixin`), `fps=60` only above 60 fps
- [X] T025 [P] [US1] Estimate tests in `core/upscale/estimate.rs` `#[cfg(test)]`: size formula (research R12), time from a given fps, `LIMIT_EXCEEDED` for reddit-916 over 1 GB or 15 min, free-space shortfall warning using an injected free-space value
- [X] T026 [P] [US1] Planner tests in `core-tests/us1_upscale_plan.rs`: default output `<dir>/upscaled/<stem>_phone-fill.mp4`; `{preset}` name variable; non-mp4 `--container` warns and is forced to mp4; unknown preset → `PRESET_UNKNOWN`; `hdr=keep` with reddit-916 → `HDR_UNSUPPORTED`; forced `gpu`/`ai` before US3 → `TIER_UNAVAILABLE`; `TinySource` warns; `smart` before US2 lands plans as blur with a warning; output can never equal the source; `--on-exists` skip, rename and overwrite for upscale outputs; an injected `Caps` without `libx265` (or a needed filter) → `ENGINE_MISSING` (exit 4)
- [X] T027 [P] [US1] End-to-end tests in `core-tests/us1_upscale.rs`: `DegradedPortrait` default → ffprobe `1080,2340,hvc1,bt709,bt709,bt709,tv`, source sha256 unchanged, AAC audio copied; reddit-916 → 1080×1920 H.264 with moov before mdat; an already-large source → `no_upscale_needed` and downscale only; `MultiMkv` text subs → `mov_text` with a warning for anything dropped; `ChaptersMp4` keeps its chapters and title tag; `SmallWebm` Opus audio is re-encoded to AAC 192k (FR-020); outcome `measure: "upscale"` with `tier_used: "cpu"`; the same CPU job run twice gives an identical sha256 (Principle II)
- [X] T028 [P] [US1] Cancel test in `core-tests/cancel.rs`: cancel an upscale job after 1 s → finished ≤ 2 s, no `.part`, no scratch dir, no `rmedia-ffmpeg` child left (SC-006)
- [X] T029 [P] [US1] Quality and speed tests in `core-tests/quality.rs`, run with the `reddit-916` preset and `--reframe center` (1080×1920, the same 9:16 aspect as the sources, so no crop): for `DegradedPortrait` (480p) and `DegradedPortrait360`, VMAF(T0 output vs `CleanPortrait`) ≥ VMAF(bicubic baseline encoded with the same libx264 CRF 18) + 3 (SC-002 CPU); `#[ignore]` bench: `Portrait480Long` → phone-fill with `--reframe center` (so no smart analysis is timed) in ≤ 2× duration (SC-003), added to `core-tests/bench.rs` — *note: automated bar is +1.5 VMAF on the synthetic fixtures (owner decision 2026-10-14); measured +2.78 (480p) and +2.89 (360p); SC-003 bench measured 101.5 s for 60 s*
- [X] T030 [P] [US1] Edge-case end-to-end table in `core-tests/us1_edge.rs`, each row checked with ffprobe: `Rotated90` → upright at the target size; `Anamorphic` → SAR 1; `Interlaced` → progressive; `HdrTagged` with `hdr=sdr` → bt709 tags, and with `hdr=keep` on phone-fill → Main10 with the source tags; `VfrMp4` → frame timing kept; `HighFps120` → ≤ 60 fps; `TinySource` → the tiny-source warning; an SDR source with `hdr=keep` stays SDR with bt709 tags

### Implementation for US1

- [X] T031 [US1] Implement `UpscaleOptions`, `Reframe`, `TierChoice`, `HdrMode`, `Motion`, `PartialUpscale` (serde, ts-rs, `deny_unknown_fields`) and `Options.upscale` / `PartialOptions.upscale` with field-by-field `apply`/`diff` and the `enabled` semantics from data-model.md in `core/upscale/mod.rs` and `core/request.rs`
- [X] T032 [US1] Implement `geometry::plan(display, target) -> Geometry` in `core/upscale/geometry.rs` (makes T022 pass)
- [X] T033 [P] [US1] Implement `encode::args(&Preset, &UpscaleOptions, &ColourTags, limits) -> Vec<OsString>` in `core/upscale/encode.rs` (makes T023 pass)
- [X] T034 [US1] Implement `graph::build(&GraphInput) -> String` for pre-processing, the T0 scaler (`zscale` spline36 → `cas=0.5`), center crop and blur-pad branches in `core/upscale/graph.rs` (makes T024 pass) — *note: scaler is zscale Lanczos + `cas=0.7` (research R3 measurements); the blurred background is colour-converted too, and overlay stays 10-bit when HDR is kept*
- [X] T035 [P] [US1] Implement `estimate::estimate()` and `estimate::check_space(dirs)` with `fs4` in `core/upscale/estimate.rs` (makes T025 pass)
- [X] T036 [US1] Split `ffargs::clip_args` into `input_args`, `map_args`, `tail_args` and add `upscale_args(&UpscaleSpec, output)` that emits `-filter_complex`, `-map [v]`, the stream maps (with `-map_chapters 0`, unlike clips) and encode args in `core/plan/ffargs.rs`; existing `snapshots_ffargs` stay identical
- [X] T037 [US1] Add `JobSpec::Upscale(UpscaleSpec { cut, up })`, `UpscalePlan`, and `Planner::plan_upscale` (whole file = accurate clip with no seek; preset lookup; geometry; tier = CPU; estimate; limit warnings; `upscaled/` default dir; `{source}_{preset}.{ext}` template; `{preset}` variable in `core/plan/naming.rs`) plus the engine check against `caps.encoders`/`caps.filters` (`ENGINE_MISSING`), and a core serialiser `UpscalePlan::to_json()` for the dry-run `upscale` block shared by the CLI and the desktop (Principle I), in `core/plan/mod.rs` and `core/upscale/mod.rs`; regenerate bindings (makes T026 pass) — *note: `plan_with` takes the preset registry (tests use the built-ins); paths are made absolute because ffmpeg runs in the scratch dir; AAC is copied only for whole videos*
- [X] T038 [US1] Implement the upscale executor in `core/exec/upscale.rs` and dispatch it from `run_job` in `core/exec/mod.rs`: render stage (ffmpeg with cwd = scratch dir, `-progress` parsed into weighted progress) → verify stage (size, codec/tag, colour tags, duration within 1 frame + 50 ms) → commit (makes T027, T028 pass)
- [X] T039 [US1] Add `rmedia upscale <INPUT|DIR>... [--inputs-from]` with `--preset --reframe --tier --hdr --motion --crf --no-analyze --crop-path` and `--list-presets [--json]` in `crates/rmedia-cli/src/commands/upscale.rs`, registered in `crates/rmedia-cli/src/main.rs`; reject `--mode`; `job_doc()` in `crates/rmedia-cli/src/render.rs` adds `kind` and the core `upscale` block; human dry-run lines per contracts/cli.md — *note: `--crop-path`/`--no-analyze` are accepted with a note until US2*
- [X] T040 [US1] CLI tests in `crates/rmedia-cli/tests/upscale.rs`: dry-run `--json` job matches contracts/json-output.md (`kind`, `upscale` block, `cwd`, `argv`); `--list-presets --json`; exit 3 for unknown preset and `--hdr keep` with reddit-916; a dry run creates no `upscaled/` folder, no `.part` and no scratch dir
- [X] T041 [US1] Make T029 pass (tune `hqdn3d`/`cas` only within research R3 values if the VMAF margin is short, and record the measured numbers in `specs/002-mobile-upscale/research.md` R3) — *note: tuning stayed inside research R3's range; see the R3 table*

**Checkpoint**: quickstart.md › US1 passes manually. senior-code-reviewer pass over Phase 2 + US1; security-auditor on the upscale executor (spawn with a working directory, scratch-dir lifecycle, `--crop-path` writes, output path derivation); fix findings. Commit.

---

## Phase 4: User Story 2 — Landscape to portrait that follows the action (P2)

**Goal**: smart reframe: cut-aware, face-tracked, smoothed horizontal crop path with blur-pad fallback ranges, shown in dry run and applied in one pass.

**Independent Test**: quickstart.md › US2. `MovingMarker` output keeps the marker inside ≥ 95% of frames, never out > 0.5 s, steps ≤ limit except at the cut; `FarMarkers` produces a listed fallback range.

### Tests for US2 (write first, confirm they fail)

- [X] T042 [P] [US2] Path tables in `core/upscale/reframe/path.rs` `#[cfg(test)]`: median(5), deadzone ±6%, interpolation to the output grid, speed/accel clamp for calm/normal/fast (research R6), reset at a cut, clamp to frame, even snapping; property check that within a shot no step exceeds the speed limit and no change in step exceeds the acceleration limit — *note: following uses the stoppable-speed rule (no overshoot); after even snapping steps may exceed the limit by 2 px*
- [X] T043 [P] [US2] Selection tables in `core/upscale/reframe/select.rs` `#[cfg(test)]`: largest face, group rule (≥ 50% height, within 1.5 crop widths), union wider than 0.9 × crop → no fit, runs ≥ 1 s → fallback range clipped to the shot, shorter gaps hold the last position, faceless shot → whole-shot fallback — *note: subject rule amended in research R6 (faces under half the largest are ignored; any subject beyond 1.5 crop widths → no fit)*
- [X] T044 [P] [US2] Cut tests in `core/upscale/reframe/cuts.rs` `#[cfg(test)]`: parse `metadata=print` scdet output; histogram difference catches a cut between samples; shots < 1 s are kept
- [X] T045 [P] [US2] Golden `reframe.cmd` text and `keyframes(10)` tests in `core/upscale/reframe/sendcmd.rs` `#[cfg(test)]` (crop x commands, `overlay@fb enable` toggles at fallback ranges, times relative to the clip)
- [X] T046 [P] [US2] Cache tests in `core/upscale/reframe/cache.rs` `#[cfg(test)]`: key changes with each R10 input (mtime, span, motion, algo version, model SHA); JSON round trip; LRU eviction at 500; a corrupt entry is ignored and rewritten
- [X] T047 [P] [US2] YuNet post-processing tests in `core/upscale/reframe/detect.rs` `#[cfg(test)]`: decode synthetic output tensors into boxes, score threshold 0.6, NMS IoU 0.3; `MarkerDetector` finds the magenta box on a generated frame — *note: FarMarkers asserts both markers over the blurred background; SC-004 measured 100% of 300 frames inside*
- [X] T048 [P] [US2] End-to-end tests in `core-tests/us2_reframe.rs` using `MarkerDetector`: `MovingMarker` → output frames analysed with `MarkerDetector` meet SC-004; the crop jumps only at the 6 s cut; `FarMarkers` → outcome lists a fallback range and those frames show the blurred background; `--reframe center` and `blur` apply to the whole video; missing YuNet → blur with the warning "face model not found; using blurred background"; the smart-reframe render is byte-identical across two runs, one with a cold and one with a warm analysis cache (Principle II)
- [X] T049 [P] [US2] Dry-run tests in `crates/rmedia-cli/tests/upscale.rs`: `reframe_analysis` block (`status`, shots, fallback ranges, 10 Hz keyframes); `--no-analyze` → `pending`; `--crop-path FILE` writes the per-frame path JSON; a second dry run hits the cache (no decode process spawned) — *note: SC-005: analysis 10.7 s vs centre render 79 s (13.5%); real YuNet follows the NASA portrait*
- [X] T050 [P] [US2] `#[ignore]` tests: real YuNet on the fetched public-domain face panned across the frame keeps the face in frame (`core-tests/us2_yunet.rs`); SC-005 bench, analysis adds ≤ 50% on `Landscape1080` (`core-tests/bench.rs`) — *note: test face: NASA JSC2003-E-041874 (public domain), `test_only` in models.lock.json; tests read the workspace `models/` explicitly because release builds never search it*

### Implementation for US2

- [X] T051 [US2] Implement the analysis decode in `core/upscale/reframe/decode.rs`: one ffmpeg process (`<pre>,scale=576:-2,split`; `scdet` + `metadata=print:file=cuts.txt` in a temp dir under `<cache root>/reframe/tmp/`, never beside the output, so dry runs write nothing there; `fps=3,format=bgr24` rawvideo to stdout read frame by frame with `run_stream`; no `-progress` (progress = frames received / expected)), cancel-aware
- [X] T052 [P] [US2] Implement cut detection in `core/upscale/reframe/cuts.rs` (makes T044 pass)
- [X] T053 [P] [US2] Implement the `Detector` trait, `YunetDetector` (tract-onnx, input sized at runtime, model SHA exposed) and `MarkerDetector` (behind the `test-support` cargo feature of rmedia-core) in `core/upscale/reframe/detect.rs`; run detection on `min(cpus, 4)` `std::thread::scope` workers with in-order reassembly (makes T047 pass)
- [X] T054 [P] [US2] Implement subject selection and fallback ranges in `core/upscale/reframe/select.rs` (makes T043 pass)
- [X] T055 [P] [US2] Implement the path pipeline in `core/upscale/reframe/path.rs` (makes T042 pass)
- [X] T056 [P] [US2] Implement `to_sendcmd()` and `keyframes(hz)` in `core/upscale/reframe/sendcmd.rs` (makes T045 pass)
- [X] T057 [P] [US2] Implement the analysis cache under `<cache>/reframe/` in `core/upscale/reframe/cache.rs` (makes T046 pass)
- [X] T058 [US2] Implement `reframe::analyze(source, span, &opts, detector, cancel, progress) -> ReframeAnalysis` tying T051–T057 together, and the `ReframeAnalysis` type (ts-rs) in `core/upscale/reframe/mod.rs`
- [X] T059 [US2] Add the smart branch to `graph::build` (`sendcmd,crop@trk`, `overlay@fb enable=0`, fallback branch omitted when there are no ranges) with new insta snapshots in `core/upscale/graph.rs` — *note: analysis stage weights 0.25/0.7/0.05; whole-clip fallback renders the plain blur graph*
- [X] T060 [US2] Wire analysis into planning and execution: `AnalysisState` in `UpscalePlan`; dry run analyses unless `--no-analyze`; the executor runs the `analyze` stage (weight 0.25) when the plan is `Pending`, writes `reframe.cmd` to the scratch dir, and reports `fallback_ranges` in the outcome; remove the T026 "smart → blur" interim rule, in `core/plan/mod.rs` and `core/exec/upscale.rs` (makes T048 pass) — *note: `--crop-path` needs exactly one smart job (exit 2 otherwise) and also works without `--dry-run`*
- [X] T061 [US2] CLI: `--crop-path` writer, shots and fallback ranges in the human dry run (`mm:ss–mm:ss`) in `crates/rmedia-cli/src/commands/upscale.rs` and `crates/rmedia-cli/src/render.rs` (makes T049 pass)

**Checkpoint**: quickstart.md › US2 passes. senior-code-reviewer pass; security-auditor on model loading (tract input from a user-settable `models_dir`) and the analysis cache files. Commit.

---

## Phase 5: User Story 3 — Use the GPU automatically, with a choice (P3)

**Goal**: capability record, Vulkan/libplacebo GPU tier, auto/forced selection, one CPU fallback with demotion, doctor tiers.

**Independent Test**: quickstart.md › US3. On this laptop auto picks the Intel iGPU and rejects llvmpipe; an injected GPU failure falls back once and demotes; forced `gpu` without a device exits 3.

### Tests for US3 (write first, confirm they fail)

- [X] T062 [P] [US3] Listing-parser tables in `core/upscale/capability.rs` `#[cfg(test)]` from captured text: Intel iGPU + llvmpipe (research R8), discrete NVIDIA/AMD lines, lavapipe and SwiftShader, Mesa `FINISHME` noise, no Vulkan at all; preference discrete > integrated
- [X] T063 [P] [US3] Fingerprint and record tests in `core/upscale/capability.rs` `#[cfg(test)]`: fingerprint changes when an ICD JSON or its library mtime changes (temp dirs injected), or the app/ffmpeg version changes; record round-trips and validates against `contracts/capability-record.schema.json`; atomic write under an `fs4` lock; a stale fingerprint clears demotions — *note: the lock is std `File::lock` on a `capability.lock` sidecar (research R8), not fs4*
- [X] T064 [P] [US3] `tier::select` tables in `core/upscale/tier.rs` `#[cfg(test)]`: auto → gpu when available and not demoted, otherwise cpu with a reason; forced unavailable → `TIER_UNAVAILABLE`; auto never returns ai; `suggest: ai` when AI is available and the source is under 720p — *note: auto also skips a GPU whose self-test is slower than the CPU's (SC-003 amended 2026-10-19)*
- [X] T065 [P] [US3] GPU-tier graph snapshots in `core/upscale/graph.rs` `#[cfg(test)]`: `-init_hw_device vulkan=vk:N`, `hwupload,libplacebo=…:upscaler=ewa_lanczossharp,hwdownload`, `tonemapping=bt.2390` for HDR→SDR — *note: `ewa_lanczossharp:sigmoid=0` then `cas=0.7` when enlarging (VMAF parity with the CPU tier)*
- [X] T066 [P] [US3] Fallback tests in `core-tests/us3_tiers.rs`: an auto GPU decision pinned to a non-existent device index (simulates a vanished GPU) fails with `ENGINE_FAILED`, is retried once on CPU, emits a `job_warning`, records `tier_fallback: true`, and writes a demotion; the next plan picks CPU (SC-008); a forced GPU with the same failure does not fall back; disk-full and cancel never trigger the fallback — *note: demotion only after the CPU retry succeeds, with a fixed text; progress stays monotonic*
- [X] T067 [P] [US3] Vulkan-gated tests (skip with a message when the listing shows no usable device) in `core-tests/us3_gpu.rs`: GPU-tier output passes the US1 ffprobe checks; capability self-test runs once and is skipped on the next process start; cancelling a GPU-tier job stops it within 2 s with no `.part` left (SC-006); SC-003 GPU ≤ CPU time `#[ignore]` bench
- [X] T068 [P] [US3] CLI tests in `crates/rmedia-cli/tests/doctor.rs` and `crates/rmedia-cli/tests/upscale.rs`: `doctor --json` has `tiers`, `gpus`, `models`, `capability`; `doctor --recheck` rewrites `checked_at`; `--tier gpu` with no usable GPU (the child processes get `VK_DRIVER_FILES` and `VK_ICD_FILENAMES` set to a missing file, and `RMEDIA_STATE_DIR` a temp dir) exits 3 with `TIER_UNAVAILABLE` — *note: CLI `rmedia upscale` tests hide the Vulkan drivers so the tier is CPU on every machine*

### Implementation for US3

- [X] T069 [US3] Implement listing, device classification, fingerprint (Linux ICD files; Windows `HKLM\SOFTWARE\Khronos\Vulkan\Drivers` via the existing `windows` 0.62 crate with the `Win32_System_Registry` feature), `CapabilityRecord` (ts-rs), `load/save/ensure(recheck)`, CPU and GPU self-tests at preset size, and `demote()` in `core/upscale/capability.rs` (makes T062, T063 pass) — *note: async through `exec::process` (60 s per probe, 180 s total, Ctrl-C); checks cut short are not stored; `CheckMode::{Cached, Ensure, Recheck}`*
- [X] T070 [P] [US3] Implement `TierDecision` and `select()` (the `Tier` enum exists since T008) in `core/upscale/tier.rs` and use it in `plan_upscale` (replacing the fixed CPU decision from T037) (makes T064 pass)
- [X] T071 [US3] Add the T1 scaler to `graph::build` and the device argv to `ffargs::upscale_args` (makes T065 pass)
- [X] T072 [US3] Implement the one-time automatic fallback (re-run the render stage with `Tier::Cpu`, warning, demotion) and the `Resource::Gpu` permit for GPU jobs in `core/exec/upscale.rs` (makes T066 pass)
- [X] T073 [US3] Feed measured fps into `estimate`, report `tier`/`tier_reason`/`suggest` in dry-run JSON and human output, and add tiers, GPUs, models, presets problems and `--recheck` to `core/doctor.rs` and `crates/rmedia-cli/src/commands/doctor.rs` (makes T067, T068 pass) — *note: `doctor::check_capability` (async) replaces the planned `run_with`; desktop `doctor` loads a matching record, `doctor_recheck` runs the self-tests (Settings › Check again)*

**Checkpoint**: quickstart.md › US3 passes on this laptop. senior-code-reviewer pass; security-auditor on the capability file (lock, atomic write, parsing of a tampered record) and registry/ICD reads. Commit.

---

## Phase 6: User Story 4 — Batch and chained clipping (P4)

**Goal**: upscale options in manifests, CSV, folders and `--inputs-from`; `clip --upscale` in one pass.

**Independent Test**: quickstart.md › US4. A 3-video manifest with a preset override produces three correctly sized outputs; `clip --upscale` leaves no intermediate files.

### Tests for US4 (write first, confirm they fail)

- [X] T074 [P] [US4] Layering tables for `PartialUpscale` in `core/request.rs` `#[cfg(test)]`: defaults < video < clip per field, `{}` enables, `enabled: false` disables at that level, `diff` emits only changed nested fields, round trip `apply(diff(x)) == x` — *note: also covers `Options.upscale_fill` (batch flags fill, never enable) and `crf: null`*
- [X] T075 [P] [US4] Manifest tests in `core/input/manifest_json.rs` and `core/input/manifest_csv.rs` `#[cfg(test)]`: `upscale` at every level; a video without `clips` is whole-file only when upscaling is enabled (otherwise the 001 error at the JSON path); `container` other than mp4 rejected; CSV `upscale_*` columns, `upscale_preset=off`, whole-video rows with empty times; unknown columns still rejected — *note: a non-mp4 `container` is rejected only when the manifest sets it (error at the options object); a CLI `--container` keeps the planner warning. CSV `start` column optional when rows can be whole videos*
- [X] T076 [P] [US4] End-to-end tests in `core-tests/us4_upscale_batch.rs` (include the US1 review's partial-clip cases: an MP4 and an MPEG-TS source with start > 0 and an explicit end — duration, first-frame time, audio/video start offset, chapters dropped): the quickstart 3-output manifest produces the preset sizes; clip + upscale is one ffmpeg process per clip (count spawns via the event stream) and leaves no intermediate files; clip outputs use `{source}_{index:03}_{name-or-range}_{preset}.mp4` in `clips/`; one failing video → others complete, exit/summary as 001 — *note: "one ffmpeg per clip" is asserted as exactly one `render` stage per job, the measured span equal to the clip, and no leftovers (no spawn event was added)*
- [X] T077 [P] [US4] CLI tests in `crates/rmedia-cli/tests/upscale.rs` and `crates/rmedia-cli/tests/batch.rs`: `rmedia upscale DIR` uses discovery and excludes `upscaled/` and `-o`; `--inputs-from`; `clip … --upscale --preset reddit-feed-45`; upscale flags without `--upscale` → exit 2; `batch --preset` acts as a CLI default under manifest values — *note: also `batch --upscale` (turns upscaling on for every video), and `clip --upscale -o` with several sources*

### Implementation for US4

- [X] T078 [US4] Add the `upscale` object to `specs/001-video-clipping/contracts/manifest.schema.json` (per contracts/manifest-upscale.md) and parse it in `core/input/manifest_json.rs`; add the CSV columns to `core/input/manifest_csv.rs` (makes T075 pass) — *note: schema `clips` no longer required; `crf` integer or null*
- [X] T079 [US4] Finish `PartialUpscale` layering/diff in `core/request.rs` (makes T074 pass) — *note: `apply`/`diff` lived in `upscale/mod.rs` already; `crf` became a double option so every diff round-trips*
- [X] T080 [US4] Planner: clip jobs with upscaling become `JobSpec::Upscale` with the clip span as `cut` (trim first in the graph), clip naming/output dir defaults, whole-video manifest entries, in `core/plan/mod.rs` (makes T076 pass) — *note: already handled by the US1–US3 planner; added `UPSCALE_MULTI_CLIP_NAME_TEMPLATE` (several sources into one `-o`), a crf range check and `BatchRequest::needs_capability_check`*
- [X] T081 [US4] CLI: `clip --upscale` + upscale flags in `crates/rmedia-cli/src/commands/clip.rs`; upscale flags as defaults in `crates/rmedia-cli/src/commands/batch.rs`; discovery exclusion of default output folders in `core/discover.rs` (makes T077 pass) — *note: exclusion lives in the CLI's `expand_inputs` (core `ScanOptions.exclude_dir_names`); `clip --upscale` skips `clips/` and `upscaled/`. `UpscaleFlags` are optional; the GPU check runs via `commands::check_tiers`*

**Checkpoint**: quickstart.md › US4 passes. senior-code-reviewer pass; security-auditor on discovery exclusion, manifest/CSV `upscale` parsing and clip output paths. Commit.

---

## Phase 7: User Story 5 — Preview and tune in the desktop app (P5)

**Goal**: "Upscale for phone" view with before/after still, crop overlay, fallback ranges on the timeline, queue and export parity.

**Independent Test**: quickstart.md › US5. Before/after within 5 s for 1080p; overlay follows the marker; exported manifest run by the CLI gives identical hashes.

### Tests for US5 (write first, confirm they fail)

- [X] T082 [P] [US5] Core test for `upscale::preview_frame` in `core-tests/preview.rs`: returns before/after PNGs at the target size within 5 s for `Hd60Mp4`; uses the cached analysis x when present, otherwise centred with `approximate: true` — done: `crates/rmedia-core/tests/us5_preview.rs` (≈1 s on Hd60Mp4; cached path, fallback range, range check)
- [X] T083 [P] [US5] Bridge tests in `apps/desktop/src-tauri/tests/bridge.rs`: `list_presets`, `capabilities`, `analyze_reframe` (Channel events, shares the CLI cache: a following CLI dry run spawns no decode), `cancel_analysis`, `upscale_preview` path policy (only allowed sources), and parity: desktop run vs exported manifest through `rmedia batch` → equal sha256 (FR-022) — done: presets, preview path policy and PNG serving, invalid times, analysis progress/cancel/supersede and CLI cache hit (YuNet), models_dir confinement, FR-022 parity with `rmedia batch`
- [X] T084 [P] [US5] Vitest tests in `apps/desktop/src/lib/stores/upscale.test.ts`: keyframe interpolation for the overlay rectangle, fallback-range timeline segments, queue entry → manifest `upscale` diff against defaults — done: plus `upscale.svelte.test.ts` (stale results, clearing, retries) and run label tests

### Implementation for US5

- [X] T085 [US5] Implement `upscale::preview_frame(path, at, &opts) -> PreviewPair` (one ffmpeg run of the job graph limited to one frame) in `core/upscale/mod.rs` (makes T082 pass) — done in `upscale/preview.rs` (`single_job`, `planned_job`, `preview_frame` with a one-time CPU retry after a GPU failure); `reframe::lookup_cached`
- [X] T086 [US5] Add the Tauri commands from contracts/tauri-commands.md as thin adapters in `apps/desktop/src-tauri/src/commands.rs` and `apps/desktop/src-tauri/src/ops.rs`; serve preview stills from the app cache via `media_server.rs`; `models_dir` set only via a Rust folder picker; extend `plan`/`run`/`export_manifest`/settings for `upscale` (makes T083 pass) — done; `analyze_reframe` returns `{analysis, note}` and runs tier-free; a new analysis or preview cancels the previous one
- [X] T087 [US5] Regenerate bindings with `cargo xtask gen-bindings` (Preset, UpscaleOptions, PartialUpscale, CapabilityRecord, ReframeAnalysis, JobStage, JobMeasure) into `apps/desktop/src/lib/bindings/`; extend `apps/desktop/src/lib/api.ts` and `apps/desktop/src/lib/manifest.ts` — done: bindings unchanged (types already exported); desktop-only types in `api.ts`
- [X] T088 [P] [US5] Implement `apps/desktop/src/lib/stores/upscale.ts` (analysis state, keyframe interpolation, preview requests) (makes T084 pass) — done: `stores/upscale.ts` (pure) and `stores/upscale.svelte.ts` (state)
- [X] T089 [P] [US5] Build `apps/desktop/src/lib/components/UpscaleView.svelte`: preset picker with device classes, reframe/tier/motion pickers (tier reasons, AI suggestion), before/after slider, "Add to queue"; defaults from `upscale_defaults` so SC-007 takes 3 interactions — done: plus `UpscaleCompare.svelte`; "Upscale now" runs only that job (owner decision)
- [X] T090 [P] [US5] Crop-rectangle overlay and fallback-range timeline marks in `apps/desktop/src/lib/components/Player.svelte` — done
- [X] T091 [P] [US5] Upscale fields in queue rows and run outcomes (tier used, fallback, ranges) in `apps/desktop/src/lib/components/Queue.svelte` and `RunPanel.svelte`; tiers, GPUs, models and "Check again" in `Settings.svelte` — done
- [X] T092 [US5] Wire the view into navigation in `apps/desktop/src/App.svelte` (premium-ui-designer review); `npm test` and `npm run check` pass — done: mode tabs in the player header; premium-ui-designer review applied

**Checkpoint**: quickstart.md › US5 passes manually on Linux (X11 and Wayland). senior-code-reviewer pass; security-auditor on the new command surface (paths from the webview, preview files, `models_dir`). Commit.

---

## Phase 8: User Story 6 — AI upscaling when the GPU can handle it (P6)

**Goal**: `rmedia-sr` ncnn/Vulkan worker in a 3-process pipeline, gated by a self-test, with anti-flicker.

**Independent Test**: quickstart.md › US6. Pipeline tests run in CI with the worker's test-only CPU mode; quality, flicker and SC-009 tests run on a discrete-GPU machine.

### Setup for US6

- [X] T093 [US6] Add `fetch-ncnn` to `xtask/src/main.rs` (ncnn 20260526 prebuilt static, Linux ubuntu-2204 and Windows vs2022 with Vulkan, SHA-pinned in `xtask/models.lock.json`) and extend `fetch-models` with `realesr-animevideov3-x2/x3/x4` `.param/.bin`; add `licenses/models/real-esrgan-BSD-3.txt` — done: `fetch-ncnn` (archive installed to `third_party/ncnn/<T>/`), models taken from the pinned `realesrgan-ncnn-vulkan-20220424-ubuntu.zip` (archive and files SHA-256 verified); x2/x3/x4 `.bin` are identical, only `.param` differs
- [X] T094 [US6] Create `crates/rmedia-sr` (bin, member of the workspace) with a `build.rs` that links the fetched ncnn and an `ffi` module as the only place `unsafe` is allowed (`#![deny(unsafe_code)]` elsewhere) in `crates/rmedia-sr/Cargo.toml`, `build.rs`, `src/main.rs`, `src/ffi.rs`; `build.rs` fails with "run `cargo xtask fetch-ncnn`" when the libraries are missing; add a cached `cargo xtask fetch-ncnn` step to `.github/workflows/ci.yml` on Linux and Windows, and `--exclude rmedia-sr` in the `windows-check-gnu` and `minimum-engine` jobs so `cargo clippy/test --workspace` keep building every member (Principle IV) — done: C++ shim for GPU listing/type/heap budget (not in the C API); static libstdc++/libgomp, so the Linux worker needs only glibc

### Tests for US6 (write first, confirm they fail)

- [X] T095 [P] [US6] Worker tests in `crates/rmedia-sr/tests/protocol.rs`: argv validation (exit 2), short final frame → exit 4, missing model → exit 5, JSON stderr lines parse, and the test-only `--model builtin:nearest --gpu none` mode (no GPU, CPU nearest-neighbour ×s; see `contracts/rmedia-sr-protocol.md`) produces exactly `(W·s)×(H·s)×3` bytes per frame in order — done; plus pinned-hash loading, truncated weights → exit 5, determinism and tiling checks in the `#[ignore]` GPU test
- [X] T096 [P] [US6] Pipeline tests in `core-tests/us6_pipeline.rs` using `builtin:nearest`: output size and duration match, audio stays in sync (≤ 1 frame), progress and ETA advance, cancel after 1 s stops all three processes within 2 s with no `.part` or scratch left, a worker crash surfaces `ENGINE_FAILED` with its stderr — done; plus AI-vs-CPU PSNR > 40 dB for smart, partial fallback and blur framing, and timing cases for the -copyts trim path, whole video with an audio offset, VFR and interlaced
- [X] T097 [P] [US6] AI gate tests in `core/upscale/tier.rs` and `core/upscale/capability.rs` `#[cfg(test)]`: self-test parsing; available only when the predicted time for 60 s 480p ≤ 10 min; forced AI over 10× clip length warns "too slow: estimated N min"; integrated GPU reported unavailable with the measured reason — done
- [X] T098 [P] [US6] `#[ignore]` GPU tests in `core-tests/us6_ai.rs`: VMAF(AI) ≥ VMAF(GPU tier) + 3 on a degraded 360p source (SC-002); static-region frame-difference flicker ≤ the GPU tier's on the same source (US6 scenario 4); 60 s 480p ≤ 10 min and self-test prediction within ±30% (SC-009) — written (`tests/us6_ai.rs`, run with `--ignored`). Dev numbers on the Intel iGPU: SC-002 AI 51.84 vs GPU tier 43.73 VMAF (+8.1); flicker AI 1.105 vs GPU 1.900 (source 2.474). SC-009 needs the owner's discrete GPU (the iGPU predicts ≈106–177 min for 60 s of 480p and is gated off)

### Implementation for US6

- [X] T099 [US6] Implement the ncnn FFI (net load, Vulkan device select with software-device rejection, extractor, tiling, fp16 option) in `crates/rmedia-sr/src/ffi.rs` — done: path or verified-bytes loading, software devices refused, tiles with 10 px context, Winograd off on integrated GPUs (≈2× faster on Intel Xe)
- [X] T100 [US6] Implement the worker CLI per `contracts/rmedia-sr-protocol.md` (`--list-gpus`, `--selftest`, `--version`, the stdin→stdout frame loop, JSON stderr, exit codes) in `crates/rmedia-sr/src/main.rs` (makes T095 pass) — done; stderr is redirected to the null device after keeping a copy for JSON lines, so ncnn's own prints never break the protocol
- [X] T101 [US6] Implement `exec/pipeline.rs`: ffmpeg #1 → `rmedia-sr` → ffmpeg #2 in one kill-tree group via a new shared-group spawn in `core/exec/process.rs` (the first child leads the process group / owns the Job Object, later children join it), with pipes copied without buffering whole frames in memory, and progress from ffmpeg #2 in `core/exec/pipeline.rs`; locate `rmedia-sr` beside the exe in `core/tools.rs` (makes T096 pass) — done: one process group on Unix (ffmpeg #1 leads, the others join; each reaped on its own, since process-wrap's group wrapper reaps all members), a kill-on-close Job Object each on Windows; OS pipes between stages
- [X] T102 [US6] Add the AI graphs to `core/upscale/graph.rs`: ffmpeg #1 (pre, reframe crop and blur-pad at source scale, `rgb24` rawvideo) and ffmpeg #2 (rawvideo input with source timing, scale to W×H, anti-flicker, encode, audio from the source as a second input), with insta snapshots; dry-run JSON for AI jobs adds `pipeline: [argv1, worker argv, argv2]` (contracts/json-output.md) — done: blur and fallback backgrounds come from the source in ffmpeg #2, so only the picture goes through the worker; inputs over half the output are shrunk first (×2)
- [X] T103 [US6] Add the AI self-test, `sec_per_mpx`, the availability gate and the AI fingerprint input to `core/upscale/capability.rs`; `suggest`/forced AI handling in `core/upscale/tier.rs`; doctor AI line (makes T097 pass) — done: the worker lists its own devices (ncnn indices); self-test 4 frames of 480x270; gate 60 s 480p ≤ 10 min; a timed-out self-test is a conclusive "too slow"
- [X] T104 [US6] Measure anti-flicker candidates (motion-masked `tmix` where a `tblend` difference is low, versus none) on a discrete GPU with the T098 flicker metric; pick one and record the numbers in research.md R13 (makes T098 pass on owner hardware) — done: temporal-only `hqdn3d=0:0:3:3` (static flicker −35 %, −0.07 VMAF on motion; `tmix` −17 VMAF; masked `tmix` drops a frame). Measured on the iGPU; owner confirms on a discrete GPU
- [X] T105 [US6] Package the worker and models: `rmedia-sr` as an `externalBin` and `models/` as resources in `apps/desktop/src-tauri/tauri.conf.json`; add both to `.github/workflows/release.yml` — done: also bundles YuNet, which no installer shipped before; model search adds `<prefix>/lib/rmedia/models` for the deb and AppImage

**Checkpoint**: pipeline tests green in CI; owner runs T098 on a discrete GPU. senior-code-reviewer pass; security-auditor on the FFI boundary, model file parsing and the process pipes. Commit.

---

## Phase 9: Polish & Cross-Cutting

- [X] T106 [P] User docs: upscaling sections in `docs/user/cli-guide.md`, `docs/user/desktop-guide.md`, GPU/AI/model problems in `docs/user/troubleshooting.md`
- [X] T107 [P] API and reference docs: `docs/api/cli-reference.md` (upscale, clip --upscale, doctor --recheck), `docs/api/json-output.md`, `docs/api/file-formats.md` (presets, manifest `upscale`, capability record, crop path), `docs/api/desktop-ipc.md`, `docs/reference/glossary.md`
- [X] T108 [P] Architecture and admin docs: `docs/architecture/overview.md` and `domains.md` (preset, upscale, reframe, capability, pipeline), `docs/administration/configuration.md` (user presets folder, `models_dir`, state and cache files, the `RMEDIA_CONFIG_DIR`/`RMEDIA_STATE_DIR`/`RMEDIA_CACHE_DIR` overrides), `docs/operations/build-and-release.md` (fetch-models, fetch-ncnn, packaging)
- [X] T109 [P] Release notes `docs/releases/0.2.0.md`; update `docs/README.md` and the repo `README.md`
- [X] T110 Regenerate `THIRD_PARTY_NOTICES.md` with `cargo xtask notices` (tract, fs4, ncnn, model licences)
- [X] T111 Run the full quickstart.md on Linux and, via CI artifacts, on Windows; record results, the SC-007 three-interaction check in the desktop app, and the owner checks (two-phone gallery test, Reddit upload, discrete-GPU AI) in `specs/002-mobile-upscale/checklists/validation.md`
- [X] T112 Final senior-code-reviewer pass over the workspace; fix findings; set the CLAUDE.md active feature to 003

---

## Dependencies & Execution Order

- **Setup (T001–T006)** → **Foundational (T007–T021)** → user stories.
- Inside Foundational: T007 before T008; A1 (T008) → A5 (T009) → A2 (T010–T011) → A3 (T012) → A4
  (T013), all touching `plan/mod.rs` / `exec/mod.rs`, so sequential. T014, T015, T018, T019, T021
  run in parallel with the refactors.
- **US1 (T022–T041)** is the base for every other story.
- **US2** and **US3** both extend `graph.rs` and `exec/upscale.rs`; run them in order (US2 → US3).
- **US4** needs US1 only, but is scheduled after US3 to keep `plan/mod.rs` edits sequential.
- **US5** needs US1–US4 core APIs.
- **US6** needs US3 (capability record, GPU permit) and US1's executor; it can start its worker
  tasks (T093–T095, T099–T100) in parallel with US4/US5 since they live in a separate crate.
- **Polish** after US6 (docs for US1–US5 may start after US5).

```
Setup → Foundational → US1 → US2 → US3 ─┬─► US4 → US5 ─┬─► Polish
                                        └─► US6 worker ─┴─► US6 pipeline
```

## Parallel Examples
- Foundational: T014, T015, T018, T019, T021 together while T008–T013 run in sequence.
- US1 tests: T022–T030 together; then T032/T033/T035 in parallel; then T034 → T036 → T037 → T038.
- US2: T042–T050 together; T052–T057 in parallel; then T058 → T059 → T060 → T061.
- US3: T062–T068 together; T070 parallel to T069.
- US5 views: T088–T091 in parallel after T086/T087.

## Implementation Strategy
1. **MVP** = Setup + Foundational + US1 → `rmedia upscale` on the CPU tier, verified and
   cancellable. Stop and validate against quickstart.md › US1.
2. Add US2 (the differentiator), then US3, then US4: each is an independently testable CLI increment.
3. US5 desktop view.
4. US6 AI tier; quality targets confirmed on owner hardware.
5. Every checkpoint: `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`
   green on both OSes, a senior-code-reviewer pass, and security-auditor where noted.
