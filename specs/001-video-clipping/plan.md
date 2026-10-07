# Implementation Plan: Video Clipping Base

**Branch**: `001-video-clipping` | **Date**: 2026-10-06 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/001-video-clipping/spec.md`

## Summary

Deliver `rmedia`, a Linux + Windows toolkit that cuts clips from videos by manual timestamps,
multi-clip text files, and batch manifests (JSON or CSV), and discovers video files in a
directory tree. A single Rust core library (`rmedia-core`) owns all media logic and drives bundled
FFmpeg/ffprobe as subprocesses; a `clap` CLI and a Tauri v2 (Svelte 5) desktop app are thin
surfaces over it. Fast mode stream-copies (keyframe-snapped, reported); accurate mode re-encodes
frame-exactly. The job executor is built to accept further job kinds: mobile upscaling (feature 002)
and GIF export (feature 003).

## Technical Context

**Language/Version**: Rust 1.94 (edition 2024) for core/CLI/desktop backend; TypeScript 6 + Svelte 5
for the desktop frontend (Node 24 for tooling)

**Primary Dependencies**: tokio 1.x, tokio-util (CancellationToken), process-wrap 10 (process groups
/ Windows Job Objects), clap 4, indicatif 0.18, walkdir 2, globset 0.4, serde/serde_json, csv 1,
thiserror 2, directories 6, which 8, tracing, ts-rs 12, Tauri **2.x** (pinned; 3.0 is alpha) +
tauri-plugin-dialog; external: FFmpeg/ffprobe ≥ 6.0 (bundled as `rmedia-ffmpeg`/`rmedia-ffprobe`)

**Storage**: Files only — `settings.json` (config dir), in-flight journal (state dir), preview
proxy/thumbnail cache (cache dir) via `directories::ProjectDirs`

**Testing**: `cargo test` (unit, integration with lavfi-generated fixtures, assert_cmd CLI tests,
insta argv snapshots); vitest for frontend stores; CI matrix ubuntu-22.04 + windows-latest

**Target Platform**: Linux x86_64 (glibc ≥ 2.35; X11 + Wayland) and Windows 10/11 x86_64

**Project Type**: Rust workspace — library + CLI + desktop app (Tauri)

**Performance Goals**: fast-mode clip < 5 s regardless of length (SC-001); scan 10k files / 1k
videos < 60 s (SC-003); cancel < 2 s (SC-007); progress updates ≤ 10 Hz per job

**Constraints**: never modify sources; no shell interpolation; no partial outputs; works offline;
CPU-only machines fully supported; GUI must cope with webviews that cannot play the source codec

**Scale/Scope**: single user, local files; batches of hundreds of clips; trees of 10k+ files

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-design | Post-design |
|---|---|---|---|
| I. One Core, Thin Surfaces | All media logic in `rmedia-core`; CLI & Tauri crates only adapt I/O | PASS | PASS — Tauri commands and CLI subcommands map 1:1 to core functions ([contracts/tauri-commands.md](./contracts/tauri-commands.md)); preview logic also in core |
| II. CLI-First, Scriptable | `--json`, `--dry-run`, stable exit codes, stdout/stderr split | PASS | PASS — [contracts/cli.md](./contracts/cli.md) |
| III. Test-First with Real Media | lavfi fixtures, ffprobe verification, table-driven parsers, no binary fixtures | PASS | PASS — `rmedia-testkit` generates fixtures at test time |
| IV. Linux = Windows | CI matrix, no shell, Job Objects/process groups, path edge cases | PASS | PASS — process-wrap + `file:` paths + Windows sanitizing |
| V. Inputs Are Sacred | never touch sources; default skip; argv only; partials removed | PASS | PASS — `.part` temp + verified no-clobber commit + journal |
| VI. Graceful Degradation | `doctor`, capability detection, CPU path always works | PASS | PASS — encoder availability checked at plan time |
| VII. Simplicity & Evidence | choices cite `research/`; complexity justified | PASS | PASS — YAML/TOML/probe-cache/SQLite/magic-sniffing cut; see research.md |

No violations; Complexity Tracking lists only deliberate extra components.

## Project Structure

### Documentation (this feature)

```text
specs/001-video-clipping/
├── plan.md              # This file
├── research.md          # Phase 0 decisions
├── data-model.md        # Phase 1 entities & types
├── quickstart.md        # Phase 1 validation guide
├── contracts/
│   ├── cli.md
│   ├── cliplist-format.md
│   ├── manifest.schema.json
│   ├── manifest-csv.md
│   ├── json-output.md
│   └── tauri-commands.md
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks output
```

### Source Code (repository root)

```text
Cargo.toml                    # workspace, shared deps & lints
rust-toolchain.toml
crates/
├── rmedia-core/src/
│   ├── lib.rs
│   ├── time.rs               # Ts parsing/formatting
│   ├── request.rs            # ClipRequest, SourceRequest, BatchRequest, Options, Origin
│   ├── input/{cliplist,manifest_json,manifest_csv,pathlist}.rs
│   ├── tools.rs  doctor.rs  probe.rs
│   ├── discover.rs
│   ├── plan/{mod,naming,streams,ffargs}.rs
│   ├── exec/{mod,process,progress,commit,journal,clip}.rs
│   ├── paths.rs, pathser.rs  # Windows-safe canonical paths; path serialization
│   └── preview.rs  event.rs  settings.rs  error.rs
├── rmedia-core/tests/        # integration tests (via rmedia-testkit)
├── rmedia-cli/src/{main,render,commands/*}.rs   # bin "rmedia"
└── rmedia-testkit/src/lib.rs                    # fixtures + ffprobe asserts (publish = false)
apps/desktop/
├── package.json  vite.config.ts  src/{App.svelte, lib/{stores,components,bindings}}
└── src-tauri/{Cargo.toml, tauri.conf.json, capabilities/, src/{main,commands}.rs}
xtask/src/main.rs            # fetch-tools (pinned + sha256), gen-bindings, notices
.github/workflows/ci.yml
```

**Structure Decision**: Rust workspace with one core library, one CLI binary, a test-support crate,
a Tauri app under `apps/desktop`, and an `xtask` for tooling. Rationale in research.md §R1.

## Phasing (maps to user stories)

| Phase | Stories | Exit criteria |
|---|---|---|
| 0 Setup | — | workspace + CI green on both OSes; `rmedia doctor` works; fixtures generate |
| 1 MVP | US1 (P1) | US1 scenarios pass; SC-001, SC-002, SC-007 tests; GUI preview spike started |
| 2 | US2 (P2) | clip lists, naming, collisions, concurrency, summary, exit codes |
| 3 | US4 (P2) | `scan`, directory inputs, SC-003 bench |
| 4 | US3 (P3) | JSON+CSV manifests, export, SC-004 |
| 5 | US5 (P2) | desktop app; SC-008 parity |
| 6 | — | installers with bundled FFmpeg, notices (FR-027) |

## Forward Compatibility

- **002 mobile-upscale**: adds `JobSpec::Upscale`, a `Resource::Gpu` semaphore, Vulkan/upscaler
  detection in `doctor`, and device presets as data files.
- **003 gif-export**: adds `JobSpec::Gif` (two-pass palette pipeline, size-budget search). It
  reuses `Segment`, the executor, events, and commit. Research: `research/05-gif-export.md`.

### Required refactors before adding job kinds (senior review A1–A5, 2026-10-06)

- **A1**: clip-specific fields (`requested`/`predicted` spans, `mode`, `media_len`) move into
  `ClipSpec`; `JobOutcome::Succeeded` carries a `JobMeasure` enum instead of a clip `Span`.
- **A2**: multi-stage jobs — a job-kind `execute(&mut JobCtx)` that can run several processes;
  `JobCtx` offers `scratch_dir()` and journals every intermediate file (not just `job.temp`).
- **A3**: weighted multi-stage progress, and per-resource semaphores (GPU vs CPU) in the scheduler.
- **A4**: `Caps` grows filters/hwaccel/GPU/model availability; issue codes become a typed enum.
- **A5**: extract output resolution/claiming/collision handling from `plan_clip` for reuse.

## Complexity Tracking

| Component | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| `rmedia-testkit` crate | Fixture generation shared by core, CLI and desktop tests | Duplicating fixture code in each crate violates DRY and drifts |
| `xtask` crate | Pinned, checksummed FFmpeg fetch + bindings generation across OSes | Shell scripts are not portable to Windows (Principle IV) |
| Preview proxy cache | Webviews cannot play many source codecs (research.md §R8) | Direct playback only would break FR-025 for MKV/AVI/HEVC sources |
| In-flight journal | Removes partials after crash/kill (FR-021, Principle V) | Without it, a killed process leaves `.part` files forever |
