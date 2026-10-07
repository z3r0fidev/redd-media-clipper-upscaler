<!--
Sync Impact Report
- Version change: 1.0.0 → 1.0.1 (PATCH: wording matches the shipped `--on-exists` flag and
  engine lookup order incl. environment variables; no change in meaning)
- Previous: (template) → 1.0.0
- Modified principles: n/a (initial ratification)
- Added sections: Core Principles I–VII, Technical & Licensing Constraints, Development Workflow,
  Governance
- Removed sections: none
- Templates requiring updates: none (plan/spec/tasks templates read the constitution at runtime)
- Deferred TODOs: none
-->

# reddit-media Constitution

## Core Principles

### I. One Core, Thin Surfaces

All media logic (discovery, timestamp parsing, manifest parsing, clipping, upscaling, encoding,
job orchestration) MUST live in a single shared core library. The CLI and the native desktop GUI
are thin presentation layers that MUST NOT contain media logic of their own. Any capability
exposed in one surface MUST be reachable from the other (feature parity), except purely
presentational affordances (e.g. a preview scrubber).

Rationale: two surfaces with duplicated logic drift apart and double the test burden.

### II. CLI-First, Scriptable, Deterministic

Every feature MUST be usable from the CLI before it ships in the GUI. The CLI MUST:
- send human-readable output to stdout and diagnostics to stderr;
- offer `--json` machine-readable output for every command that reports results;
- return documented, stable non-zero exit codes for distinct failure classes;
- support `--dry-run` for any operation that writes files, printing the planned jobs;
- produce identical output files for identical inputs and options.

### III. Test-First with Real Media (NON-NEGOTIABLE)

Tests are written before implementation and MUST fail before the implementation makes them
pass. Media behaviour MUST be verified against real video files generated at test time with
FFmpeg (`lavfi` sources such as `testsrc2`/`sine`); outputs MUST be verified with `ffprobe`
(duration within tolerance, resolution, codec, stream presence), not merely by exit code.
Parsers (timestamps, clip lists, manifests) MUST have table-driven unit tests covering
malformed input. Binary test fixtures MUST NOT be committed.

### IV. Linux and Windows Are Equal Citizens

Every release target MUST build, pass tests, and package on both Linux (x86_64) and Windows
(x86_64) in CI. Code MUST NOT rely on a shell to launch processes, MUST handle paths with
spaces, Unicode, and Windows drive letters/UNC paths, and MUST clean up child processes on
cancellation on both platforms (process groups on Linux, Job Objects on Windows).

### V. Inputs Are Sacred

The application MUST NEVER modify or delete a source video. Writes go only to an explicit or
derived output location; existing outputs are never overwritten without an explicit
`--on-exists overwrite` choice (default: skip; `rename` auto-suffixes). External tools MUST be invoked with argument
vectors, never interpolated command strings. Partially written outputs from failed or cancelled
jobs MUST be removed.

### VI. Graceful Capability Degradation

GPU acceleration and AI upscaling are optional enhancements. The application MUST detect
available capabilities (encoders, Vulkan/GPU, upscaler binaries, models) at runtime, report them
(`doctor` command / GUI diagnostics), and fall back to a CPU-only classical pipeline that always
works. A missing optional capability MUST produce a clear message, never a crash.

### VII. Simplicity and Evidence-Based Decisions

Start with the simplest design that satisfies the spec (YAGNI). Technology and algorithm choices
MUST cite the relevant document in `research/`. Additional complexity (new process boundaries,
new dependencies, new formats) MUST be justified in the plan's Complexity Tracking table.

## Technical & Licensing Constraints

- FFmpeg/ffprobe are invoked as external binaries; the app MUST locate them via (1) explicit
  config (a flag, the settings file or the `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` environment
  variables), (2) bundled sidecar, (3) `PATH`, in that order, and report the version found.
- Distribution of GPL-licensed components (e.g. FFmpeg builds with libx264/libx265) MUST be
  documented with license notices and source offers; AI model weights MUST have their licenses
  recorded in-repo before bundling.
- Output presets for mobile devices MUST be data (versioned preset definitions), not code
  branches, so new devices can be added without code changes.
- Long-running work MUST report progress and MUST be cancellable from both surfaces.

## Development Workflow

- Spec-Driven Development via Spec Kit: `constitution → specify → clarify → plan → tasks →
  analyze → implement`. Each feature lives under `specs/NNN-feature-name/`.
- Research outlines live in `research/` and are referenced from `plan.md` / `research.md`.
- Every completed user story MUST pass: unit + integration tests on both OSes, a senior code
  review (code-reviewer / senior-code-reviewer agent), and a security review for any code that
  handles file paths or spawns processes.
- Commits follow Conventional Commits.

## Governance

This constitution supersedes conflicting practices. Amendments require a written rationale, a
version bump (MAJOR: principle removed/redefined; MINOR: principle/section added or materially
expanded; PATCH: clarifications), and a Sync Impact Report at the top of this file. Every plan
MUST include a Constitution Check gate evaluated before research and again after design; any
violation MUST be justified in Complexity Tracking or the design changed.

**Version**: 1.0.1 | **Ratified**: 2026-10-06 | **Last Amended**: 2026-10-06
