# rmedia — project guide for agents

Linux + Windows video toolkit: clipping (001), mobile upscaling + smart reframe (002), GIF export
for Reddit (003). Spec-driven via Spec Kit.

## Where things are
- Constitution: `.specify/memory/constitution.md` (read before planning or reviewing).
- Research outlines: `research/` (01 clipping/discovery, 02 upscaling, 03 mobile displays,
  04 stack/packaging, 05 GIF export, 06–09 verified device/upscaler/reframe research for 002). Items marked [K]/[verify]/[unverified] are not yet confirmed.
- Feature specs: `specs/NNN-name/` (spec, plan, research, data-model, contracts, quickstart, tasks).
- Docs: start at `docs/README.md` (user, admin, developer, API, architecture, operations, security, reference).
- Active feature: `specs/002-mobile-upscale/` (105/112 tasks; Setup, Foundational incl. refactors
  A1–A5, US1 CPU tier, US2 smart reframe, US3 GPU tier + capability record, US4 batch/`clip
  --upscale`, US5 desktop view and US6 AI tier are done; US6 owner checks need a discrete GPU).
  Next: Polish (T106–T112). The AI worker `crates/rmedia-sr` needs `cargo xtask fetch-ncnn`
  (and `fetch-models` for its GPU tests) before `cargo build --workspace`. Progress: `specs/002-mobile-upscale/progress.md`; roadmap: `docs/roadmap.md`.
  001 (`specs/001-video-clipping/`) is implemented. Planned: 003 GIF export (research/05, no spec yet).
- Git: no remote and no `main` branch yet; work is on `001-video-clipping` and `002-mobile-upscale`.

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
Active plan: specs/002-mobile-upscale/plan.md
<!-- SPECKIT END -->
