# rmedia Development Guide

## Overview

This guide is for engineers who build, test, and change rmedia. It covers the repository layout,
prerequisites, builds of the CLI and the desktop app, tests, linting, TypeScript bindings, the
Spec Kit workflow, project rules, and how to add a new job kind.

Related documents:

- [Testing](testing.md): test inventory and what each suite proves.
- [Build and Release](../operations/build-and-release.md): CI, packaging, and the FFmpeg supply
  chain.
- [Configuration](../administration/configuration.md): settings, environment variables, and file
  locations.

## Repository Layout

```text
Cargo.toml                 workspace: core, CLI, testkit, xtask (desktop crate excluded)
rust-toolchain.toml        Rust 1.94 + rustfmt + clippy
.cargo/config.toml         `cargo xtask` alias
deny.toml                  cargo-deny policy
crates/rmedia-core/        all media logic
  src/time.rs                timestamps (Ts) parsing and formatting
  src/request.rs             ClipRequest / SourceRequest / BatchRequest, Options, PartialOptions
  src/input/                 clip lists, JSON and CSV manifests, path lists
  src/tools.rs doctor.rs     engine location, version check, capability detection
  src/probe.rs discover.rs   ffprobe wrapper, folder scanning
  src/plan/                  planning: naming, stream selection, ffmpeg argv (ffargs.rs)
  src/exec/                  scheduler, process spawning, progress, commit, crash journal
  src/preview.rs             desktop preview frames, proxies, thumbnails, cache
  src/settings.rs            per-user directories and desktop settings
  tests/                     integration tests (lavfi fixtures)
crates/rmedia-cli/         the `rmedia` binary (clap); src/commands/{clip,batch,scan,doctor}.rs
crates/rmedia-testkit/     test fixtures generated with FFmpeg + ffprobe assertions
apps/desktop/              Tauri 2 + Svelte 5 + TypeScript desktop app
  src/                       App.svelte, lib/{components,stores,bindings}
  src-tauri/                 Rust backend: commands.rs, ops.rs, media_server.rs; tests/
xtask/                     build tasks: fetch-tools, pin-tools, gen-bindings, stage-cli, notices
binaries/                  fetched engine + staged CLI (git-ignored)
specs/001-video-clipping/  spec, plan, research, data model, contracts, tasks, checklists
research/                  research outlines 01 to 05
.specify/                  Spec Kit configuration, templates, constitution
```

The desktop backend (`apps/desktop/src-tauri`) is a standalone crate with its own `Cargo.lock`. It
is excluded from the root workspace because it needs webkit2gtk to build. Build and test it with
`--manifest-path apps/desktop/src-tauri/Cargo.toml`.

## Prerequisites

| Requirement | Version or package | Source |
|---|---|---|
| Rust | 1.94 with `rustfmt` and `clippy` (installed automatically by rustup from `rust-toolchain.toml`) | `rust-toolchain.toml` |
| Node.js | 24 | `README.md`, `plan.md` |
| FFmpeg | 6.0 or later on PATH, **or** the pinned build from `cargo xtask fetch-tools` | `tools.rs`, `quickstart.md` |
| `curl`, `tar` | Needed by `cargo xtask fetch-tools` | `xtask/src/main.rs` |
| Linux desktop build | `libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev patchelf` | `README.md`, `ci.yml` |
| Linux AppImage packaging | additionally `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-{base,good,bad} gstreamer1.0-libav` | `release.yml` |

> **Unverified:** The Node 24 requirement is stated in `README.md` and `plan.md`, but nothing
> enforces it (`package.json` has no `engines` field, and CI uses the runner's preinstalled Node).

The Tauri CLI comes from `apps/desktop/package.json` (`@tauri-apps/cli` 2.12.1); run it with
`npx tauri`.

## Build

### Fetch the bundled engine

```bash
cargo xtask fetch-tools
```

This step downloads the pinned, SHA-256-verified FFmpeg into `binaries/`. You need it for the
desktop build (Tauri sidecars), and it is the recommended engine for tests. Repeated runs reuse
verified binaries.

### CLI

```bash
cargo build -p rmedia-cli          # target/debug/rmedia
cargo run -p rmedia-cli -- doctor
```

A development build does not find the bundled engine automatically, because
`binaries/rmedia-ffmpeg-<triple>` is not named `rmedia-ffmpeg` and is not next to
`target/debug/rmedia`. Point it at the engine explicitly, or let it use FFmpeg from PATH:

```bash
export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
       RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
```

### Desktop app

The Tauri build requires all three sidecars in `binaries/`: the engine and the staged CLI.

```bash
cargo xtask fetch-tools
cargo xtask stage-cli               # builds the release CLI into binaries/rmedia-<triple>
cd apps/desktop
npm ci
npx tauri dev                       # Vite dev server on http://localhost:1420
npx tauri dev -- -- /path/video.mp4 # open files on startup
```

Release packages:

```bash
cargo xtask stage-cli
(cd apps/desktop && npx tauri build)   # deb + AppImage on Linux, NSIS + MSI on Windows
```

Output goes to `apps/desktop/src-tauri/target/release/bundle/`.

## Test

The short form (details in [Testing](testing.md)):

```bash
export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
       RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
cargo test --workspace                                  # core unit + integration + CLI
cargo test --workspace --release -- --ignored bench     # SC-001 / SC-003 timing benches
cargo build -p rmedia-cli && cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
(cd apps/desktop && npm test && npm run check)          # vitest + svelte-check
```

- **Fixtures** are real media generated at test time from FFmpeg `lavfi` sources (`testsrc2`,
  `sine`, `color`) by `rmedia-testkit`. They are cached under `target/rmedia-fixtures/<hash>/`.
  Never commit binary fixtures.
- **Without FFmpeg**, media tests print `skipping: ffmpeg not available` and pass. Set
  `RMEDIA_REQUIRE_FFMPEG=1` (as CI does) to make them fail instead.
- **Desktop backend tests** run the CLI binary for the parity test. Build it first
  (`cargo build -p rmedia-cli`), or set `RMEDIA_CLI_BIN` to its path.
- **argv snapshots** use `insta`. After an intended change to FFmpeg arguments, review and accept
  the new snapshots in `crates/rmedia-core/tests/snapshots/`.

## Lint and Format

Required before every commit (`CLAUDE.md`):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- `rustfmt.toml`: edition 2024, `max_width = 110`, `use_small_heuristics = "Max"`.
- Workspace lints: `unsafe_code = "deny"`, `missing_debug_implementations = "warn"`, all clippy
  lints warn (errors under `-D warnings`). The desktop crate also denies `unsafe_code`.
- `.editorconfig`: UTF-8, LF line endings, 4-space indent, 2 spaces for JSON, YAML, TS, Svelte, JS,
  CSS, HTML, and Markdown. `.gitattributes` enforces LF.
- Frontend: `npm run check` (svelte-check with `tsconfig.json`).
- Dependencies: `cargo deny check` (CI runs it for both Cargo manifests).

## TypeScript Bindings

The desktop frontend's types in `apps/desktop/src/lib/bindings/` are generated from `rmedia-core`
types with [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit them by hand.

```bash
cargo xtask gen-bindings
```

The task deletes the bindings directory and runs
`cargo test -q -p rmedia-core --features ts --lib export_bindings` with
`TS_RS_EXPORT_DIR=apps/desktop/src/lib/bindings` and `TS_RS_LARGE_INT=number`.

To expose a new core type to the frontend, annotate it as the existing types do:

```rust
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
```

Then regenerate and commit the bindings. CI fails if the committed bindings differ from the
generated ones.

## Project Rules

From `CLAUDE.md` and the constitution (`.specify/memory/constitution.md`, version 1.0.1):

| Rule | What it means in code |
|---|---|
| One core, thin surfaces (Principle I) | All media logic goes in `rmedia-core`. The CLI and Tauri crates only adapt I/O. Tauri commands and CLI subcommands map one to one to core functions. Anything available in one surface must be reachable from the other. |
| CLI first (Principle II) | Every feature works from the CLI before the GUI. Human output on stdout, diagnostics on stderr, `--json` for results, `--dry-run` for anything that writes, stable exit codes (`0`, `1`, `2`, `3`, `4`, `130`), deterministic outputs. |
| Test first with real media (Principle III, non-negotiable) | Write tests before the implementation and see them fail. Verify media with `ffprobe` (duration, resolution, codec, streams), not only exit codes. Table-driven parser tests that include malformed input. No binary fixtures. |
| Linux and Windows are equal (Principle IV) | Never launch through a shell. Handle spaces, Unicode, drive letters, and UNC paths. Kill child process trees on cancel (process groups on Linux, Job Objects on Windows). |
| Inputs are sacred (Principle V) | Never write to or delete a source file. Spawn tools with argument vectors only, with paths passed as `file:<path>` through `exec::process`. Write outputs to a `.part` file, verify it, then commit without overwriting (default `on_exists: skip`). Remove partial outputs. |
| Graceful degradation (Principle VI) | Detect capabilities at run time, report them in `doctor`, and fall back to the CPU path. A missing capability yields a clear message, never a crash. |
| Simplicity and evidence (Principle VII) | Cite `research/` for technology choices. Justify new complexity in the plan's Complexity Tracking table. |
| Commits | Conventional Commits. |

Each completed user story also needs unit and integration tests on both operating systems, a senior
code review, and a security review for any code that handles file paths or spawns processes
(constitution, Development Workflow).

## Spec Kit Workflow

The project is developed spec-first with Spec Kit. The constitution defines the sequence:

```text
constitution → specify → clarify → plan → tasks → analyze → implement
```

| Step | Command (Claude Code skill) | Output |
|---|---|---|
| Constitution | `/speckit-constitution` | `.specify/memory/constitution.md` |
| Specify | `/speckit-specify` | `specs/NNN-name/spec.md` |
| Clarify | `/speckit-clarify` | Answers encoded back into `spec.md` |
| Plan | `/speckit-plan` | `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md` |
| Tasks | `/speckit-tasks` | `tasks.md` |
| Analyze | `/speckit-analyze` | Consistency report across spec, plan, and tasks (read-only) |
| Implement | `/speckit-implement` | Code, following `tasks.md` |

Also available: `/speckit-checklist`, `/speckit-converge`, and `/speckit-taskstoissues`
(`.claude/skills/`).

- Each feature lives in `specs/NNN-feature-name/`. The active feature is recorded in
  `.specify/feature.json` (currently `specs/001-video-clipping`) and in the `SPECKIT` block of
  `CLAUDE.md`.
- Every plan must include a Constitution Check gate, evaluated before research and again after
  design.
- Research outlines in `research/` are not confirmed until marked so. Items tagged `[K]`, `[verify]`,
  or `[unverified]` are not yet confirmed (`CLAUDE.md`).

Planned features: 002 (mobile upscaling with smart reframing) and 003 (GIF export for Reddit).

## Add a New Job Kind

The executor is designed to take further job kinds (features 002 and 003). The plan records
mandatory refactors before the first new kind is added.

### Current extension point

- `crates/rmedia-core/src/plan/mod.rs` defines `pub enum JobSpec { Clip(ClipSpec) }`.
  `PlannedJob.spec` holds it.
- `crates/rmedia-core/src/exec/mod.rs` → `run_job()` dispatches on it:
  `JobSpec::Clip(spec) => clip::execute(spec, job.requested, job.predicted, &mut ctx).await`.
- The scheduler does not look inside a job kind. `JobCtx` provides the tools, the temp output path,
  the final output path, cancellation, the job ID, and progress events. The scheduler handles
  concurrency, journaling of `job.temp`, commit, and cleanup.

### Required refactors first (plan.md, senior review A1 to A5)

| ID | Change |
|---|---|
| A1 | Move clip-specific fields (`requested` and `predicted` spans, `mode`, `media_len`) from `PlannedJob` into `ClipSpec`. `JobOutcome::Succeeded` carries a `JobMeasure` enum instead of a clip `Span`. |
| A2 | Support multi-stage jobs: a job-kind `execute(&mut JobCtx)` that can run several processes. `JobCtx` gains `scratch_dir()` and journals every intermediate file, not only `job.temp`. |
| A3 | Weighted multi-stage progress, and per-resource semaphores (GPU vs CPU) in the scheduler. |
| A4 | `Caps` grows filter, hwaccel, GPU, and model availability. Issue codes become a typed enum. |
| A5 | Extract output resolution, claiming, and collision handling from `plan_clip` for reuse. |

### Procedure

1. Write the spec, plan, and tasks for the feature with Spec Kit (`specs/NNN-name/`). Pass the
   Constitution Check.
2. Complete the refactors A1 to A5 that the new kind needs, with tests passing before and after.
3. Write failing tests first: argv snapshot tests for the new FFmpeg pipeline, and integration tests
   against `rmedia-testkit` fixtures that verify outputs with `ffprobe`. Add new fixture kinds to
   `rmedia-testkit` if needed (lavfi only).
4. Add the variant to `JobSpec` (for example `JobSpec::Gif(GifSpec)` or
   `JobSpec::Upscale(UpscaleSpec)`) and its planning in `plan/`.
5. Add the job's `execute` module under `exec/` and a match arm in `run_job()`. Spawn every process
   through `exec::process` with argument vectors, and write only to `ctx` temp or scratch paths so
   that the journal and commit apply.
6. Report new capabilities in `doctor.rs` and fall back gracefully when they are missing.
7. Expose the feature in the CLI (new subcommand or flags in `crates/rmedia-cli`), including
   `--json`, `--dry-run`, and exit codes. Expose it in the desktop app through `ops.rs` and
   `commands.rs`.
8. Run `cargo xtask gen-bindings` if core types changed, and `cargo xtask notices` if dependencies
   changed.
9. Update the contracts in `specs/NNN-name/contracts/` and the user and administrator
   documentation.

Feature-specific notes from `plan.md` (Forward Compatibility):

- **002 mobile-upscale:** adds `JobSpec::Upscale`, a `Resource::Gpu` semaphore, Vulkan and upscaler
  detection in `doctor`, and device presets as data files (the constitution requires presets as
  data, not code).
- **003 gif-export:** adds `JobSpec::Gif` (two-pass palette pipeline, size-budget search), reusing
  `Segment`, the executor, events, and commit (`research/05-gif-export.md`).

> **Unverified:** None of the A1 to A5 refactors have been implemented yet. `JobSpec` has only the
> `Clip` variant. The procedure above combines the plan with the current code; adjust it once the
> refactors land.
