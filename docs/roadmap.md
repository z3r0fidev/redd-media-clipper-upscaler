# rmedia Roadmap

This page is the single list of what is done, what is next and what is planned. Other lists
(`README.md`, `CLAUDE.md`, `specs/002-mobile-upscale/progress.md`) must agree with it. Task IDs
refer to `specs/002-mobile-upscale/tasks.md`, which is the source of truth for 002 progress.

Last updated: 2026-10-27.

## Status at a Glance

| Feature | State | Available now |
|---|---|---|
| 001 Video clipping | Complete (73/73 tasks); version 0.1.0, not yet tagged | `clip`, `batch`, `scan`, `doctor`, desktop app |
| 002 Mobile upscaling and smart reframe | Complete (112/112 tasks); version 0.2.0 release candidate, not yet tagged | `rmedia upscale` (CPU, GPU and AI tiers, smart reframe), `clip --upscale`, manifest upscaling in `batch`, `doctor` tiers and `--recheck`, desktop "Upscale for phone" view |
| 003 GIF export for Reddit | Complete (80/80 tasks, 2026-10-26); 0.3.0 in development, waiting on owner checks | On `main`, unreleased: `rmedia gif` (targets, size budget, Reddit MP4, gifsicle/gifski), `clip --gif`, `batch --gif`, manifest `gif` blocks, desktop "GIF for Reddit" view ([0.3.0 notes](releases/0.3.0-dev.md)) |

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
that bundles gifski fails until the application licence is decided (a root `LICENSE` and an
allowed `app_licence` in the root `Cargo.toml`). Owner check
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
- US2 (T045–T053) done: the budget search, the gifsicle post-pass at `-O2`, `BUDGET_UNREACHABLE`
  and `--allow-trim`. SC-002 20/20 under the aim, SC-003 23.8 s (bar 30 s), SC-004 9/10 within
  25%. SC-009 **not met**: gifsicle saves 6.4% (lossy 30) and 12.3% (lossy 200) on the synthetic
  set against a 15% aim (owner decision below).
- US3 (T054–T056) done: the Reddit MP4 (SC-006 6.1× and 18.2× smaller).
- US4 (T057–T058) done: 9:16, 4:5 and 1:1 with 002's reframe; smart framing.
- US5 (T059–T064) done: JSON and CSV manifests, `clip --gif`, `batch --gif`, `clip --gif --upscale`
  as one job.
- US6 (T065–T072) done: desktop GIF commands, the GIF view, GIF parity with the CLI; SC-007 two
  interactions, the estimate matched the output.
- Polish (T073–T080): docs, notices and pin checks, the Linux quickstart and the final reviews
  are done; T078 `release.yml` run 40345344064 (gifski off) packaged Linux and Windows, and T079
  Windows CI passed 482/482 (80/80, [progress](../specs/003-gif-export/progress.md)). Stopped
  for the owner checks below.

### Update (2026-10-27)

- Windows owner-check runbook ([runbook](operations/windows-owner-checks.md)) and the SC-009
  script `operations/windows-qa/Measure-Sc009.ps1` (not yet run) are on `main`.
- `release.yml` run 40452999345 (main 789d7e9, gifski off) succeeded; the release gate said ok
  before and after staging and with `--scan` on the built bundles. Windows artifact
  411,470,509 B, Linux 636,305,532 B. Use this build for the Windows checks.
- `clip`, `upscale`, `gif` and `batch` now catch Ctrl-C from the moment they start (3ea7571;
  exit 130, nothing written before planning ends). Runbook case (d) needs a release build from
  fae5d47 or later; build 40452999345 predates it. CI green on 3ea7571 (run 40467352168).

### Open owner decisions

- SC-009 (decided 2026-10-26): the automated bar is 5% on the synthetic set; owner check: at
  least 15% smaller with gifsicle on real footage ([runbook](operations/windows-owner-checks.md#5-sc-009-gifsicle-on-real-footage)).
- Application licence (decided 2026-10-27): releases ship **without gifski** (`release.yml`'s
  `gifski` input now defaults to off); the built-in engine covers every target and `max-quality`
  falls back with `ENGINE_FALLBACK`. If gifski is wanted later, the fallback is licensing the app
  AGPL-3.0 or GPL-3.0 (allowed by `cargo xtask check-release-gate`); the commercial
  gifski + imagequant route was not chosen.
- Visual C++ runtime (decided 2026-10-27): link it statically. ncnn is built from source with
  `/MT` and no OpenMP (`.github/workflows/ncnn-static.yml`) and the Windows binaries use
  `crt-static`; if that fails or the worker is noticeably slower, ship `vcruntime140`,
  `msvcp140` and `vcomp140` next to `rmedia-sr.exe` instead.
- Code signing (decided 2026-10-27): Azure Artifact Signing, signed in the US as an
  organisation. Owner: create the Azure account and pass identity validation; then the release
  workflow gets a signing step.
- Tag `v0.1.0` and `v0.2.0`; Dependabot notices automation.
- Owner checks: two-phone gallery test, a Reddit upload, +3 VMAF on real footage, Reddit's GIF
  limits, gifsicle's 15% saving on real footage (SC-009). The Windows installers and desktop app are untested on real hardware.

## Release and Project Housekeeping

- [ ] Tag `v0.1.0` and `v0.2.0` and run the release workflow; publishing and code signing are
  manual.
- [x] Decide the application licence for releases: ship without gifski (2026-10-27); AGPL-3.0 or
  GPL-3.0 if gifski is wanted later. The manifests' `MIT OR Apache-2.0` is still a placeholder.
- [x] Mirror the pinned third-party archives and publish the FFmpeg corresponding source
  (`mirror-2026-10-22`, `ffmpeg-source-n9.0.2-31-gc89d6ee1ca`; see
  [Build and Release](operations/build-and-release.md)).
- [x] Add a git remote: public `origin` on GitHub, work on `main`, CI green.
- [x] Remove the Windows AI worker's need for the Visual C++ runtime (`vcruntime140`,
  `msvcp140`, `vcomp140`): ncnn built with `/MT` and no OpenMP (`ncnn-static.yml`, release
  `ncnn-static-2026-10-28`), every Windows binary built with `crt-static`, checked in CI (2026-10-28).
  Owner check left: AI speed on Windows hardware (runbook §3.4); if it is noticeably slower,
  ship the three DLLs next to `rmedia-sr.exe` instead.
- [ ] Windows code signing with Azure Artifact Signing (US organisation, decided 2026-10-27):
  account and identity validation by the owner, then a signing step in `release.yml`.
- [ ] Manual desktop QA on Windows hardware (Windows paths and Job Object handling are unverified;
  runbook: [Windows owner checks](operations/windows-owner-checks.md)).

## Related

[Release notes 0.2.0](releases/0.2.0.md) · [Architecture overview](architecture/overview.md) ·
[CLI reference](api/cli-reference.md)
