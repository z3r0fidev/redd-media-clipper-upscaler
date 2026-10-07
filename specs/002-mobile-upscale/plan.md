# Implementation Plan: Mobile Upscaling & Smart Reframe

**Branch**: `002-mobile-upscale` | **Date**: 2026-10-07 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/002-mobile-upscale/spec.md`

## Summary

Add an upscale job kind to `rmedia`. It turns a video (or a clip of one) into a phone-ready file:
the right size for a device preset, crisp, correctly colour-tagged, and filling a portrait screen.

**How the core does it:**
- Device presets are JSON data.
- Each job is one `-filter_complex` pass of the bundled FFmpeg:
  1. pre-process: deinterlace, square pixels, tone-map, denoise;
  2. reframe: tracked crop, center crop or blurred background;
  3. scale with the chosen quality tier, then encode;
  4. tag.
- **Smart reframe:**
  - Analysis runs on the CPU before rendering: scene cuts, YuNet face detection under `tract`, and a
    smoothed horizontal crop path.
  - The path and the fallback ranges reach FFmpeg as a `sendcmd` command file.
- **Tiers:**
  - The CPU tier is always available.
  - A Vulkan/libplacebo GPU tier is chosen automatically from a cached capability record, with one
    fallback to CPU.
  - The AI tier (last story) is a separate `rmedia-sr` worker built on ncnn + Vulkan, placed between
    two FFmpeg processes.
- **Reuse of 001:** inputs, layering, naming, collision handling, commit, journal, events, dry run
  and exit codes are unchanged. Clip + upscale is a single pass.

## Technical Context

**Language/Version**: Rust 1.94 (edition 2024); TypeScript 6 + Svelte 5 for the desktop view.

**Primary Dependencies**: everything from 001, plus:
- `tract-onnx` 0.23 (pure-Rust ONNX inference for YuNet; research/09 §4);
- `fs4` 1.x (free disk space and the capability-file lock).

US6 adds:
- the `rmedia-sr` crate, which links prebuilt **ncnn 20260526** (static, Vulkan) through a small FFI
  over the C API;
- Real-ESRGAN animevideov3 models.

External: the bundled FFmpeg n9.0.2. It provides zscale, cas, hqdn3d, bwdif, libplacebo,
sendcmd, scdet, gblur, tonemap, libx265, libx264 and libvmaf (research/02 "Measured").

**Storage**: files only.
- Built-in presets are compiled in; user presets live in `<config>/presets/*.json`.
- The capability record is `<state>/capability.json`.
- Reframe analysis is cached in `<cache>/reframe/<key>.json`.
- Models are bundled under `models/`; `xtask fetch-models` places them there for development.

**Testing**: `cargo test`:
- lavfi fixtures from `rmedia-testkit`, including new synthetic ones (degraded portrait, moving
  marker + cut, two far-apart markers, rotated, anamorphic, interlaced, HDR-tagged);
- ffprobe checks on outputs, and `testkit::vmaf()` for SC-002;
- insta snapshots of filter graphs and argv;
- golden `sendcmd` text.

GPU tests and AI tests are `#[ignore]` unless a usable Vulkan device or the models are present.

**Target Platform**: Linux x86_64 and Windows 10/11 x86_64 (Principle IV). The GPU tier uses Vulkan
1.3 through the bundled FFmpeg, on any vendor.

**Project Type**: Rust workspace (library + CLI + Tauri desktop app) plus one new worker binary
(US6).

**Performance Goals**:

| Goal | Target |
|---|---|
| CPU tier, 60 s 480p30 → phone-fill | ≤ 2× clip duration (SC-003) |
| GPU tier | no slower than CPU (SC-003) |
| Reframe analysis on 60 s of 1080p | ≤ 50% added time (SC-005); measured 68 ms per analysis frame at 3 fps, ≈ 12 s single-threaded, parallel across cores |
| Cancel | ≤ 2 s (SC-006) |
| AI tier, 60 s 480p on a mid-range discrete GPU | ≤ 10 min (SC-009) |

**Constraints**:
- Sources are never modified, and FFmpeg is always called with an argv (no shell).
- Works offline, with no network at run time.
- SDR stays SDR, and colour tags are written explicitly.
- Video is never enlarged and then shrunk (FR-005).
- Only permissively licensed detectors and models are used (FR-010, FR-015).
- Output frame rate is at most 60 fps.

**Scale/Scope**: a single user with local files, batches of tens to hundreds of videos, and sources
from 240p to 4K.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Pre-design | Post-design |
|---|---|---|---|
| I. One Core, Thin Surfaces | Presets, geometry, reframe analysis, tiers, capability and filter graphs live in `rmedia-core`; CLI/Tauri only adapt | PASS | PASS. `rmedia upscale` and the Tauri commands `analyze_reframe`/`upscale_preview` call core functions ([contracts/cli.md](./contracts/cli.md), [contracts/tauri-commands.md](./contracts/tauri-commands.md)). `rmedia-sr` is a frame filter with no media decisions; the core drives it |
| II. CLI-First, Scriptable | `--json`, `--dry-run` (with crop path), stable exit codes; deterministic output | PASS | PASS. [contracts/json-output.md](./contracts/json-output.md). Determinism holds per machine and tier (research R11) |
| III. Test-First with Real Media | lavfi fixtures, ffprobe/VMAF verification, no binary fixtures, models not committed | PASS | PASS. Synthetic marker fixtures plus `MarkerDetector` cover smart reframe without face images (R7) |
| IV. Linux = Windows | No shell; kill-tree for the 3-process AI pipeline; Windows paths in filtergraphs avoided | PASS | PASS. The command file is passed by a relative name with cwd set to the job scratch dir (R5); the pipeline shares one Job Object or process group (R13) |
| V. Inputs Are Sacred | `.part` → verify → no-clobber commit; analysis and scratch files journaled and removed | PASS | PASS. The scratch dir is journaled (refactor A2) |
| VI. Graceful Degradation | Tiers detected and reported; CPU always works; missing YuNet → blur-pad with a warning; forced unavailable tier → clear error | PASS | PASS. Capability record, `doctor` tiers, one automatic fallback (R9) |
| VII. Simplicity & Evidence | Choices cite research/02, 06–09; new deps and processes justified | PASS | PASS. See Complexity Tracking |

No violations.

## Project Structure

### Documentation (this feature)

```text
specs/002-mobile-upscale/
├── plan.md  research.md  data-model.md  quickstart.md
├── contracts/
│   ├── cli.md                    # rmedia upscale, clip --upscale, doctor tiers, exit codes
│   ├── preset.schema.json        # device preset files
│   ├── manifest-upscale.md       # "upscale" in JSON manifests, upscale_* CSV columns
│   ├── json-output.md            # dry-run upscale block, crop path, outcomes, doctor tiers
│   ├── capability-record.schema.json
│   ├── tauri-commands.md         # additions for the upscale view
│   └── rmedia-sr-protocol.md     # worker argv, stdin/stdout frames, stderr progress (US6)
├── checklists/requirements.md
└── tasks.md                      # /speckit-tasks output
```

### Source Code (repository root)

```text
crates/rmedia-core/src/
├── preset/{mod.rs, builtin/*.json}         # registry, validation, user presets
├── upscale/
│   ├── mod.rs        # UpscaleOptions, PartialUpscale, UpscalePlan, plan_upscale()
│   ├── geometry.rs   # displayed size, target fit, crop window, no-enlarge
│   ├── tier.rs       # Tier, TierChoice, select()
│   ├── capability.rs # GPU listing, fingerprint, self-test, record, demotion
│   ├── graph.rs      # filter_complex builder (pure)
│   ├── encode.rs     # preset → encoder/colour/tag args
│   ├── estimate.rs   # size/time estimate, free space, platform limits
│   └── reframe/{mod,decode,cuts,detect,select,path,sendcmd,cache}.rs
├── plan/{mod,ffargs,naming,streams}.rs     # A5 extraction; Upscale jobs; {preset} var
├── exec/{mod,upscale,pipeline}.rs          # A1–A3; stages; GPU semaphore; AI pipe (US6)
├── probe.rs                                # + rotation, SAR, field order, colour tags
└── request.rs  doctor.rs  event.rs  settings.rs
crates/rmedia-cli/src/commands/upscale.rs   # + clip --upscale, doctor --recheck
crates/rmedia-testkit/src/lib.rs            # new fixtures, vmaf(), marker helpers
crates/rmedia-sr/                           # US6 worker binary (ncnn FFI)
apps/desktop/src/lib/components/UpscaleView.svelte (+ Player overlay, Queue fields)
apps/desktop/src-tauri/src/{commands,ops}.rs
xtask/src/main.rs                           # fetch-models (+ ncnn for US6)
licenses/models/                            # YuNet MIT, Real-ESRGAN BSD-3
```

**Structure Decision**:
- All 002 logic goes into the existing core, in two new modules (`preset`, `upscale`), and plugs
  into the 001 planner and executor through `JobSpec::Upscale`.
- The only new crate is `rmedia-sr` (US6). It keeps native GPU code and driver crashes out of the
  core process (R13).

## Phasing (maps to user stories)

| Phase | Stories | Exit criteria |
|---|---|---|
| 0 Foundation | — | Refactors A1–A5 with all 001 tests green. Probe adds rotation, SAR, field order and colour. Preset registry. New fixtures and `vmaf()`. Engine checks for sendcmd `enable` and VFR as tests |
| 1 MVP | US1 (P1) | `rmedia upscale` CPU tier. Center and blur over the whole video. Encode and colour. Estimate and disk check. SC-001 (automated part), SC-002 (CPU), SC-006; SC-003 bench |
| 2 | US2 (P2) | Smart reframe analysis and cache, mixed graph, crop path in dry run. SC-004; SC-005 bench |
| 3 | US3 (P3) | Capability record, GPU tier, auto/forced, fallback and demotion, doctor tiers. SC-008 |
| 4 | US4 (P4) | Nested options in manifests/CSV, `clip --upscale` in one pass, folders and batch |
| 5 | US5 (P5) | Desktop upscale view, crop overlay, preview, queue/export parity. SC-007 |
| 6 | US6 (P6) | `rmedia-sr`, pipeline executor, self-test, anti-flicker. SC-002 (AI), SC-009 on a GPU runner or owner hardware |
| 7 Polish | — | `docs/` pages, notices and model licences, packaging of models and the worker, release notes |

## Complexity Tracking

| Component | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| `tract-onnx` dependency | Face detection for smart reframe (FR-013/015), pure Rust, MIT/Apache | ONNX Runtime or OpenCV add native libraries per OS; the 2023 YuNet model cannot be resized under tract, but 2026may can (research/09 §4) |
| `fs4` dependency | Free-space check (FR-021) and a locked capability file | Platform-specific statvfs/GetDiskFreeSpaceEx code duplicates a maintained crate |
| `rmedia-sr` crate + prebuilt ncnn (US6) | AI tier on any Vulkan GPU (FR-010) | ORT/DirectML is Windows-only; `realesrgan-ncnn-vulkan` is frozen and image-only (research/09 §1); in-process FFI would let a driver crash kill the app |
| Multi-process pipe executor (US6) | ffmpeg → worker → ffmpeg over rawvideo | Image-sequence round trips multiply disk I/O and break cancellation guarantees |
| `filter_complex` + `sendcmd` file | Tracked crop and per-range blur-pad fallback in one encode | Per-shot segments + concat need extra encodes and risk audio drift at joins |
| Capability record cache | FR-009, SC-008: no repeated self-test; fallback memory | Re-benchmarking on every run costs seconds and cannot remember failures |
| Reframe analysis cache | Dry-run/desktop preview and the render share one analysis (FR-016, SC-005) | Running the analysis twice doubles its cost |
| User preset folder | FR-001: add presets without code changes | Built-in-only presets need a release for each new device |
| Downloaded (not committed) model files | Constitution III (no binary fixtures) and licence tracking | Committing ONNX/ncnn weights bloats the repo and mixes licences into the source tree |
