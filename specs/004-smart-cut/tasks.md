---
description: "Task list for 004 Smart Cut"
---

# Tasks: Smart Cut

**Input**: Design documents from `specs/004-smart-cut/`

**Prerequisites**: plan.md, spec.md, research.md (R1–R11), data-model.md, contracts/,
quickstart.md

**Status (2026-11-16)**: T001–T046 done: Phases 1 and 2 (with the spikes S1–S3; S1's WebView2,
phone and Reddit rows are owner checks), US1, the MVP, US2 (fallbacks and strict mode), US3
(the dry run), US4 (manifests and parity) and US5 (the desktop app). Next: Phase 8 (T054–).
Roadmap:
`docs/roadmap.md`.

**Tests**: REQUIRED. Constitution Principle III (test-first with real media) is non-negotiable.
- Within each story, write the test tasks first and confirm they FAIL before implementing.
- Media comes from `rmedia-testkit` lavfi fixtures. No binary fixtures or engine binaries are
  committed.

**Organization**:
- Grouped by user story, in priority order US1 → US5.
- Phase 2 makes `smart` a valid mode everywhere (still planned as accurate), adds the fixtures and
  the keyframe index, and runs the spikes S1–S3 that settle R5's open choices.

## Format: `[ID] [P?] [Story] Description`
- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Paths are relative to the repo root.
- `core/` is shorthand for `crates/rmedia-core/src/`, `core-tests/` for
  `crates/rmedia-core/tests/`, `cli/` for `crates/rmedia-cli/src/`, `cli-tests/` for
  `crates/rmedia-cli/tests/`, `desk/` for `apps/desktop/src-tauri/src/` and `ui/` for
  `apps/desktop/src/lib/`.

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Record the 0.3.0 baseline in a new `specs/004-smart-cut/checklists/validation.md`:
  - run `cargo test --workspace` (bundled engine, `RMEDIA_REQUIRE_FFMPEG=1`) and the desktop
    tests (`apps/desktop/src-tauri`: `cargo test`; `apps/desktop`: `npm test`);
  - note the passing counts (workspace 505 / 15 ignored, desktop 22) and the insta snapshot
    list (`core-tests/snapshots`, `cli-tests/snapshots`).

  Every phase must keep them green (SC-009, FR-018). Done: 505 passed / 0 failed / 15 ignored
  with `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` set to the bundled engine (without them the libvmaf
  tests fail on the system ffmpeg); recorded in `checklists/validation.md`.
- [X] T002 Scaffold empty modules and register them:
  - `core/smart/{mod,keyframes,args}.rs`, declared as `pub mod smart` in `core/lib.rs`;
  - `core/exec/smart.rs`, declared in `core/exec/mod.rs`;
  - test files `core-tests/smart_keyframes.rs` and `smart_us4.rs`; `smart_us1.rs` …
    `smart_us3.rs` are created with their stories' first tests, so no empty test binaries are
    built.
- [X] T003 [P] Add testkit fixtures in `crates/rmedia-testkit/src/lib.rs` (plan › Testing):
  - `SmartMp4` (testsrc2 1280×720 25 fps, 30 s, sine audio, libx264
    `keyint=50:min-keyint=50:scenecut=0:bframes=3`, closed GOPs), plus derived `SmartMov`,
    `SmartMkv` and `SmartTs` (same streams, remuxed);
  - `SmartBaseline` (as `SmartMp4`, `-profile:v baseline`, no B-frames, no audio);
  - `SmartHd60` (1920×1080 25 fps, 75 s, 2 s GOPs; the SC-005 bench source);
  - `OpenGop` (`open-gop=1`, `bframes=3`, 10 s);
  - `High10` (`-pix_fmt yuv420p10le`, `-profile:v high10`, 6 s);
  - `HevcMp4` (libx265 if the engine has it, else the test skips with `require_encoder!`);
  - `ResChange` (two 4 s halves at 1280×720 and 960×540, joined with the concat demuxer in TS,
    then remuxed to mp4) — moved to T032, its only user: a fixture made from two encodes needs a
    builder that the single-command `Fixture::args` does not have;
  - `OtherEncoder` (`preset=slow:ref=4:bframes=5`, 2 s GOPs, 20 s; the R4 matching case).

  Reuse `Gop50Mp4`, `Hd60Mp4`, `VfrMp4`, `OffsetTs`, `MislabelledTs`, `Rotated90`,
  `Anamorphic`, the interlaced fixture and `MultiMkv`. Derived fixtures hash their parent's spec.
- [X] T004 [P] Add testkit helpers in `crates/rmedia-testkit/src/lib.rs`:
  - `video_packets(path) -> Vec<Packet { pts, dts, key }>` (ffprobe packet JSON, v:0);
  - `frame_count(path)` (`-count_frames nb_read_frames`);
  - `decode_errors(path) -> Vec<String>` (`ffmpeg -v error -f null -`);
  - `framemd5(path, from, frames) -> Vec<String>` (video only, hashes without timestamps);
  - `stream_starts(path) -> (f64, f64)` (first video and audio pts, for the 20 ms sync check).

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: `smart` parses everywhere but still plans as accurate; the keyframe index and the
fallback types exist; the spikes settle the join format. No user story can start before this.

### Mode and options (data-model › Mode, Options.smart_fallback)

- [X] T005 [P] Tests first in `core-tests/smart_us4.rs` (`mode_parses_everywhere`), and extend
  the existing parser tests:
  - `Mode::Smart` from JSON manifests at run, video and clip level, from the CSV `mode` column
    (case-insensitive) and from `Settings.defaults.mode`;
  - `smart_fallback` from JSON (`true`/`false`), CSV (`true`/`false`/empty) and settings;
  - the CSV error text `mode must be fast, accurate or smart, not "<value>"`;
  - layering: clip over video over run over settings, for both fields;
  - every 0.3.0 manifest and settings fixture under `core-tests/data` loads with identical
    `Options`.
- [X] T006 Add `Mode::Smart` (serde `"smart"`, doc comment) and `Options.smart_fallback: bool`
  (default `true`; `PartialOptions.smart_fallback: Option<bool>` with the existing layering) in
  `core/request.rs`; settings defaults in `core/settings.rs`.
- [X] T007 Accept `smart` and the `smart_fallback` column in `core/input/manifest_csv.rs` and
  `smart_fallback` at all three levels in `core/input/manifest_json.rs`; update the CSV error
  message. Do not change the CSV writer yet (T046). T005 passes.
- [X] T008 Until US1 lands, resolve `Mode::Smart` to `Mode::Accurate` in `core/plan/mod.rs`
  with a `// TODO(004 T026)` marker, so `--mode smart` already works end to end as accurate.
  Every existing snapshot stays unchanged.

### Issue codes and fallback types (data-model › FallbackReason, Issue codes)

- [X] T009 [P] Add `FallbackReason` (12 variants, serde `snake_case`, `Ord` for the totals map,
  `message()`, `known_before_encoding()`) in `core/smart/mod.rs`, with a unit test that pins
  every serde value and message (the values are stable once released, FR-007).
- [X] T010 [P] Add the issue codes `SMART_FALLBACK` (warning), `SMART_UNAVAILABLE` (error) and
  `SMART_NOT_APPLIED` (notice) in `core/error.rs`, following the existing code table and its
  test.

### Keyframe index (research R1, R2)

- [X] T011 Tests first in `core-tests/smart_keyframes.rs`:
  - table tests on recorded packet JSON (`core-tests/data/smart/*.json`, text, small): a
    closed-GOP x264 stream (every keyframe `clean`), an `open-gop=1` stream (the leading
    picture at 1.960 marks the keyframe at 2.000 not clean), a stream without B-frames;
  - `frame_duration` is `Some` for CFR and `None` for uneven spacing;
  - caps: a synthetic 20,001-keyframe input is `truncated`;
  - real fixtures: `SmartMp4/Mov/Mkv/Ts` give keyframes every 2.000 s (within one frame) and
    all clean; `OpenGop` gives unclean keyframes; a window read returns only the window's
    packets.

  Then implement `KeyframeIndex`, `KeyframeEntry` and `keyframes(tools, media, window, cancel)` in
  `core/smart/keyframes.rs`: ffprobe through `exec::process` (`-select_streams v:0
  -show_entries packet=pts,dts,flags -read_intervals`), reusing 001's packet-JSON parsing in
  `core/probe.rs`; a plan-time index keeps every packet in the window, the whole-file index only
  keyframes; 20,000-keyframe and 30 s caps.

  Done, with two findings: MPEG-TS seeks land after the requested point, so a window is read
  from 12 s, then 65 s, before its start (as 001's `keyframe_at_or_before` does), and whole-file
  chunks start at the file's beginning and overlap by 30 s (`chunked_reads_find_every_keyframe_once`);
  x264 `open-gop=1` puts leading pictures after some keyframes only, and where depends on its
  thread count (CI and this machine differed at 6 s), so `OpenGop` is encoded with `threads=1`
  and the test checks the index against the packets read independently.

### Phase 0 spikes (quickstart › Phase 0 spikes; results go in research.md)

- [X] T012 Spike S1, player compatibility (R5): with a throwaway script in the scratchpad (not
  committed), make smart outputs from `OtherEncoder` by hand as in R3–R5: mp4 `avc3`, mp4 `avc1`
  with in-band parameter sets, mkv and ts. Play each in WebKitGTK (the desktop app), Chrome,
  Firefox and VLC on Linux; WebView2 on Windows (owner machine or CI artifact); an Android phone
  and an iPhone (owner). Record a table in `research.md` R5 and set the default sample entry. The
  phone and Reddit rows may stay open as owner checks; the default is chosen from the desktop
  rows.

  Done: `avc1` with in-band parameter sets. FFmpeg, GStreamer 1.24, VLC 3.0.24, Firefox 157 and
  Chrome 157 decode it, seams included; `avc3` fails in VLC 3 and Chrome even for plain files.
  VLC was installed as a user Flatpak. Open: WebView2, Android, iPhone, Reddit.
- [X] T013 [P] Spike S2, rotation (R5): smart-cut `Rotated90` by hand with `-display_rotation` on
  the joined input for mp4, mov, mkv and ts; record the displayed size per container in
  `research.md` R5, and whether `rotation` stays a fallback reason (and for which containers).

  Done: head and tail need `-noautorotate`; with it, mp4, mov and mkv keep the rotation; ts
  outputs fall back with `rotation`.
- [X] T014 [P] Spike S3, ts copy seek (R3): run the segment-muxer copy with Kprev seeking on
  `OffsetTs` and `MislabelledTs`; record in `research.md` R3 whether the copied frame count
  equals the index count, and any adjustment needed.

  Done: 250 of 250 frames, identical, on both. Adjustment: `-copyts`, and the middle segment is
  picked by its start in `-segment_list` (its number is not fixed). Superseded in US1: the
  segment muxer depends on where seeks land (Matroska lands a GOP early), so T025 copies as fast
  mode does, ended by a packet count (research R3).

**Checkpoint**: `--mode smart` runs as accurate everywhere; T001's baseline is green; the spikes
are recorded and any change to R3/R5 is reflected in plan.md and data-model.md.

---

## Phase 3: User Story 1 — A frame-exact clip that keeps the original quality (Priority: P1) 🎯 MVP

**Goal**: `rmedia clip --mode smart` on an eligible H.264 source gives a frame-exact clip whose
whole GOPs are copied, verified before commit.

**Independent Test**: a 60 s mid-GOP range of `SmartHd60` in mp4: within one frame at both ends,
≥ 80% of frames copied byte-for-byte, no decode errors, and the run reports the shares.

### Tests for User Story 1 ⚠️ (write first, confirm they fail)

- [X] T015 [P] [US1] `core-tests/smart_us1.rs` › `frame_exact_ends` (SC-001): for
  `SmartMp4/Mov/Mkv/Ts` and `SmartBaseline`, ranges with both ends mid-GOP: the output frame
  count equals the planned count, and the first and last frames match the source frames at the
  requested start and end (PSNR against the neighbours, as R4 measured).
- [X] T016 [P] [US1] `smart_us1.rs` › `keyframe_aligned_ends_skip_parts` (scenario 2 and edge
  cases): start on a keyframe → no head; end on a keyframe → no tail; both → pure copy, still
  frame-exact; end of media → no tail; a range ending after the last keyframe → tail from it.
- [X] T017 [P] [US1] `smart_us1.rs` › `long_clips_copy_most_frames` (SC-002) and
  `copied_frames_are_identical` (SC-006 part): `copied_share ≥ 0.8` for clips of ≥ 10 GOPs;
  framemd5 of every copied frame equals the source's.
- [X] T018 [P] [US1] `smart_us1.rs` › `seams_decode_cleanly` (SC-003): 0 decode errors; sorted pts
  step by one frame duration across both seams; audio and video start within 20 ms and the
  audio covers the range (scenario 3); run on every eligible fixture including `OtherEncoder`.
- [X] T019 [P] [US1] `smart_us1.rs` › `reencoded_parts_match_accurate_quality` (SC-006): head and
  tail VMAF (`testkit::vmaf()`) against the source ≥ accurate mode's for the same frames.
- [X] T020 [P] [US1] `smart_us1.rs` › `outcome_reports_shares` (scenario 4, FR-012) and
  `deterministic_per_machine` (R10): `JobMeasure::Clip.smart` has `mode_used: smart` and the
  copied and re-encoded frame counts; two runs give byte-identical outputs.
- [X] T021 [P] [US1] Argv snapshots in `core-tests/snapshots_ffargs.rs` (new cases, insta): the
  head, copy, tail, audio and join argv for `SmartMp4` (mp4 with the S1 sample entry), `SmartMkv`
  and `SmartTs`, and a source with colour tags and SAR (`Anamorphic`). Paths are `file:`, concat
  list entries are relative names.
- [X] T022 [P] [US1] `core-tests/cancel.rs` › `smart_*` (SC-008, scenario 5): cancel during each
  stage (head, copy, tail, audio, join, verify); stopped within 2 s, no `.part`, scratch folder
  or child process left.
- [X] T023 [P] [US1] `core-tests/bench.rs` › `smart_vs_accurate` (`#[ignore]`, SC-005): a 60 s
  range of `SmartHd60`; smart ≤ ⅓ of accurate mode's wall time.

### Implementation for User Story 1

- [X] T024 [US1] `smart::check(media, index, range, caps, output_ext) -> Result<SmartPlan,
  FallbackReason>` in `core/smart/mod.rs` (R2), for now returning `Ok` for eligible sources and
  the matching reason for the rest; build `SmartPlan` (head, copy, tail `Part`s with frame counts
  from the index, `copied_share`, `copy_lead`, `PartEncode` from the probe, `sample_entry`,
  `rotation`). Unit tests on synthetic indexes for the part boundaries in T016.

  Done, with every R2 rule (T035's rules are in; US2 adds their tests and notices). Also: an
  odd-sized picture (x264 cannot encode it) gives `params_changed`; the reference count is not
  matched (research R4); `copied_share` has four decimals, as the contract shows it.
- [X] T025 [US1] `core/smart/args.rs`: pure argv builders for head and tail (R4: exact seek with
  `-noautorotate` (S2), `-frames:v N`, matched profile, level, `ref`, `bframes`, `open-gop=0:stitchable=1`, colour
  tags, `setsar`, `-fps_mode passthrough`, preset medium CRF 18, mpegts), copy (R3, revised: a
  coarse seek, an output-side cut `copy_lead` before K1, `-frames:v` = the copied frames), audio (R6: accurate mode's `EncodeProfile` over [A, Z)), and
  join (R5: concat demuxer over relative TS names, `-tag:v` from `sample_entry`, rotation from
  S2, `StreamPlan` mapping as in accurate mode). T021 passes.
- [X] T026 [US1] Add `ClipSpec.smart: Option<SmartPlan>` in `core/plan/ffargs.rs`; in
  `core/plan/mod.rs` replace T008's shim: when the resolved mode is `Smart`, read the plan-time
  index window and call `check`; `Ok` keeps `Mode::Smart` with `ClipSpec.smart`, `Err` plans an
  ordinary accurate `ClipSpec` (the reasons and notices come in US2). Existing snapshots stay
  unchanged.

  Done. The index is read 3 s past the end, so a keyframe on the end, and the packets after K2
  (the open-GOP rule), are in it; plan-time reasons are logged at info level until US2's notices.
- [X] T027 [US1] `core/exec/smart.rs`: run the stages head → copy → tail → audio → join in the
  job's scratch folder (002's `create_scratch`, journaled), one process at a time through
  `exec::process`; join into the `.part` file; emit `JobStage` events with the R9 stage names
  and weights; remove the scratch folder on success, failure and cancel. Dispatch from
  `core/exec/clip.rs` when `ClipSpec.smart` is `Some`.
- [X] T028 [US1] Verification (R7) in `core/exec/smart.rs`: packet scan (count, pts steps across
  seams, first pts 0, duration within one frame), seam-window decodes (no error lines, frame
  counts, size and pixel format), framemd5 of the first and last copied GOP against the source.
  On failure, discard the `.part` file and return the reason (`params_changed`/`seam_check`);
  the retry comes in US2 (T037). On success, commit through the existing no-clobber commit.

  Done as R7 now describes it (packet scan; seam windows with `-noautoscale` framemd5: errors,
  frame sizes, copy identity). The run-time fallback (T037) is in `exec/clip.rs` already: the
  `.part` is removed, a `SMART_FALLBACK` warning is sent and the job runs as accurate; strict
  mode fails it with `SMART_UNAVAILABLE` (`detail.reason`). US2 adds the tests.
- [X] T029 [US1] `SmartOutcome` on `JobMeasure::Clip` in `core/event.rs` (absent unless smart was
  requested), filled from the plan and the verified counts; the ts-rs export for the desktop
  types.
- [X] T030 [US1] CLI: `--mode smart` in `cli/commands/{clip,batch}.rs` (clap value), the human
  result line `smart, 92% copied` in `cli/render.rs`; `cli-tests/clip_single.rs` ›
  `smart_mode_clip` and a `contract_clip_json.rs` case for the outcome JSON
  (contracts/json-output.md › Outcome).

**Checkpoint**: run `quickstart.md` › Manual run (the eligible part) and T015–T023. Then
`cargo fmt`, clippy, `cargo test --workspace`, senior-code-reviewer.

---

## Phase 4: User Story 2 — Risky sources fall back safely and say why (Priority: P2)

**Goal**: every source smart mode cannot trust is cut in accurate mode with a stable reason;
strict mode refuses instead.

**Independent Test**: smart mode on the risky fixtures gives frame-exact accurate clips, each
with its distinct reason.

### Tests for User Story 2 ⚠️

- [X] T031 [P] [US2] `core-tests/smart_us2.rs` › `risky_sources_fall_back` (SC-004): one case per
  reason known before encoding, with its fixture: `HevcMp4` → `codec`, a `.webm`/`.avi` source or
  output → `container`, a High 4:2:2 or `High10` profile → `profile`/`pixel_format` (whichever
  rule R2 reaches first; assert the documented one), the interlaced fixture → `interlaced`,
  `VfrMp4` → `vfr`, `Rotated90` cut to a `.ts` output → `rotation` (S2; other containers keep the rotation and are
  smart-cut), a sub-GOP range → `no_full_gop`,
  `OpenGop` → `open_gop`, an engine without libx264 (fake caps) → `encoder_missing`. Each job
  succeeds, is frame-exact, has `mode_used: accurate`, `copied_frames: 0`, the reason, and the
  `SMART_FALLBACK` notice with `detail.reason`.

  Done. `container` and `encoder_missing` use a `.webm` output: accurate mode cannot write
  `.avi` and writes `.mp4` instead (which smart cut handles), and without libx264 accurate mode
  cannot write H.264 either. `High10` reaches `profile` first. The HEVC case runs when the
  engine has libx265.
- [X] T032 [P] [US2] `smart_us2.rs` › `run_time_fallback` (scenario 3): `ResChange` across the
  resolution change → `params_changed`; a test-only hook that corrupts the joined part →
  `seam_check`. The `.part` file is discarded, the clip is redone in accurate mode, and a
  `JobWarning` `SMART_FALLBACK` event is sent.

  Done. `rmedia_testkit::res_change()` builds the fixture in steps (video only: with audio the
  join leaves a gap where the first half's audio runs longer). The hook,
  `smart::test_hooks::reverse_parts_of` (feature `test-support`), joins the parts in reverse
  order for one source path.
- [X] T033 [P] [US2] `smart_us2.rs` › `strict_mode` (scenario 4, FR-009): with
  `smart_fallback=false`, a plan-time reason gives the planning issue `SMART_UNAVAILABLE` and
  nothing runs; a run-time reason fails the job with `SMART_UNAVAILABLE` and nothing is written.
  `cli-tests/clip_single.rs` › `no_smart_fallback_exit_codes`: exit 3 at plan time, 1 at run
  time, 0 for fallbacks without the flag.

  Done; the CLI's run-time case uses `res_change()`, since the test hook does not reach the
  binary.
- [X] T034 [P] [US2] `smart_us2.rs` › `not_applied_with_upscale_or_gif` (FR-013): `--upscale` or
  `--gif` with `--mode smart` runs as accurate with the `SMART_NOT_APPLIED` notice; upscale and
  GIF snapshots unchanged.

### Implementation for User Story 2

- [X] T035 [US2] Complete `smart::check` in `core/smart/mod.rs` with every R2 rule in the
  documented order (cheap probe rules first, then index rules), including the window packet
  spacing check for `vfr` and both copy boundaries for `open_gop`. Unit tests per rule on
  synthetic `MediaInfo` and indexes.

  Done: the rules were in from T024; `smart::tests` adds one case per rule and the order.
- [X] T036 [US2] In `core/plan/mod.rs`: on a reason, add the `SMART_FALLBACK` notice and the
  warning text from contracts/json-output.md, or, under strict mode, the `SMART_UNAVAILABLE`
  planning issue (job not runnable); `SMART_NOT_APPLIED` when the clip also upscales or makes a
  GIF or Reddit MP4.

  Done. `Notice` and `Issue` gain `detail` (`ErrorDetail`, so `reason`); `ClipSpec.smart_fallback`
  carries a plan-time reason to the outcome (`reencoded_frames` from the output's length). A
  keyframe index that cannot be read falls back too (a `SMART_FALLBACK` notice without a
  reason; under strict mode `SMART_UNAVAILABLE`).
- [X] T037 [US2] Run-time fallback in `core/exec/smart.rs`: after a failed verification, re-run
  the job as accurate (the accurate `ClipSpec` is kept in the plan for this), emit the
  `SMART_FALLBACK` `JobWarning`, and set `SmartOutcome.fallback`; under strict mode fail the job
  with `SMART_UNAVAILABLE`.

  Done in `exec/clip.rs` (US1); `JobWarning` gains `detail` for the reason.
- [X] T038 [US2] CLI: `--no-smart-fallback` in `cli/commands/{clip,batch}.rs`; the result line
  `accurate (smart fallback: <reason>)`; the strict exit codes in `cli/main.rs`'s existing
  mapping (3 for planning issues, 1 for failed jobs).
- [X] T039 [US2] `doctor` (FR-017): `core/doctor.rs` reports `smart: {available, reason}` from
  `Caps.encoders`; `cli/commands/doctor.rs` prints `smart cut: available (libx264)` or
  `smart cut: unavailable — <reason>`; `cli-tests/doctor.rs` case and JSON snapshot.

**Checkpoint**: `quickstart.md` › the fallback example; T031–T034 green; senior-code-reviewer.

---

## Phase 5: User Story 3 — Know before running what will be copied (Priority: P3)

**Goal**: the dry run shows, per job and for the batch, what will be copied, re-encoded or fall
back.

**Independent Test**: a dry run of five smart clips (three eligible, two falling back) lists the
spans, shares and reasons, and writes nothing.

- [X] T040 [P] [US3] Tests first in `cli-tests/batch.rs` › `smart_dry_run` and
  `contract_clip_json.rs` › `smart_plan_json`: the human lines `smart: head … · copy … · tail … ·
  NN% copied` and `smart → accurate: <message>`; the batch line `smart: 3 jobs; fallback: 2
  (open_gop 1, vfr 1)`; the JSON `smart` block per job and the plan-level `smart` totals
  (contracts/json-output.md); no file or scratch folder written. Done; strict mode's two
  `SMART_UNAVAILABLE` issues carry `detail.reason` in the JSON too.
- [X] T041 [US3] `SmartSummary` on `PlannedJob` and `SmartTotals` on `Plan` in
  `core/plan/mod.rs` (data-model › Planned job additions), filled from `check`; `pipeline`
  lists the five argv (contracts/json-output.md). Done; `PlannedJob::commands()` lists them, and
  the job document adds `cwd` (the join reads `list.txt` from the scratch folder).
- [X] T042 [US3] Render the per-job and batch lines in `cli/render.rs`; JSON is the serde form.
  T040 passes. Done; the dry run shows the `smart →` line in place of the fallback warning.

**Checkpoint**: T040 green; the JSON contract matches contracts/json-output.md word for word.

---

## Phase 6: User Story 4 — Smart mode in batches and manifests (Priority: P4)

**Goal**: the mode travels with the work, and CLI and desktop give the same outputs.

**Independent Test**: equivalent JSON and CSV manifests with `mode: smart` at different levels
give identical outputs from the CLI and the desktop app; an export round-trips the mode.

- [X] T043 [P] [US4] Tests first in `core-tests/us3_batch.rs` › `smart_*`: JSON and CSV manifests
  with smart at run, video and clip level (contracts/manifest-smart.md example) give identical
  outputs; the most specific level wins; 0.3.0 manifests give byte-identical outputs to the
  T001 baseline (SC-009). Done: `us3_batch::smart_json_and_csv_manifests_give_identical_outputs`
  and `smart_absent_from_0_3_0_manifests` (no smart fields in a 0.3.0 plan; the 0.3.0 commands are
  pinned by snapshots unchanged since the baseline commit `f286b44`, only new ones added).
- [X] T044 [P] [US4] Tests first in `core-tests/smart_us4.rs` › `csv_writer_round_trip`: the
  writer emits `smart_fallback` after `mode` only when a row sets it; the existing header
  snapshot is unchanged; write → read gives the same `Options`.
- [X] T045 [P] [US4] Tests first in `apps/desktop/src-tauri/tests/bridge.rs` › `smart_parity`
  (FR-019): an exported desktop queue with smart clips run by `rmedia batch` gives outputs
  identical to the app's run. Done for both the JSON and the CSV export.
- [X] T046 [US4] CSV writer change in `core/input/manifest_csv.rs` (T044) and `export_manifest`
  keeping `mode` and `smart_fallback` per clip (`desk/ops.rs`). T043–T045 pass. Done: only the
  CSV writer changed; `export_manifest` already kept both (JSON passes the queue through, CSV
  uses the writer).

**Checkpoint**: T043–T045 green; `quickstart.md` › Automated checks › Manifests and parity.

---

## Phase 7: User Story 5 — Smart mode in the desktop app with keyframe markers (Priority: P5)

**Goal**: Smart next to Fast and Accurate, keyframe markers, and a live head/copy/tail split.

**Independent Test**: open an H.264 video, choose Smart, select a range across keyframes; the
markers match the keyframes, the bands show the spans, and the run report matches.

### Tests for User Story 5 ⚠️

- [X] T047 [P] [US5] `apps/desktop/src-tauri/tests/bridge.rs` › `keyframes_*`: the `keyframes`
  command returns the index for `SmartMp4` (2 s ticks), serves a second request from the cache,
  invalidates on a changed mtime, refuses a path that is not an allowed source, and
  `cancel_keyframes` stops a running scan.
  Done: `bridge::keyframes_index_cache_and_cancel` (a refused path, 2 s ticks, a cache hit,
  invalidation on mtime, cancel).
- [X] T048 [P] [US5] `bridge.rs` › `smart_plan_*`: `smart_plan` returns the same `SmartSummary`
  as `rmedia clip --mode smart --dry-run --json` for the same range; a newer call cancels the
  older one; a fallback source returns its reason.
  Done: `bridge::smart_plan_matches_the_cli`. It returns `{summary, notices}` so the page can
  show the fallback notice's message.
- [X] T049 [P] [US5] Component tests (`npm test`, the existing vitest setup) for
  `ui/components/Player.svelte` (ticks at the index times; head/copy/tail bands and the share
  under the timeline; the fallback message), `Queue.svelte` (Smart label, "Only smart (no
  fallback)"), `RunPanel.svelte` ("Smart · 92% copied", "Accurate (smart fallback: …)") and
  `Settings.svelte` (Smart default, doctor line).
  Done: the layout and labels are pure functions in `src/lib/stores/smart.ts`, and the state
  lives in `smart.svelte.ts` (`smart.test.ts`, `smart.svelte.test.ts`). The vitest setup runs in
  node without a component DOM, so the components themselves are checked by the Xvfb checkpoint.

### Implementation for User Story 5

- [X] T050 [US5] `keyframes` and `cancel_keyframes` in `desk/commands.rs`, with the per-source
  cache (key: path, size, mtime) and request cancellation in `desk/ops.rs`, calling
  `smart::keyframes` (R11); register them in `desk/lib.rs` and the capability file.
  Done: app commands need no capability entry.
- [X] T051 [US5] `smart_plan` in `desk/commands.rs` and `desk/ops.rs`, calling the core planning
  for one range (no media logic in the desktop crate; Principle I); newer-call cancellation as
  `gif_estimate` does.
  Done: it takes `span: RangeArg` (the type the page already uses) and returns
  `SmartPlanResult {summary, notices}`.
- [X] T052 [US5] Regenerate bindings (`cargo xtask gen-bindings`) for `KeyframeIndex`,
  `KeyframeEntry`, `SmartSummary`, `SmartOutcome`, `FallbackReason` and the new `Mode` value;
  commit `ui/bindings`.
  Done: no change; the bindings were generated with US1–US3.
- [X] T053 [US5] UI: Player ticks (loaded without blocking the view, scenario 1) and bands
  (`ui/components/Player.svelte`); Smart in the mode choice and the strict option
  (`Queue.svelte`); the result labels (`RunPanel.svelte`); the default mode and the doctor line
  (`Settings.svelte`). T047–T049 pass.
  Done: the per-clip strict option is a "Smart" / "Smart only" pill on the clip row.

**Checkpoint**: run `quickstart.md` › Desktop under Xvfb; check SC-007 (3 interactions, markers
≤ 2 s on a 10-minute source). npm check and test, the desktop tests, senior-code-reviewer and
security-auditor (new IPC commands, the cache keyed by path).

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T054 [P] Update the docs:
  - `docs/api/cli-reference.md`: `--mode smart`, `--no-smart-fallback`, the dry-run and result
    lines, the doctor line, the exit codes;
  - `docs/api/json-output.md`: the `smart` blocks, totals, stages and outcome;
  - `docs/api/file-formats.md`: `mode: smart`, `smart_fallback` in JSON and CSV.
- [ ] T055 [P] Update the docs:
  - `docs/user/{cli-guide,desktop-guide,troubleshooting}.md`: choosing a mode, what each
    fallback reason means and what to do about it;
  - `docs/api/desktop-ipc.md`: `keyframes`, `cancel_keyframes`, `smart_plan`;
  - `docs/administration/configuration.md`: `defaults.mode` and `defaults.smart_fallback`.
- [ ] T056 [P] Update the docs:
  - `docs/architecture/{overview,domains}.md`: the `smart/` module and the multi-stage clip job;
  - `docs/reference/`: the three issue codes and the 12 fallback reasons, with a glossary entry
    for GOP, keyframe, open GOP and seam;
  - `docs/developer/testing.md`: the new fixtures and helpers.
- [ ] T057 Write the release notes `docs/releases/0.4.0.md` (status "in development"); update the
  `README.md` feature list, `docs/README.md` and `docs/roadmap.md`.
- [ ] T058 Run all of `quickstart.md` on Linux; record the results, and the Windows results from
  CI, in `checklists/validation.md`. Owner checks stay open: playback of a smart mp4 on phones
  (S1 rows) and a Reddit upload.
- [ ] T059 Final senior-code-reviewer and security-auditor passes over all 004 changes, fixing
  the high and medium findings. Then:
  - mark the tasks `[X]`;
  - write `specs/004-smart-cut/progress.md`;
  - update `docs/roadmap.md` and the `CLAUDE.md` active feature.

  Stop for review. The version bump to 0.4.0 and the tag follow
  `docs/operations/build-and-release.md` › Cut a Release.

---

## Dependencies & Execution Order

- **Setup (T001–T004)** → **Foundational (T005–T014)** → user stories.
- **Inside Foundational:**
  - T005 → T006 → T007 → T008 (shared `request.rs`, parsers, `plan/mod.rs`).
  - T009, T010 and T011 run alongside them (separate files). T011 needs T004.
  - The spikes T012–T014 need only T003 and can start at once; T012's result sets the
    `sample_entry` constant that T021 and T025 use.
- **US1 (T015–T030)** is the base for every story. Its tests T015–T023 run in parallel; then
  T024 → T025 → T026 → T027 → T028 → T029 → T030.
- **US2** needs US1 (it extends `check`, `plan/mod.rs` and `exec/smart.rs`).
- **US3** needs US2 (the dry run shows the fallback reasons).
- **US4** needs US1 only (T005–T007 already parse the mode); it may run beside US3, but T046
  touches `desk/ops.rs`, so keep it before US5.
- **US5** needs US3 (`SmartSummary`) and US4 (export).
- **Polish** after US5 (docs for US1–US4 may start after US4).

```
Setup → Foundational ─┬─ mode, types, index ─► US1 → US2 → US3 ─┬─► US5 → Polish
                      └─ spikes S1–S3 ─► T021/T025               └─ US4 ┘
```

## Parallel Examples
- Setup: T003 and T004 together.
- Foundational: T009, T010 and T011 while T005 → T008 run in sequence; T012, T013 and T014
  together.
- US1 tests: T015–T023 together; then the implementation chain.
- US2 tests: T031–T034 together; then T035 → T036 → T037 → T038; T039 alongside.
- US4: T043, T044 and T045 together.
- US5: T047, T048 and T049 together; T050 and T051 then T052 → T053.

## Implementation Strategy
1. **MVP** = Setup + Foundational + US1 → `rmedia clip --mode smart` on eligible H.264 sources,
   verified and cancellable (ineligible sources quietly run as accurate until US2). Stop and
   validate against quickstart.md › Manual run.
2. Add US2 (safe fallback, the condition for shipping the mode at all), then US3 (dry run) and
   US4 (manifests), each an independently testable CLI increment.
3. US5 desktop markers and live split.
4. Every checkpoint: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace` green on both OSes, CI green on `main`, senior-code-reviewer, and
   security-auditor where noted.
5. Owner-gated items:
   - S1's phone rows (Android, iPhone) and WebView2 if no Windows machine is reachable;
   - a Reddit upload of a smart mp4;
   - the 0.4.0 release.
