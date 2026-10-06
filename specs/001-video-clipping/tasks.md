---
description: "Task list for 001 Video Clipping Base"
---

# Tasks: Video Clipping Base

**Input**: Design documents from `specs/001-video-clipping/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: REQUIRED. Constitution Principle III (test-first with real media) is non-negotiable.
Within each story, write the test tasks first and confirm they FAIL before implementing.

**Organization**: Grouped by user story. Order: US1 (P1) → US2 (P2) → US4 (P2) → US3 (P3) →
US5 (P2). US3 comes before US5 because the GUI imports and exports manifests.

## Format: `[ID] [P?] [Story] Description`
- **[P]**: parallelizable (different files, no dependency on an incomplete task)
- Paths are relative to the repo root (see plan.md › Project Structure)

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Create workspace `Cargo.toml` (members: crates/rmedia-core, crates/rmedia-cli, crates/rmedia-testkit, xtask, apps/desktop/src-tauri; `[workspace.dependencies]` pinned per plan.md; `[workspace.lints]` with `unsafe_code = "deny"` except where justified) and `rust-toolchain.toml` (1.94, rustfmt, clippy)
- [X] T002 [P] Scaffold `crates/rmedia-core` (lib, edition 2024) with empty modules from plan.md: `src/lib.rs`, `time.rs`, `request.rs`, `input/mod.rs`, `tools.rs`, `doctor.rs`, `probe.rs`, `discover.rs`, `plan/mod.rs`, `exec/mod.rs`, `event.rs`, `settings.rs`, `error.rs`, `preview.rs` — *note: `input/`, `discover`, `preview` modules are created in their own phases (US2/US4/US5) rather than as empty stubs*
- [X] T003 [P] Scaffold `crates/rmedia-cli` (bin `rmedia`, clap derive skeleton with subcommands `clip|batch|scan|doctor`) in `crates/rmedia-cli/src/main.rs` — *note: `batch`/`scan` subcommands are added with US3/US4; no stubs shipped*
- [X] T004 [P] Scaffold `crates/rmedia-testkit` (`publish = false`) in `crates/rmedia-testkit/src/lib.rs`
- [X] T005 [P] Add `.gitignore` (target/, node_modules/, binaries/, apps/desktop/dist/), `.editorconfig`, `rustfmt.toml`, `deny.toml` (cargo-deny licences/advisories) at the repo root
- [X] T006 [P] Create `.github/workflows/ci.yml`: matrix ubuntu-22.04 + windows-latest; steps fmt --check, clippy -D warnings, `cargo xtask fetch-tools`, `cargo test --workspace` with `RMEDIA_REQUIRE_FFMPEG=1`; extra Linux job against distro ffmpeg; cargo-deny job — *note: written and validated locally (fmt, clippy, tests, windows-gnu check); first GitHub run pending a remote*
- [X] T007 Implement `xtask` in `xtask/src/main.rs`: `fetch-tools` downloads pinned BtbN GPL builds (linux64 / win64) by URL + SHA-256 from `xtask/tools.lock.json`, extracts ffmpeg/ffprobe to `binaries/rmedia-ffmpeg-<target-triple>[.exe]` and `binaries/rmedia-ffprobe-<target-triple>[.exe]`, and is idempotent

---

## Phase 2: Foundational (Blocking Prerequisites)

**⚠️ No user-story work starts until this phase is complete.**

- [X] T008 Implement fixture generator in `crates/rmedia-testkit/src/lib.rs`: `fixture(Kind) -> PathBuf`, generated once into `target/rmedia-fixtures/<spec-hash>/` via temp + rename (safe for parallel tests); kinds `Gop50Mp4` (testsrc2 320x240@25, 60 s, `-g 50 -keyint_min 50 -sc_threshold 0 -bf 2`, sine aac), `Hd60Mp4` (1920x1080), `MultiMkv` (2 audio + SRT), `SmallWebm`, `NoAudioMp4`, `VfrMp4`, `OffsetTs` (`-output_ts_offset 10`), `CoverArtM4a`, `FakeMp4` (text), `PhotoMp4` (JPEG); helpers `probe_duration`, `probe_streams`, `sha256`, `first_frame_psnr_vs(source, t)`
- [X] T009 [P] Define `Error` enum with stable `code()` strings (INVALID_INPUT, SOURCE_MISSING, PROBE_FAILED, OUTPUT_EXISTS, OUTPUT_UNWRITABLE, DISK_FULL, ENGINE_FAILED, VERIFY_FAILED, CANCELLED, ENGINE_MISSING) in `crates/rmedia-core/src/error.rs`
- [X] T010 [P] Write table-driven tests (≈60 rows, valid + malformed, per data-model.md `Ts`) in `crates/rmedia-core/src/time.rs` `#[cfg(test)]`; confirm they fail
- [X] T011 Implement `Ts` (i64 µs) parse/format (display `HH:MM:SS.mmm`, filename `HH-MM-SS.mmm`, ffmpeg `S.ffffff`), and `EndSpec` parsing (`end`, `+dur`) in `crates/rmedia-core/src/time.rs`
- [X] T012 [P] Write tests for tool location order (explicit → bundled `rmedia-ffmpeg` beside `current_exe()` → PATH), version parsing (`ffmpeg version 6.1.1-3ubuntu5`, `n7.1`, `N-11xxxx-g…` git builds) and minimum 6.0 in `crates/rmedia-core/src/tools.rs`
- [X] T013 Implement `tools::locate()` returning `Tools { ffmpeg, ffprobe, source, version }` in `crates/rmedia-core/src/tools.rs`
- [X] T014 Implement `probe::probe(path) -> MediaInfo` (ffprobe `-show_format -show_streams -of json`, `file:` paths, attached_pic detection) and its tests against fixtures in `crates/rmedia-core/src/probe.rs`
- [X] T015 [P] Define `event.rs` types (`Event`, `JobOutcome`, `RunSummary`, `ScanEvent`, `EventSink` trait) with serde and `ts-rs` derive behind feature `ts` in `crates/rmedia-core/src/event.rs` — *note: ts-rs derives land with the bindings task (T062)*
- [X] T016 Write process tests in `crates/rmedia-core/tests/process.rs`: spawn a long ffmpeg encode, cancel → child tree gone within 2 s (check with sysinfo); drop handle → killed; Unix: parent SIGKILLed → child dies (PDEATHSIG); Windows: Job Object closes children
- [X] T017 Implement `exec::process` in `crates/rmedia-core/src/exec/process.rs`: process-wrap with process group + kill-on-drop + `PR_SET_PDEATHSIG` on a dedicated spawner thread (Unix); Job Object `KILL_ON_JOB_CLOSE` + `CREATE_NO_WINDOW` (Windows); concurrent stdout/stderr readers; 50-line stderr ring buffer — *note: spawns happen on long-lived tokio worker threads (never `spawn_blocking`), which satisfies the PDEATHSIG thread caveat without a dedicated spawner thread; verified by the SIGKILL test*
- [X] T018 [P] Write tests for the `-progress` parser using canned key=value blocks (including `N/A`, `progress=end`) in `crates/rmedia-core/src/exec/progress.rs`
- [X] T019 Implement the progress parser → `JobProgress` (fraction clamped, ETA from `speed`, throttle ≤ 10 Hz) in `crates/rmedia-core/src/exec/progress.rs`
- [X] T020 Implement `doctor::run() -> DoctorReport` (tool paths and source, version, encoders `libx264 aac libvpx-vp9 libopus`, muxers, accurate-mode containers, problems list) in `crates/rmedia-core/src/doctor.rs`, plus `rmedia doctor [--json]` in `crates/rmedia-cli/src/commands/doctor.rs` (exit 4 when unusable)

**Checkpoint**: CI green on both OSes; `rmedia doctor` works; fixtures generate on Windows.

---

## Phase 3: User Story 1 — Single clip with manual timestamps (P1) 🎯 MVP

**Goal**: `rmedia clip IN -s S (-e E | -d +D) [--mode fast|accurate]` produces one verified clip.

**Independent test**: Gop50 fixture, `-s 10.5 -e 20`. Fast mode gives start 10.0, end 20 and a
report that it was adjusted. Accurate mode gives 9.5 s ±1 frame and a first frame that best matches
source t=10.5. The source sha256 is unchanged.

### Tests for US1 (write first; must fail)
- [X] T021 [P] [US1] argv snapshot tests (insta) for fast and accurate × mp4/mkv/webm/avi(→mp4 fallback) × stream layouts, using committed ffprobe-JSON text inputs in `crates/rmedia-core/tests/snapshots_ffargs.rs` and `crates/rmedia-core/tests/data/probe/*.json`
- [X] T022 [P] [US1] Planner unit tests (end clamp with warning, end<start error, start≥duration error, duration form, output≠source, attached_pic and data streams never mapped, subtitle conversion and drop warnings) in `crates/rmedia-core/src/plan/mod.rs` — *note: covered by `tests/us1_clip.rs` (validation, clamping) and `tests/snapshots_ffargs.rs` (stream mapping, subtitle conversion/drop warnings)*
- [X] T023 [P] [US1] Integration tests for US1 acceptance scenarios 1–5 (durations via ffprobe, fast-mode snap 10.5→10.0 with `adjusted=true`, accurate PSNR first-frame check, source sha256 unchanged, OffsetTs start_time handling, VfrMp4 accurate) in `crates/rmedia-core/tests/us1_clip.rs`
- [X] T024 [P] [US1] Commit/cleanup tests (existing output + Skip → Skipped; Overwrite; failure leaves no `.part`; read-only output dir → OUTPUT_UNWRITABLE with no partial file; disk-full mapped to DISK_FULL via stderr classification unit test; journal cleanup of dead pid on start) in `crates/rmedia-core/tests/commit.rs`
- [X] T025 [P] [US1] CLI tests with assert_cmd (exit codes 0/3/4, `--dry-run` writes nothing, `--json` NDJSON ends with a `summary` line, odd paths `-dash.mp4`, `a b ü.mp4`; Windows-only: path > 260 chars and `\\?\` prefixed paths) in `crates/rmedia-cli/tests/clip_single.rs`

### Implementation for US1
- [X] T026 [P] [US1] Implement `request.rs` types (`Options`, `PartialOptions` layering, `ClipRequest`, `SourceRequest`, `BatchRequest`, `Origin`, `StreamSelection`, `Mode`, `OnExists`) in `crates/rmedia-core/src/request.rs`
- [X] T027 [P] [US1] Implement the container compatibility and encode-profile tables (research.md R4) in `crates/rmedia-core/src/plan/streams.rs`
- [X] T028 [US1] Implement pure argv builders `fast_args` / `accurate_args` (common prefix, `file:` paths, explicit `-map`, `-avoid_negative_ts make_zero`, `+faststart`, `-fps_mode passthrough`, explicit `-f` muxer) in `crates/rmedia-core/src/plan/ffargs.rs`
- [X] T029 [US1] Implement `probe::keyframe_at_or_before(path, t)` via `-read_intervals "<t+start_time>%+#1"` in `crates/rmedia-core/src/probe.rs`
- [X] T030 [US1] Implement naming template + Windows-safe sanitizing (reserved names, `<>:"/\|?*`, trailing dot/space) for a single job in `crates/rmedia-core/src/plan/naming.rs`
- [X] T031 [US1] Implement `plan::plan(&BatchRequest, &Tools, &Caps) -> Plan` (probe sources, validate, clamp, predict snap, resolve output, disposition, warnings, argv) in `crates/rmedia-core/src/plan/mod.rs`
- [X] T032 [US1] Implement `exec::commit` (`.part` naming, ffprobe verify, hard_link no-clobber with rename fallback, Overwrite via rename/`MoveFileExW`, Rename suffixing, delete retry ≤ 1 s on Windows) in `crates/rmedia-core/src/exec/commit.rs`
- [X] T033 [US1] Implement `exec::journal` (`state_dir/inflight/<pid>.json`, `purge_stale()` called at startup) in `crates/rmedia-core/src/exec/journal.rs`
- [X] T034 [US1] Implement the `exec::run` scheduler (semaphore, `JobSpec::execute(&mut JobCtx)` trait so 002/003 can add kinds, per-job child cancel tokens, measured `actual_start`/`actual_end`, `RunProgress`, `RunSummary`) in `crates/rmedia-core/src/exec/mod.rs`
- [X] T035 [US1] Implement CLI `clip` with `-s/-e/-d`, shared flags, and Ctrl-C handling (first press cancels gracefully and exits 130; second exits immediately) in `crates/rmedia-cli/src/commands/clip.rs`
- [X] T036 [US1] Implement the CLI renderers (human: indicatif bars to stderr plus a summary table; `--json`: NDJSON per contracts/json-output.md; dry-run plan document) in `crates/rmedia-cli/src/render.rs`
- [X] T037 [US1] Add the SC-001 benchmark (fast clip from Hd60Mp4 < 5 s) as an `#[ignore]` test in `crates/rmedia-core/tests/bench.rs`
- [X] T038 [US1] GUI preview spike (de-risk US5): minimal Tauri 2 app in `apps/desktop/` that plays a proxy mp4 through the asset protocol with seeking on WebKitGTK; record the result (works, or needs the localhost range server fallback) in `specs/001-video-clipping/research.md` §R8 — *result recorded in research.md §R8: asset protocol cannot feed `<video>` on WebKitGTK; US5 uses a localhost range server*

**Checkpoint**: US1 independently shippable as a CLI MVP. Run senior-code-reviewer and
security-auditor (process spawning, path handling).

---

## Phase 4: User Story 2 — Multiple clips via clip list (P2)

**Goal**: `rmedia clip IN -l clips.txt` produces one output per line.

**Independent test**: a 3-line list plus comments gives 3 correctly named clips. A malformed line 2
gives `file:2: reason`, exit 3 and no files. A failure injected into one clip still lets the other
two succeed.

### Tests for US2
- [X] T039 [P] [US2] Clip-list parser table tests (BOM, CRLF, comments, decimal-comma rule, names with spaces, `end`, `+dur`, every error listed with its line) in `crates/rmedia-core/src/input/cliplist.rs` — *note: comment rule refined to ` # ` so names like `goal #3` survive (contract updated)*
- [X] T040 [P] [US2] Integration tests for US2 scenarios 1–3, plus intra-plan collision (two clips → same name: error unless `--on-exists rename`) and concurrency `-j 3`, in `crates/rmedia-core/tests/us2_cliplist.rs`
- [X] T041 [P] [US2] CLI tests for `-l` (exit 1 when one job fails while the others succeed; summary counts) in `crates/rmedia-cli/tests/clip_list.rs`

### Implementation for US2
- [X] T042 [US2] Implement `input::cliplist::parse(path, sources) -> Result<BatchRequest, Vec<Issue>>` in `crates/rmedia-core/src/input/cliplist.rs`
- [X] T043 [US2] Extend naming with `{index}`, `{index:03}`, `{name}`, `{name-or-start}`, `{start}`, `{end}`, the multi-source default template, intra-plan duplicate detection, and Rename suffixing in `crates/rmedia-core/src/plan/naming.rs`
- [X] T044 [US2] Wire `-l` and multiple inputs into CLI `clip`, with default `jobs = clamp(ncpu/2,1,4)`, in `crates/rmedia-cli/src/commands/clip.rs`

**Checkpoint**: US1 and US2 pass together.

---

## Phase 5: User Story 4 — Discover videos in a directory (P2)

**Goal**: `rmedia scan DIR` lists real videos. Directories and `--inputs-from` work as clip inputs.

**Independent test**: a tree with 4 real videos (nested), FakeMp4, PhotoMp4, CoverArtM4a, a `.zip`,
hidden and `.part` files gives exactly the 4 videos and the right reject reasons.

### Tests for US4
- [ ] T045 [P] [US4] Discovery integration tests (US4 scenarios 1–5: rejects, zip ignored, non-recursive, include/exclude, hidden, symlink loop not followed on Unix, cancellation mid-scan) in `crates/rmedia-core/tests/us4_discover.rs`
- [ ] T046 [P] [US4] CLI tests for `scan --json` (schema per contracts/json-output.md), `--paths`, and `clip DIR -l` / `--inputs-from -` (per-source subfolders); Windows-only: scan of a UNC path `\\localhost\C$\...` when available in `crates/rmedia-cli/tests/scan.rs`
- [ ] T047 [P] [US4] SC-003 benchmark (10k files incl. 1,000 hard-linked videos < 60 s) as `#[ignore]` in `crates/rmedia-core/tests/bench.rs`

### Implementation for US4
- [ ] T048 [US4] Implement `discover::scan(roots, ScanOptions, sink, cancel) -> ScanResult` (walkdir, skip rules from research.md R6, extension allowlist, `buffer_unordered(ncpu)` probing, natural sort, `ScanEvent` stream) in `crates/rmedia-core/src/discover.rs`
- [ ] T049 [P] [US4] Implement `input::pathlist` (one path per line, `#` comments, relative to the file) in `crates/rmedia-core/src/input/pathlist.rs`
- [ ] T050 [US4] Implement CLI `scan` (human table, `--json`, `--paths`, scan flags) in `crates/rmedia-cli/src/commands/scan.rs`, and accept DIR / `--inputs-from` in `crates/rmedia-cli/src/commands/clip.rs`

**Checkpoint**: Run security-auditor on the discovery path handling (symlinks, long/UNC paths, non-UTF-8 names).

---

## Phase 6: User Story 3 — Batch clipping via manifest (P3)

**Goal**: `rmedia batch jobs.json|jobs.csv` runs a multi-video batch.

**Independent test**: 3 videos × 2 clips gives 6 outputs and a summary of 6/0. Equivalent JSON and
CSV give equal `Plan`s.

### Tests for US3
- [ ] T051 [P] [US3] JSON manifest tests (schema conformance with contracts/manifest.schema.json, `deny_unknown_fields`, syntax error reported by line:col, semantic errors by JSON path, override layering defaults < video < clip, relative paths) in `crates/rmedia-core/src/input/manifest_json.rs`
- [ ] T052 [P] [US3] CSV manifest tests (header required, unknown column error, grouping by input in first-appearance order, comments, errors by line) in `crates/rmedia-core/src/input/manifest_csv.rs`
- [ ] T053 [P] [US3] Integration tests for US3 scenarios 1–4 and SC-004 (100 clips over 20 videos with 1 injected failure → 99 succeed, exit 1) in `crates/rmedia-core/tests/us3_batch.rs`

### Implementation for US3
- [ ] T054 [P] [US3] Implement the JSON manifest parser and writer in `crates/rmedia-core/src/input/manifest_json.rs`
- [ ] T055 [P] [US3] Implement the CSV manifest parser and writer in `crates/rmedia-core/src/input/manifest_csv.rs`
- [ ] T056 [US3] Implement CLI `batch` with `--continue-on-missing` in `crates/rmedia-cli/src/commands/batch.rs`

---

## Phase 7: User Story 5 — Desktop application (P2)

**Goal**: The same workflows in a Tauri 2 + Svelte 5 app on Linux and Windows.

**Independent test**: load the Gop50 fixture, mark 2 clips, run, and get 2 outputs identical (by
hash) to the CLI run of the exported manifest. Cancelling all jobs takes < 2 s and leaves no partial
files.

### Tests for US5
- [ ] T057 [P] [US5] Bridge tests that call the command fns without a webview (plan/run/cancel; parity: export manifest → `rmedia batch` → equal sha256s, SC-008) in `apps/desktop/src-tauri/tests/bridge.rs`
- [ ] T058 [P] [US5] Vitest tests for the queue store and the event→UI-state reducer in `apps/desktop/src/lib/stores/*.test.ts`

### Implementation for US5
- [ ] T059 [US5] Implement `preview` in core (`frame_at` MJPEG pipe, `ensure_proxy` 480p H.264/VP8 with passthrough timestamps cached by sha256 with an LRU cap, `thumbnails`) with tests in `crates/rmedia-core/src/preview.rs`
- [ ] T060 [US5] Set up Tauri 2 (pinned 2.x): `apps/desktop/src-tauri/tauri.conf.json` (asset protocol scope = cache dir, CSP, `externalBin` for rmedia-ffmpeg/ffprobe), `capabilities/default.json` (dialog only), Vite + Svelte 5 + TS frontend
- [ ] T061 [US5] Implement the Tauri commands from contracts/tauri-commands.md as thin adapters (Channels for scan/run/proxy; cancel on window close) in `apps/desktop/src-tauri/src/commands.rs`
- [ ] T062 [US5] Generate ts-rs bindings with `cargo xtask gen-bindings` → `apps/desktop/src/lib/bindings/`, and add a CI diff check in `.github/workflows/ci.yml`
- [ ] T063 [P] [US5] Sources view (file/folder pickers, drag-drop, scan table with progress and cancel, export selected results as a path list or manifest for batch use per FR-012) in `apps/desktop/src/lib/components/Sources.svelte`
- [ ] T064 [P] [US5] Player view (direct play / proxy / still-frame fallback, timeline + thumbnail strip, I/O keys, frame step, times entered via `parse_time`) in `apps/desktop/src/lib/components/Player.svelte`
- [ ] T065 [P] [US5] Queue view (editable rows, predicted snap display, per-source audio/subtitle track selection per FR-015, import clip list or manifest, export manifest) in `apps/desktop/src/lib/components/Queue.svelte`
- [ ] T066 [P] [US5] Run panel (per-job and overall progress, ETA, cancel job/all, summary with reasons) in `apps/desktop/src/lib/components/RunPanel.svelte`
- [ ] T067 [P] [US5] Settings & Diagnostics (doctor report, defaults, jobs, proxy cache size) in `apps/desktop/src/lib/components/Settings.svelte`
- [ ] T068 [US5] App shell, navigation, design tokens, and light/dark themes in `apps/desktop/src/App.svelte` (premium-ui-designer review)

**Checkpoint**: Manual check per quickstart §4 on Linux X11 and Wayland, and on the Windows CI artifact. Run security-auditor on the Tauri command surface (paths from the webview, asset-protocol scope, CSP).

---

## Phase 8: Polish & Cross-Cutting

- [ ] T069 Packaging: Tauri bundles NSIS + MSI (windows-latest) and AppImage + deb (ubuntu-22.04), with the CLI `rmedia` shipped in the installer, in `apps/desktop/src-tauri/tauri.conf.json` and `.github/workflows/release.yml`
- [ ] T070 [P] `THIRD_PARTY_NOTICES.md` (cargo-about) plus FFmpeg licence text and a source offer for the pinned build in `licenses/` (FR-027)
- [ ] T071 [P] Write `README.md` with install, CLI usage, the clip-list/manifest formats (link to contracts), and doctor
- [ ] T072 Run the full quickstart.md validation on Linux, and on Windows via CI artifacts; record the results in `specs/001-video-clipping/checklists/validation.md`
- [ ] T073 Final senior-code-reviewer pass over the whole workspace; fix findings

---

## Dependencies & Execution Order

- **Setup (T001–T007)** → **Foundational (T008–T020)** → user stories.
- **US1 (T021–T038)** is the base for every other story: they need the planner, executor and commit.
- **US2** and **US4** can proceed in parallel after US1. US4's T050 touches `clip.rs`, so sequence it
  after T044.
- **US3** needs US2's naming work (T043).
- **US5** needs US1–US4 core APIs. The T038 spike runs early, during US1.
- **Polish** comes after US5.

```
Setup → Foundational → US1 ─┬─► US2 ─┬─► US3 ─┐
                            └─► US4 ─┘        ├─► US5 → Polish
                     (T038 spike) ────────────┘
```

## Parallel Examples
- Foundational: T009, T010, T012, T015, T018 together (separate files).
- US1 tests: T021–T025 together; then T026 and T027 in parallel; then T028 → T031 → T034.
- US3: T054 and T055 in parallel.
- US5 views: T063–T067 in parallel once T061/T062 are done.

## Implementation Strategy
1. **MVP** = Setup + Foundational + US1 → a usable `rmedia clip` CLI on both OSes. Stop and validate.
2. Add US2, then US4, then US3: each is an independently testable CLI increment.
3. US5 GUI, then packaging (FR-027).
4. Every phase checkpoint: tests green on both OSes and a senior-code-reviewer review.
   security-auditor reviews US1 (process/exec) and US4 (filesystem traversal).
