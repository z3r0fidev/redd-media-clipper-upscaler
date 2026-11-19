# Validation: Budgeted Reddit Video (005)

## T001 baseline (2026-11-19, `27d2a11`/`81b81c7`, before any 005 code)

Run with the bundled engine (`RMEDIA_REQUIRE_FFMPEG=1`, `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` =
`binaries/rmedia-ff*-x86_64-unknown-linux-gnu`):

| Suite | Result |
|---|---|
| `cargo test --workspace` | 581 passed, 0 failed |
| `apps/desktop/src-tauri`: `cargo test` | 34 passed |
| `apps/desktop`: `npx vitest run` | 48 passed |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, desktop clippy | clean |
| MinGW Windows check (`--target x86_64-pc-windows-gnu --exclude rmedia-sr`) | clean |

Snapshots: the files in `crates/rmedia-core/tests/snapshots/` and
`crates/rmedia-cli/tests/snapshots/`. Every 005 phase must keep them unchanged for jobs without a
fit (FR-022).
