# rmedia Roadmap

This page is the single list of what is done, what is next and what is planned. Other lists
(`README.md`, `CLAUDE.md`, `specs/002-mobile-upscale/progress.md`) must agree with it. Task IDs
refer to `specs/002-mobile-upscale/tasks.md`, which is the source of truth for 002 progress.

Last updated: 2026-10-25.

## Status at a Glance

| Feature | State | Available now |
|---|---|---|
| 001 Video clipping | Complete (73/73 tasks); version 0.1.0, not yet tagged | `clip`, `batch`, `scan`, `doctor`, desktop app |
| 002 Mobile upscaling and smart reframe | Complete (112/112 tasks); version 0.2.0 release candidate, not yet tagged | `rmedia upscale` (CPU, GPU and AI tiers, smart reframe), `clip --upscale`, manifest upscaling in `batch`, `doctor` tiers and `--recheck`, desktop "Upscale for phone" view |
| 003 GIF export for Reddit | ▶ In progress (2026-10-25): tasks written, Phase 1 and most of Phase 2 done, US1 under way | Nothing yet for users (plan: `specs/003-gif-export/plan.md`, tasks: `specs/003-gif-export/tasks.md`) |

## Feature 002: Mobile Upscaling and Smart Reframe

### Done

| Phase | Tasks | What it delivered |
|---|---|---|
| 1 Setup | T001–T006 | Baseline, dependencies, module scaffolding, `cargo xtask fetch-models`, model notices, CI step |
| 2 Foundational | T007–T021 | Refactors A1–A5 of the job model (job-agnostic outcomes with `measure`, multi-stage jobs with scratch folders, weighted progress, per-resource job slots, typed issue codes), test fixtures, VMAF measurement, richer probe, engine capability probes, device presets |
| 3 US1 (MVP) | T022–T041 | `rmedia upscale` on the CPU tier, seven device presets, `--list-presets`, upscale block in dry-run JSON, centre and blur reframe, HDR tone-map or keep, size and time estimates, platform limit warnings, free-space warnings, verify stage, cancel within 2 s |

| 4 US2 Smart reframe | T042–T061 | Follow faces (YuNet), shot detection, smooth crop path, `--crop-path`, `--motion`, analysis cache |
| 5 US3 GPU tier | T062–T073 | Vulkan GPU tier (libplacebo), machine capability record and self-tests, `auto` with a speed guard, one CPU fallback with demotion, `doctor` tiers/GPUs/models and `--recheck`, desktop **Check again** |
| 6 US4 Batch and `clip --upscale` | T074–T081 | `upscale` in JSON manifests (every level, whole videos without `clips`) and CSV (`upscale_*` columns), `batch --upscale` and upscale flags as fills, `clip --upscale` in one pass, default output folders always skipped by scans |
| 7 US5 Desktop upscale view | T082–T092 | "Upscale for phone" view: crop overlay and blurred ranges on the player, before/after still, preset/framing/quality pickers, "Upscale now" (one job) and "Add to queue", models folder in Settings |
| 8 US6 AI tier | T093–T105 | `rmedia-sr` AI worker (ncnn + Vulkan, Real-ESRGAN, pinned models), three-process pipeline, self-test gate (60 s of 480p in ≤ 10 min), too-slow warning, anti-flicker, installers bundle the worker and models |
| 9 Polish | T106–T112 | Docs audited against the code, 0.2.0 release notes and README, notices, quickstart on Linux and the desktop SC-007 check, CI green on GitHub (FFmpeg 6.x filter fix, Windows worker link and test fixes), archive mirror and FFmpeg source release, final reviews |

The checkpoint reviews (senior-code-reviewer, security-auditor) for every finished phase are done
and their findings are fixed. 299 workspace tests pass (10 ignored) on the bundled engine, and
the suite also passes against the minimum FFmpeg 6.1.

### Next and pending

| Order | Phase | Tasks | Planned outcome | Status |
|---|---|---|---|---|
| 1 | US2 Smart reframe | T042–T061 | Follow faces (YuNet via `cargo xtask fetch-models`), shot detection, smooth crop path, `--crop-path`, `--motion`, analysis cache. | Done (2026-10-16) |
| 2 | US3 GPU tier and capability record | T062–T073 | Vulkan GPU tier, self-test and capability record, `doctor` tiers, GPUs and models, `doctor --recheck`, automatic fallback to CPU | Done (2026-10-19) |
| 3 | US4 Batch and `clip --upscale` | T074–T081 | Upscale from manifests and `clip --upscale`; partial-clip tests carried over from the US1 review; `batch` runs the machine check when it has upscale jobs | Done (2026-10-19) |
| 4 | US5 Desktop upscale view | T082–T092 | Upscale view in the desktop app | Done (2026-10-19) |
| 5 | US6 AI worker | T093–T105 | `rmedia-sr` worker with ncnn for AI upscaling | Done (2026-10-20); quality and speed on a discrete GPU are an owner check |
| 6 | Polish, docs, packaging | T106–T112 | Final review pass, documentation | Done (2026-10-22) |

Each phase ends with a checkpoint: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, the quickstart section for the story, and the two reviews.

All of 002 is on `main`; no tagged release includes it yet (0.2.0 is a release candidate).

### Owner validation (cannot be automated)

- [ ] Two-phone gallery test (iPhone and Android): no bars, correct colours (SC-001).
- [ ] Reddit test upload of `reddit-916` and `reddit-feed-45` outputs.
- [ ] +3 VMAF over bicubic on real low-quality footage (SC-002 owner check).
- [x] Discrete-GPU run of the AI tier tests (T098, SC-002 AI, SC-009): Quadro M4000, all three
  pass (2026-10-22).

### Decision recorded

2026-10-14, owner: the automated SC-002 bar on the CPU tier is +1.5 VMAF over bicubic on
synthetic fixtures (measured +2.78 at 480p and +2.89 at 360p). The +3 target is checked by the
owner on real footage.

### Known limits (recorded in `specs/002-mobile-upscale/checklists/validation.md`)

- HDR kept with `--hdr keep` carries no mastering metadata; a warning says so.
- A pre-existing symlinked `upscaled/` folder is followed.
- Rare colour tags (smpte240m, film) fall back to BT.601 or BT.709 defaults.
- Very long source names can exceed 255 bytes in temporary names.
- On the reference iGPU the GPU tier is slower than the CPU, so `auto` uses the CPU there.
- Windows: a GPU driver registered only under the display-adapter key is not in the fingerprint
  (`rmedia doctor --recheck` after a driver update).
- A verify failure on the GPU tier is not retried on the CPU.
- `cargo deny` reports the advisory for `proc-macro-error` through Tauri's GTK dependencies.
  This predates feature 002.

## Feature 003: GIF Export for Reddit

Specified on 2026-10-22: [spec](../specs/003-gif-export/spec.md); planned on 2026-10-23:
[plan](../specs/003-gif-export/plan.md); tasks: [tasks](../specs/003-gif-export/tasks.md) (T001–T080).
Research: `research/05-gif-export.md`. Six stories: a GIF that fits a Reddit post (P1), automatic
size-budget search (P2), a silent "Reddit MP4" alternative (P3), portrait and square shapes via
002's reframe (P4), batch and `clip` integration (P5), and a desktop GIF view (P6). It reuses the
job model that feature 002 introduced. Owner decision: gifsicle (GPL-2.0) and gifski (AGPL-3.0)
are optional engines, and (2026-10-23) both are bundled, pinned and built from source; a release
that bundles gifski fails until the application licence (a root `LICENSE`) exists. Owner check
before release: confirm Reddit's GIF size limits in its Help Center.

### Progress (2026-10-25)

- Done: Phase 1 (T001–T005 setup) and Phase 2 apart from three tasks: GIF types and options,
  targets, timing, probe, estimate, `gif::verify`, engine lookup and `doctor`, refactors B1–B4
  (reframe input, N-stage pipelines), the GIF engine supply chain (`xtask pin-gif-tools`,
  `check-release-gate`, `.github/workflows/gif-tools.yml`, notices) and its security review
  (`specs/003-gif-export/checklists/validation.md`). Tasks T001–T009, T011, T012, T014–T032.
- T009 done: both engines built by `gif-tools.yml` run 40211502841 and pinned in
  `xtask/gif-tools.lock.json` (release `gif-tools-2026-10-25.40211502841`). gifski's lock moves
  crossbeam-epoch 0.9.18 → 0.9.20 for RUSTSEC-2026-0204 (`lock_updates`).
- US1 (T033–T044) done: `rmedia gif` with the built-in engine and gifski (`max-quality`, with
  fallback), dry run with a sample size estimate, verify and cancel. SC-010: gifski 99.98 vs
  98.80 VMAF (Land6), 84.63 vs 83.50 (Busy10).
- T010 done: both engines are desktop sidecars and in the CLI archive; CI's bundled-engine jobs
  run the GIF tests with them. A manual release run can leave gifski out.
- T013 done: gifski, gifsicle `--lossy` and x264 are byte-reproducible on repeated runs.
- US1 checkpoint done: quickstart and both reviews, findings fixed or recorded in
  `specs/003-gif-export/checklists/validation.md`.
- Next: US2 automatic size budget (T045–).
- Then: US2 automatic size budget, US3 Reddit MP4, US4 portrait and square, US5 batch and `clip`
  integration, US6 desktop GIF view, and Polish (T045–T080).

### Open owner decisions

- Application licence (gates any release that bundles gifski: `cargo xtask check-release-gate`).
  gifski's quantiser imagequant is GPL-3.0-or-later (allowed, 2026-10-25), so a commercial route
  needs licences for both (`commercial-gifski-imagequant`).
- The unpinned release `gif-tools-2026-10-25` (gifski with crossbeam-epoch 0.9.18) is still
  public; delete it or keep it.
- Visual C++ runtime for the 002 Windows installers.
- Tag `v0.1.0` and `v0.2.0`; Dependabot notices automation.
- Owner checks: two-phone gallery test, a Reddit upload, +3 VMAF on real footage, Reddit's GIF
  limits. The Windows installers and desktop app are untested on real hardware.

## Release and Project Housekeeping

- [ ] Tag `v0.1.0` and `v0.2.0` and run the release workflow; publishing and code signing are
  manual.
- [ ] Decide the application licence (manifests declare `MIT OR Apache-2.0` as a placeholder).
- [x] Mirror the pinned third-party archives and publish the FFmpeg corresponding source
  (`mirror-2026-10-22`, `ffmpeg-source-n9.0.2-31-gc89d6ee1ca`; see
  [Build and Release](operations/build-and-release.md)).
- [x] Add a git remote: public `origin` on GitHub, work on `main`, CI green.
- [ ] Decide how the Windows installers provide the Visual C++ runtime the AI worker needs
  (`vcruntime140`, `msvcp140`, `vcomp140`).
- [ ] Manual desktop QA on Windows hardware (Windows paths and Job Object handling are unverified).

## Related

[Release notes 0.2.0](releases/0.2.0.md) · [Architecture overview](architecture/overview.md) ·
[CLI reference](api/cli-reference.md)
