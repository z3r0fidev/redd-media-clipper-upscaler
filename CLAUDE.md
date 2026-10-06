# rmedia — project guide for agents

Linux + Windows video toolkit: clipping (001), mobile upscaling + smart reframe (002), GIF export
for Reddit (003). Spec-driven via Spec Kit.

## Where things are
- Constitution: `.specify/memory/constitution.md` (read before planning or reviewing).
- Research outlines: `research/` (01 clipping/discovery, 02 upscaling, 03 mobile displays,
  04 stack/packaging, 05 GIF export). Items marked [K]/[verify]/[unverified] are not yet confirmed.
- Feature specs: `specs/NNN-name/` (spec, plan, research, data-model, contracts, quickstart, tasks).
- Active feature: `specs/001-video-clipping/`.

## Stack
- Rust 1.94 workspace: `crates/rmedia-core` (ALL media logic), `crates/rmedia-cli` (bin `rmedia`),
  `crates/rmedia-testkit` (lavfi fixtures), `apps/desktop` (Tauri 2.x + Svelte 5 + TS), `xtask`.
- FFmpeg/ffprobe are subprocesses only (never linked); bundled as `rmedia-ffmpeg`/`rmedia-ffprobe`.

## Rules
- No media logic in CLI or Tauri crates; add it to core and expose it.
- Spawn tools with argv only, paths as `file:<path>`, through `exec::process` (kill-tree aware).
- Never write to a source file; outputs go through `.part` → verify → no-clobber commit.
- Tests first; generate media with `rmedia-testkit`; never commit binary fixtures.
- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` must pass.
- Commits: Conventional Commits.

<!-- SPECKIT START -->
Active plan: specs/001-video-clipping/plan.md
<!-- SPECKIT END -->
