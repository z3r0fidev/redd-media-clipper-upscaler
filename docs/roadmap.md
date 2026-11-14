# rmedia Roadmap

This page is the single list of what is done, what is next and what is planned. Other lists
(`README.md`, `CLAUDE.md`, `specs/002-mobile-upscale/progress.md`) must agree with it. Task IDs
refer to `specs/002-mobile-upscale/tasks.md`, which is the source of truth for 002 progress.

Last updated: 2026-11-14.

## Status at a Glance

| Feature | State | Available now |
|---|---|---|
| 001 Video clipping | Complete (73/73 tasks); version 0.1.0, tagged `v0.1.0` (`bd1eef8`) | `clip`, `batch`, `scan`, `doctor`, desktop app |
| 002 Mobile upscaling and smart reframe | Complete (112/112 tasks); version 0.2.0, tagged `v0.2.0` (`b6385a1`) | `rmedia upscale` (CPU, GPU and AI tiers, smart reframe), `clip --upscale`, manifest upscaling in `batch`, `doctor` tiers and `--recheck`, desktop "Upscale for phone" view |
| 003 GIF export for Reddit | Complete (80/80 tasks, 2026-10-26); version 0.3.0, tagged `v0.3.0` (`b96bfdd`, 2026-11-14) | `rmedia gif` (targets, size budget, Reddit MP4, gifsicle/gifski), `clip --gif`, `batch --gif`, manifest `gif` blocks, desktop "GIF for Reddit" view ([0.3.0 notes](releases/0.3.0.md)) |
| 004 Smart cut | Planned, next; not yet specified (`specs/004-smart-cut/` to come); version 0.4.0 when done | Hybrid frame-exact clip mode, see Pending item 22 |
| 005 Budgeted Reddit video | Planned after 004; no version yet | MP4 with audio under Reddit size and length caps, see Pending item 23 |

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

All of 002 is on `main` and released as tag `v0.2.0` (no GitHub release yet).

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
that bundles gifski needs a root `LICENSE` and an allowed `app_licence` in the root
`Cargo.toml`; both are in place since rmedia became AGPL-3.0-or-later (2026-11-13). Owner check
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
  25%. SC-009 restated 2026-11-02 (owner decision below): reddit-post's lossy is now 10; gifsicle
  saves 5.2% on the synthetic set and 8.1% on real footage, losing at most 0.67 VMAF.
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

- SC-009 (decided 2026-11-02, replacing 2026-10-26): no lossy level reached 15% within 1 VMAF
  point on real live-action footage or the synthetic set, so the bar is quality: gifsicle may cost
  at most 1 VMAF point on any clip, and its saving is reported. reddit-post's default lossy went
  from 30 to 10 (real footage: 8.1% smaller, worst loss 0.67). The synthetic bench keeps a 5%
  floor (measured 5.2%). Data: `specs/003-gif-export/checklists/validation.md`.
- Application licence (decided 2026-10-27, replaced 2026-11-13 by AGPL-3.0-or-later with gifski
  bundled): releases ship **without gifski** (`release.yml`'s
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
  limits, SC-009 on real footage with a mix of content (done for live action, 2026-11-02). The Windows installers and desktop app are untested on real hardware.

### Update (2026-10-28 to 2026-11-04)

- Fixed: the palette pass and the short estimate decoded to the end of the source (`4e9a41f`; an
  8 s GIF from 21 min into a 62 min source went from about 30 min per pass to 6 s); job warnings
  printed twice (`5a1984b`); gifsicle's file is kept only when smaller (`5fa548f`).
- SC-009 became a quality bar and reddit-post's lossy went 30 to 10 (`a6a7fdc`, breaking). Sweep
  on 9 real clips (saved / worst VMAF loss): lossy 0 2.0% / 0.00, 10 8.1% / 0.67, 20 10.4% / 1.42,
  30 11.9% / 2.06, 60 15.0% / 3.97, 80 16.6% / 5.19.
- Release run 41550220800 (on `5fa548f`, gifski off, gate ok, nothing published): Windows
  artifact 13194480079, Linux 13193349290. Files are still named 0.2.0. Windows runbooks point at it.
- Windows SSH checks on build 40740230580 (i7-1185G7, Iris Xe, Win 11) passed: Unicode and long
  paths, accurate mode, skip, source hash, GIF post/comment, Reddit MP4, max-quality fallback,
  9:16 smart, `clip --gif`, cancel cases a, a2, c, d, silent NSIS install, no VC++ DLLs bundled.
  AI tier: cpu 29 fps, gpu 12 fps, ai 0.2 fps (refused as too slow); auto picks CPU. SC-009 on
  that build failed (8.9% saved, worst loss 2.19) but does not count (palette bug).
- Tech debt closed 2026-11-04: an invalid `settings.json` is kept as
  `settings.invalid-<time>.json` instead of being overwritten (`c8978b2`); `pin-tools` updates
  `licenses/FFMPEG-NOTICE.md` (`2e2db88`); an injected-panic test covers the 001 review's H2
  (`761a0c9`); the desktop queue picks several audio tracks (`1f0a223`; desktop step 13 needs a
  build newer than run 41550220800).

### Update (2026-11-14)

- Windows checks on 0.3.0 (run 43534418980) all passed; `v0.3.0` tagged on `b96bfdd` and the
  release notes finalised (`docs/releases/0.3.0.md`).
- **Tag build, release run 43573283485: success on both platforms; nothing published.**

  | Artifact | Id | Size | sha256 |
  |---|---|---|---|
  | `rmedia-x86_64-pc-windows-msvc` | 14105967890 | 415,376,129 B | `50247ba85ef961d46e9b4a99e592c4ac1ba5e00caafd1decb9c64bd59dfc5165` |
  | `rmedia-x86_64-unknown-linux-gnu` | 14106166938 | 639,646,386 B | `b02253d5b63d6b4e979ba12f3f4f031818625e33d9de832be55fc2c01726e70a` |
- Also this session: rmedia is AGPL-3.0-or-later with gifski bundled; Dependabot PRs handled
  (TypeScript 7 and later ignored, #27 closed); `dependabot-notices.yml` regenerates
  `THIRD_PARTY_NOTICES.md` on Dependabot PRs; desktop portrait preview fixed (`d969f39`); flaky
  Windows cancel test fixed (`ea3601c`); troubleshooting entries for the duplicate graphics card
  and the libEGL DRI3 warning; the Windows runbooks rewritten for per-build output folders.
- Reviews run at the 0.3.0 milestone (senior-code-reviewer, security-auditor,
  product-strategy-advisor): no Critical or High findings and nothing that corrupts data. Their
  findings are items 5 to 7 below. The close-out procedure is a permanent runbook:
  [Closing a session](operations/session-close.md).

### Pending (single list; the Obsidian TODO and ROADMAP notes mirror it)

Work in this order. The next session starts at item 5 (M1).

**Owner actions**

1. Publish a GitHub release for `v0.3.0` from the packages of run 43573283485 (unsigned); decide
   whether to publish catch-up releases for 0.1.0 and 0.2.0.
2. Code signing: create the Azure Artifact Signing account and pass identity validation; then a
   signing step, `SHA256SUMS` and an attestation go into `release.yml`.
3. Reddit: confirm the current limits in its Help Center and do a real upload (post GIF, comment
   GIF, `reddit-916` and `reddit-feed-45` outputs).
4. 002 owner checks: two-phone gallery test (SC-001) and +3 VMAF on real footage (SC-002).
   Optional: AI speed on a faster Windows GPU; the Wayland check of the GIF view; the
   `VK_LOADER_LAYERS_DISABLE=~implicit~` `vulkaninfo` test; Linux NVIDIA VM diagnostics for the
   libEGL DRI3 warning; clean-machine VC++ test in Windows Sandbox; SC-003 timing on a quiet
   machine; cleanup of old cancel output folders on the laptop.

**Security review findings (2026-11-14: no Critical or High, 3 Medium, 6 Low)**

5. **M1 (verified; fix first, with a regression test).** `apps/desktop/src-tauri/src/ops.rs`,
   `resolve_existing` (about line 1055), used by `may_write` (about 81) and the plan step (about
   405): `<picked>/nope/../../elsewhere` passes `starts_with` lexically and writes outside the
   picked output folder. Reject ParentDir and CurDir components, or normalise lexically and then
   canonicalise.
6. **M2.** `ops.rs` `load_manifest` (about 348–361): opening a manifest grants read access to
   every video it names and next-to-source write access in their folders. Decide whether to
   confirm or limit manifest-granted paths; they must not unlock next-to-source writes.
7. **M3.** `.github/workflows/dependabot-notices.yml`: check out `head.sha`, not `head.ref`, and
   use the same SHA for the scope check; set `persist-credentials: false` on the PR checkout and
   give the token only to the push step.
8. **L1.** `crates/rmedia-core/src/tools.rs`: the ffmpeg and ffprobe PATH lookup lacks the
   refuse-cwd guard and the clean environment the GIF engines have.
9. **L2.** `release.yml`: packages are unsigned and have no `SHA256SUMS`, SBOM or provenance
   attestation; `npm ci` runs install scripts; rust-cache is used on tag builds.
10. **L3.** `set_engine_path` has no `-version` banner check for ffmpeg (the GIF engines have one).
11. **L4.** Clip lists and manifests are read without a size cap.
12. **L5.** The `media_server` accept loop spins on persistent errors.
13. **L6.** The desktop CSP allows `img-src` and `media-src` from any 127.0.0.1 port.

**Code review findings (2026-11-14: layering holds)**

14. Medium: `crates/rmedia-core/src/gif/engines.rs` (239–291): the banner and `kill_tree` spawn with
    `std::process`, so no Job Object on Windows; this breaks the `exec::process` rule.
15. Medium: `crates/rmedia-core/src/doctor.rs` (165–200, 263–270) uses synchronous `std::process`
    with no timeout, called inline from the async `doctor_checked` (`ops.rs` 239–283).
16. Medium: `crates/rmedia-core/src/exec/commit.rs` (50–58): the no-clobber fallback (when hard
    links are unavailable) can leave a zero-byte file at the target if the rename fails. Add a
    test.
17. Medium: `exec/gif.rs` (482–500) `SizeWatch` polls every 250 ms and the overshoot bound is
    untested; `exec/gif.rs` (766 lines, 3 unit tests) and `gif/plan.rs` (660 lines, no tests)
    need tests.
18. Low: `apps/desktop/src-tauri/src/ops.rs` `AppState` (1099 lines, about 12 `Mutex<HashMap>`):
    a generic `Registry<T>` and a split per feature.
19. Low: `#[allow(unsafe_code)]` in `upscale/capability.rs` (356) contradicts "unsafe only in
    `rmedia-sr` `ffi.rs`": document the exception or move the code.
20. Low: `expect()` on locks in `crates/rmedia-cli/src/render.rs` (a poisoned lock panics).
21. Tech debt: route every spawn through `exec::process` (the engines banner, doctor capability
    probes, `tools::quiet_command`); split the large modules (`ops.rs`, `exec/gif.rs`,
    `upscale/capability.rs` 1373 lines, `plan/mod.rs` 1189, `xtask/gif_tools.rs` 1487, testkit
    `lib.rs` 1467); stop core calling `Settings::load()` inside doctor and capability code; run
    the GIF tests in the distro-ffmpeg CI job.

**Planned features** (a version follows the feature number: 004 becomes 0.4.0)

22. **004 Smart cut** (next; not yet specified; run `/speckit-specify`, then plan and tasks, in
    `specs/004-smart-cut/`). A hybrid clip mode, `--mode smart` (size M to L): frame-exact start
    and end, interior GOPs stream-copied, the partial head and tail GOPs re-encoded to match the
    source parameters, then joined. H.264 in mp4, mov, mkv and ts first; an automatic fallback to
    accurate mode with a typed code for HEVC, interlaced, VFR, open-GOP, mismatched parameters,
    10-bit or no full GOP. Seams are verified; the dry run reports copied against re-encoded; the
    manifest gets a `mode` value; the desktop app gets a Smart mode and keyframe markers. Out of
    scope: HEVC and AV1 smart cut, joining clips, effects, changes to upscale or GIF, multi-file
    input. Suggested criteria: ±1 frame; at least 80% copied on long clips; no seam decode errors;
    every risky source falls back with a code. Research: `research/01-clipping-discovery.md` §1.4.
23. **005 Budgeted Reddit video**: an MP4 with audio, with size and length caps as data targets.
    Start after the Reddit limits check (item 3).
24. Later candidates (no number yet): a desktop crop-path editor for smart reframe
    (`research/08`); audio-only export with loudness normalisation; frame and timecode input and
    chapter or EDL import.
25. Deferred: animated WebP and AVIF, captions, generative outpaint.

## Release and Project Housekeeping

- [x] Tag `v0.1.0` (`bd1eef8`, the 0.1.0 notes' last commit) and `v0.2.0` (`b6385a1`, 002
  complete) on 2026-11-13; the tag pushes ran the release workflow. The `v0.1.0` build
  (43534837432) fails in that commit's own workflow (Linux apt conflict, fixed in `165b023`;
  Windows tests without the staged ffmpeg); 0.1.0 is superseded, so it has no artifacts. The `v0.2.0` build
  (43534837537) passed: Windows artifact 14090577285 (406,978,945 B, `sha256:bff5b80a…`), Linux
  14089591743 (631,585,854 B, `sha256:c28ae45f…`). Publishing and code signing
  are manual. `v0.3.0` was tagged on 2026-11-14 after the Windows checks (tag build 43573283485, success).
- [x] Decide the application licence: AGPL-3.0-or-later, copyright OpenLogic Distribution Ltd
  (2026-11-13; replaces "ship without gifski" from 2026-10-27). Root `LICENSE`, manifests,
  `app_licence` and cargo-deny exceptions for rmedia's own crates; releases bundle gifski.
- [x] Mirror the pinned third-party archives and publish the FFmpeg corresponding source
  (`mirror-2026-10-22`, `ffmpeg-source-n9.0.2-31-gc89d6ee1ca`; see
  [Build and Release](operations/build-and-release.md)).
- [x] Add a git remote: public `origin` on GitHub, work on `main`, CI green.
- [x] Remove the Windows AI worker's need for the Visual C++ runtime (`vcruntime140`,
  `msvcp140`, `vcomp140`): ncnn built with `/MT` and no OpenMP (`ncnn-static.yml`, release
  `ncnn-static-2026-10-28`), every Windows binary built with `crt-static`, checked in CI (2026-10-28).
  Owner check left: AI speed on Windows hardware (runbook §3.4); if it is noticeably slower,
  ship the three DLLs next to `rmedia-sr.exe` instead.
- [x] Dependabot notices automation (2026-11-13): `dependabot-notices.yml` regenerates
  `THIRD_PARTY_NOTICES.md` on Dependabot's PRs and starts CI; TypeScript 7 is ignored until
  svelte-check supports it.
- [ ] Windows code signing with Azure Artifact Signing (US organisation, decided 2026-10-27):
  account and identity validation by the owner, then a signing step in `release.yml`.
- [ ] Manual desktop QA on Windows hardware (Windows paths and Job Object handling are unverified;
  runbook: [Windows owner checks](operations/windows-owner-checks.md)).

## Related

[Release notes 0.2.0](releases/0.2.0.md) · [Architecture overview](architecture/overview.md) ·
[CLI reference](api/cli-reference.md)
