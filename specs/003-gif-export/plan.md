# Implementation Plan: GIF Export for Reddit

**Branch**: `003-gif-export` (work happens on `main`) | **Date**: 2026-10-23 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/003-gif-export/spec.md`

## Summary

Add a GIF job kind to `rmedia`. It turns a video, a clip of one, or a 002 reframed/upscaled result
into a looping GIF, or a silent "Reddit MP4", that fits a size budget.

**How the core does it:**
- GIF targets (Reddit post, Reddit comment, max quality, flat/UI) are JSON data, in a registry
  that sits beside 002's device presets.
- The built-in engine is the bundled FFmpeg's **two-pass palette** pipeline:
  - The input chain is 002's: trim, deinterlace, square pixels, tone-map, and the reframe crop or
    blur-pad.
  - Then `fps`, a lanczos downscale (never enlarging), and `palettegen` from the final-size frames
    into the job's scratch folder.
  - Then `paletteuse` with rectangle diff mode.
- A **bounded budget search** encodes samples of three 1 s windows (separate seeks), models the
  size as a first frame plus deltas, and walks a fixed ladder: lossy optimiser → dithering →
  colours → frame rate → width, then floors and one interpolation. It uses at most 9 encodes
  (asserted ≤ 10) and is deterministic. The pre-encode estimate is this search's predicted
  result.
- **Optional engines run as separate processes, bundled and pinned (owner decision 2026-10-23):**
  - gifsicle is the post-pass: `-O3` plus a lossy level.
  - gifski is the best-quality engine, fed from FFmpeg over a y4m pipe.
  - Both are built from pinned sources by a manual workflow and mirrored like FFmpeg.
  - A release that bundles gifski is blocked until the application licence exists.
- **Reddit MP4** is libx264, silent, `+faststart`, with a CRF ladder for budgets.
- **Reuse:** inputs, layering, naming, collisions, commit, journal, events, dry run and exit codes
  from 001; scratch folders, staged progress, reframe analysis and colour handling from 002.

## Technical Context

**Language/Version**: Rust 1.94 (edition 2024); TypeScript 6 + Svelte 5 for the desktop view.

**Primary Dependencies**:
- No new Rust crates.
- Bundled FFmpeg n9.0.2 (palettegen, paletteuse, unsharp, the gif muxer, libx264,
  yuv4mpegpipe; verified in research R1).
- New bundled sidecars (research R8, R9):
  - **gifsicle 1.98** (GPL-2.0), from lcdf.org sources;
  - **gifski 1.34.0** (AGPL-3.0-or-later), built from the crates.io crate with its default features
    (`binary`, `gifsicle`): y4m/PNG input and no FFmpeg linkage.

**Storage**: files only.
- Built-in GIF targets are compiled in.
- User targets live in `<config>/gif-targets/*.json`.
- Palettes and samples go in the job's scratch folder and are removed with it.
- Smart-reframe analysis uses 002's cache.
- Engine pins are in `xtask/gif-tools.lock.json`.

**Testing**: `cargo test`, with:
- lavfi fixtures from `rmedia-testkit`, plus new ones: busy 720p, flat UI, static, alpha;
- GIF checks: packet delays with time base 1/100, NETSCAPE loop block, frame count, size;
- `testkit::vmaf()` for SC-005/009/010;
- insta snapshots of graphs and argv;
- a budget-search model test on a fake encoder.

Optional-engine tests use `require_tool!`. They are strict under `RMEDIA_REQUIRE_GIF_TOOLS=1` in
the bundled-engine CI jobs, and skip in the minimum-engine job.

**Target Platform**: Linux x86_64 and Windows 10/11 x86_64.

**Project Type**: Rust workspace (library, CLI, Tauri desktop app, xtask). No new crate.

**Performance Goals**:

| Goal | Target |
|---|---|
| 10 s 720p → Reddit post with budget search | ≤ 3× clip duration on the reference machine (SC-003) |
| Encodes per job | ≤ 10, samples included (FR-010) |
| Estimate before encoding | within 25% for 90% of test clips (SC-004); desktop shows it within 5 s |
| Cancel | ≤ 2 s, no partial files or stray processes (SC-008) |

**Constraints**:
- Sources are never modified, and every tool is called with an argv (no shell).
- Works offline.
- GIF frame delays are never under 2 cs, and the rate is capped at 50 fps.
- No enlarging for GIF (FR-004).
- Both optional engines stay separate processes, never linked (licences; `deny.toml` stays
  permissive-only).

**Scale/Scope**: single user, clips of a few seconds to about 60 s, batches of tens.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-design | Post-design |
|---|---|---|---|
| I. One Core, Thin Surfaces | Targets, timing, graphs, budget search, engine lookup and verification live in `rmedia-core` | PASS | PASS. `rmedia gif`, `clip --gif` and the Tauri `gif_estimate`/`open_output` commands call core functions ([contracts/cli.md](./contracts/cli.md), [contracts/tauri-commands.md](./contracts/tauri-commands.md)) |
| II. CLI-First, Scriptable | `--json`, `--dry-run` with estimate and search plan, stable exit codes, byte-identical repeat runs | PASS | PASS. [contracts/json-output.md](./contracts/json-output.md); 001/002 JSON stays compatible (B4). The built-in engine's determinism was measured (R1), and the search is a pure function of sample sizes (R5). MP4 output is deterministic per machine, as in 002. gifski is checked in Phase 0; if it is not deterministic, its outcomes say so and byte-identity tests cover the built-in engine only (R14) |
| III. Test-First with Real Media | lavfi fixtures, ffprobe/packet checks, VMAF; no binary fixtures | PASS | PASS. GIF verification reads packets and the loop block (R11) |
| IV. Linux = Windows | No shell; kill-tree across ffmpeg → gifski; both sidecars built for both OSes | PASS | PASS. The rewritten pipe (B3) kills a Linux process group or one kill-on-close Job Object per child on Windows, consumer first (R7); palette paths are relative names with cwd = scratch (R1) |
| V. Inputs Are Sacred | `.part` → verify → commit; samples and palettes in a journaled scratch folder | PASS | PASS |
| VI. Graceful Degradation | Missing gifsicle/gifski → built-in engine with a warning; missing gif muxer or libx264 disables only that output | PASS | PASS. `doctor` reports both outputs and both engines (FR-021, FR-024) |
| VII. Simplicity & Evidence | Choices cite `research/05` and R1–R12; new processes and formats justified | PASS | PASS. See Complexity Tracking |
| Licensing constraint | GPL/AGPL components get notices and source offers | PASS | PASS. Notices and a source mirror for both engines; a mechanical release gate for gifski's AGPL (R10) |

No unjustified violations.

## Project Structure

### Documentation (this feature)

```text
specs/003-gif-export/
├── plan.md  research.md  data-model.md  quickstart.md
├── contracts/
│   ├── cli.md                    # rmedia gif, clip/batch --gif, doctor lines, exit codes
│   ├── gif-target.schema.json    # GIF target files
│   ├── manifest-gif.md           # "gif" in JSON manifests, gif_* CSV columns
│   ├── json-output.md            # dry-run gif block, search record, outcome measure, doctor
│   ├── tauri-commands.md         # gif_estimate, open_output, media server types
│   └── gif-tools-lock.md         # pin file, build workflow, notices, release gate
├── checklists/requirements.md
└── tasks.md                      # /speckit-tasks output (not yet)
```

### Source Code (repository root)

```text
crates/rmedia-core/src/
├── gif/
│   ├── mod.rs        # GifOptions, PartialGif, GifPlan, check_gif(), plan_gif()
│   ├── target.rs     # GifTarget, GifTargetRegistry, builtin/*.json
│   ├── timing.rs     # fps → centisecond delays, exactness, clamping
│   ├── graph.rs      # GIF / MP4 tails on top of upscale::graph::frame_chain
│   ├── budget.rs     # Ladder, Search, SampleEncoder trait (pure)
│   ├── estimate.rs   # pre-run size estimate from the sample model
│   ├── engines.rs    # gifsicle / gifski lookup, versions, args
│   └── verify.rs     # packet delays, loop block, frame count
├── upscale/graph.rs                  # + pub frame_chain(), Flavor::Gif (refactor B1)
├── upscale/reframe/mod.rs            # analyze_cached(&ReframeInput) (refactor B2)
├── exec/{gif.rs, pipeline.rs}        # GIF stages; N-stage pipe (refactor B3)
├── plan/mod.rs  request.rs  event.rs  error.rs  probe.rs  tools.rs  doctor.rs  settings.rs
└── input/{manifest_json,manifest_csv}.rs
crates/rmedia-cli/src/commands/gif.rs     # + GifFlags, clip/batch --gif, doctor lines
crates/rmedia-testkit/src/lib.rs          # fixtures, gif_delays(), gif_loop(), require_tool!
apps/desktop/src/lib/components/GifView.svelte, stores/gif.ts, stores/gif.svelte.ts
apps/desktop/src-tauri/src/{commands,ops,media_server}.rs
xtask/src/main.rs, xtask/gif-tools.lock.json  # pin-gif-tools, fetch, check-pins, notices
.github/workflows/gif-tools.yml               # builds the pinned engines (manual)
licenses/GIFSICLE-NOTICE.md, GIFSKI-NOTICE.md
```

**Structure Decision**:
- All 003 logic goes in one new core module, `gif`. It plugs into the planner and executor
  through `JobSpec::Gif`, the slot `plan/mod.rs:87` already reserves.
- 002 code is reused through three small refactors (B1–B4, [data-model.md](./data-model.md)),
  not copied.
- No new crate.

## Phasing (maps to user stories)

| Phase | Stories | Exit criteria |
|---|---|---|
| 0 Foundation | — | Refactors B1–B4 with all 001/002 tests and snapshots green. Determinism checks for gifski, gifsicle `--lossy` and x264 (R14); gifski 4:4:4 y4m check (R7). Probe adds `r_frame_rate`, `nb_frames`, `time_base` and alpha. Target registry, timing, verify helpers, engine lookup. `gif-tools.yml` run, pins mirrored, `fetch-tools` stages both engines |
| 1 MVP | US1 (P1) | `rmedia gif` with the built-in engine: sample estimator (`estimate.rs`, R4) and dry run with the estimate, cancel, verify, loops and alpha (R15). gifski engine with fallback and strict mode (FR-023). SC-001, SC-005, SC-008, SC-010 |
| 2 | US2 (P2) | Budget search, gifsicle post-pass and lossy (FR-022), `BUDGET_UNREACHABLE` and consented trim. SC-002, SC-003 bench, SC-004, SC-009 |
| 3 | US3 (P3) | Reddit MP4 with its CRF ladder; the "MP4 would be smaller" note. SC-006 |
| 4 | US4 (P4) | Shapes and framing through 002's reframe; downscale-only from upscaled input |
| 5 | US5 (P5) | Manifest `gif` block and `gif_*` CSV columns; `clip --gif`, `batch --gif`; clip → reframe/upscale → GIF in one job |
| 6 | US6 (P6) | Desktop GIF view, live estimate, playback, queue and export parity. SC-007 |
| 7 Polish | — | `docs/` pages, notices, release notes, roadmap; owner checks (Reddit limits, licence) |

## Requirement traceability

| Requirements | Where designed |
|---|---|
| FR-001 loop, FR-002 palette, FR-003 timing, FR-004 no enlarge | R1, R3, [data-model GifOptions](./data-model.md), [cli.md](./contracts/cli.md) `--loop` |
| FR-005 Reddit MP4 | R6, `--format mp4` |
| FR-006/007 targets, FR-008 overrides | R2, [gif-target.schema.json](./contracts/gif-target.schema.json), `GifFlags` |
| FR-009–012 budget search | R4, R5, SearchRecord, [json-output.md](./contracts/json-output.md) |
| FR-013/014 shape, framing, downscale | B1, B2, `FramePlan`, `--shape`/`--framing`, chained jobs |
| FR-015/016 inputs, manifests, clip | [manifest-gif.md](./contracts/manifest-gif.md), `clip`/`batch --gif` |
| FR-017/018/019 001 conventions, verify, estimate | R4, R11, `JobSpec::Gif` stages; free-space check extended to GIF jobs |
| FR-020 desktop | [tauri-commands.md](./contracts/tauri-commands.md) |
| FR-021/024 diagnostics, engine lookup | `engines.rs`, `doctor` lines, R7 |
| FR-022/023 gifsicle, gifski | R7, R8, R9 |
| FR-025 notices, pins, licence gate | R10, [gif-tools-lock.md](./contracts/gif-tools-lock.md) |
| SC-001…010 | Phases 1–6 exit criteria; checks in [quickstart.md](./quickstart.md) |

## Complexity Tracking

| Component | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| gifsicle sidecar (GPL-2.0) | FR-022: lossy LZW post-pass, the first budget step; SC-009 | FFmpeg has no lossy GIF mode; linking is impossible (C, GPL-2.0-only), so it runs as a separate process |
| gifski sidecar (AGPL-3.0) | FR-023: best-quality engine; SC-010 | Linking the crate would make the app AGPL; a separate process keeps the engine and the app apart (R10) |
| `gif-tools.yml` build workflow + `gif-tools.lock.json` | Owner decision to bundle; neither project ships Linux/Windows binaries (R8, R9) | Third-party Windows builds are unpinnable and unaudited; building on users' machines is not offline |
| Release licence gate | FR-025: gifski can't ship until the app licence is decided | A manual checklist item can be missed on a tag push |
| N-stage pipe executor (generalises 002's AI pipe) | ffmpeg → gifski over y4m with kill-tree cancellation (SC-008) | PNG frame dumps cost hundreds of MB of scratch disk per clip and break the 2 s cancel bound on slow disks |
| Second data registry (GIF targets) | FR-006: targets are data; users add their own | The device `Preset` struct is codec- and audio-specific; a union type would weaken both schemas |
| `BUDGET_UNREACHABLE` issue code, `ErrorInfo.detail`, `notices` (B4) | FR-011: a distinct, scriptable failure with the duration that would fit | Reusing `INVALID_INPUT` hides why the job failed and what would fix it |
| Sample-based estimator | FR-010, SC-004: GIF size has no closed-form predictor (research/05 §4) | Bits-per-pixel formulas miss by 2–5× across content types |
