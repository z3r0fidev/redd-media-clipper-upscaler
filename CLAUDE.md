# rmedia — project guide for agents

Linux + Windows video toolkit: clipping (001), mobile upscaling + smart reframe (002), GIF export
for Reddit (003). Spec-driven via Spec Kit.

## Where things are
- Constitution: `.specify/memory/constitution.md` (read before planning or reviewing).
- Research outlines: `research/` (01 clipping/discovery, 02 upscaling, 03 mobile displays,
  04 stack/packaging, 05 GIF export, 06–09 verified device/upscaler/reframe research for 002). Items marked [K]/[verify]/[unverified] are not yet confirmed.
- Feature specs: `specs/NNN-name/` (spec, plan, research, data-model, contracts, quickstart, tasks).
- Docs: start at `docs/README.md` (user, admin, developer, API, architecture, operations, security, reference).
- Active feature: **003 GIF export for Reddit** — not started: no spec yet (research/05). Next step:
  `/speckit-specify`, then plan mode, as for 002.
- Done: 001 (`specs/001-video-clipping/`, 0.1.0) and 002 (`specs/002-mobile-upscale/`, 112/112,
  0.2.0 release candidate; owner checks left: two-phone gallery test, Reddit upload, +3 VMAF on
  real footage). The AI worker `crates/rmedia-sr` needs `cargo xtask fetch-ncnn` (and
  `fetch-models` for its GPU tests) before `cargo build --workspace`. Roadmap: `docs/roadmap.md`.
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
Active plan: none (003 has no spec yet; the last plan was specs/002-mobile-upscale/plan.md)
<!-- SPECKIT END -->
