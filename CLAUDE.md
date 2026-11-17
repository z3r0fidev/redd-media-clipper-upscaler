# rmedia — project guide for agents

Linux + Windows video toolkit: clipping (001), mobile upscaling + smart reframe (002), GIF export
for Reddit (003). Spec-driven via Spec Kit.

## Where things are
- Constitution: `.specify/memory/constitution.md` (read before planning or reviewing).
- Research outlines: `research/` (01 clipping/discovery, 02 upscaling, 03 mobile displays,
  04 stack/packaging, 05 GIF export, 06–09 verified device/upscaler/reframe research for 002). Items marked [K]/[verify]/[unverified] are not yet confirmed.
- Feature specs: `specs/NNN-name/` (spec, plan, research, data-model, contracts, quickstart, tasks).
- Docs: start at `docs/README.md` (user, admin, developer, API, architecture, operations, security, reference).
- Done and released: 001 (`specs/001-video-clipping/`, 0.1.0, tag v0.1.0), 002
  (`specs/002-mobile-upscale/`, 112/112, 0.2.0, tag v0.2.0) and **003 GIF export for Reddit**
  (`specs/003-gif-export/`, 80/80, 0.3.0, tag v0.3.0 on b96bfdd; GitHub release v0.3.0 published
  2026-11-15 from tag build run 43573283485, unsigned). rmedia is AGPL-3.0-or-later; releases bundle gifski.
  Owner checks left: Reddit's limits and a real upload, the 002 two-phone test and +3 VMAF.
  Pending list and review findings: `docs/roadmap.md`.
- Next feature: **004 Smart cut** (`--mode smart`; spec, plan and tasks in
  `specs/004-smart-cut/`, 59 tasks), then 005 budgeted Reddit video.
- 004 progress: T001–T059 done (59/59) (Phases 1–2, US1, the MVP: `rmedia clip --mode smart` works,
  verified and cancellable, and US2: every risky source falls back with a reason,
  `--no-smart-fallback` makes it an error, and US3: the dry run shows the parts, shares and
  reasons, and US4: smart mode in JSON/CSV manifests and desktop exports, with CLI = desktop
  outputs, and US5: the desktop app's Smart mode, keyframe ticks and head/copy/tail bands; `research.md` R3/R5/R7 record what changed). Security L1 is done too. Phase 8 is done:
  docs, validation and T059's reviews (no high; mediums fixed; `specs/004-smart-cut/progress.md`).
  Next: the 0.4.0 tag build, then the owner checks in `docs/operations/smart-cut-owner-checks.md`
  (all platforms) before publishing the release (`docs/releases/0.4.0.md`). Run tests with `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` set to
  `binaries/rmedia-ff*-x86_64-unknown-linux-gnu` (the system ffmpeg lacks libvmaf).
- Session close: follow `docs/operations/session-close.md`.
- The AI worker `crates/rmedia-sr` needs `cargo xtask fetch-ncnn` (and `fetch-models` for its GPU
  tests) before `cargo build --workspace`.
- Git: public remote `origin` (github.com/z3r0fidev/redd-media-clipper-upscaler), work on `main`.
  CI must be green. Pinned third-party downloads are mirrored: after `cargo xtask pin-tools`,
  run the `ffmpeg-source` workflow and `cargo xtask mirror mirror-<date>`; `cargo xtask
  check-pins --online` must pass.

## Stack
- Rust 1.94 workspace: `crates/rmedia-core` (ALL media logic), `crates/rmedia-cli` (bin `rmedia`),
  `crates/rmedia-testkit` (lavfi fixtures), `crates/rmedia-sr` (AI worker over ncnn; `unsafe` only
  in its `ffi.rs`), `apps/desktop` (Tauri 2.x + Svelte 5 + TS), `xtask`.
- FFmpeg/ffprobe are subprocesses only (never linked); bundled as `rmedia-ffmpeg`/`rmedia-ffprobe`.

## Rules
- No media logic in CLI or Tauri crates; add it to core and expose it.
- Spawn tools with argv only, paths as `file:<path>`, through `exec::process` (kill-tree aware).
- Never write to a source file; outputs go through `.part` → verify → no-clobber commit.
- Tests first; generate media with `rmedia-testkit`; never commit binary fixtures.
- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` must pass.
- Commits: Conventional Commits.

<!-- SPECKIT START -->
Active plan: specs/004-smart-cut/plan.md
<!-- SPECKIT END -->
