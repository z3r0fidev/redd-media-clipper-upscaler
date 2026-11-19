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

## US1 (T011–T022), 2026-11-19

| Check | Test | Result |
|---|---|---|
| SC-001, cap met | `core-tests/fit_us1.rs` `a_tight_cap_is_met_with_sound_in_sync` (`FitBusy` 2–27 s, 8 MB) | pass: ≤ cap |
| SC-002, ≥ 90% used | same | pass |
| SC-003, ±1 frame, audio in sync | same: 25 s ± 1/30 s; audio start within 1024/48000 s | pass |
| FR-016, H.264 High yuv420p, AAC LC stereo, fast start, clean decode | same | pass |
| FR-007, quality mode not inflated; no audio means no audio track | `a_generous_cap_is_not_filled_and_no_audio_means_no_audio_track` | pass |
| FR-005/FR-010, retry, then `FIT_FAILED` with nothing left; minimum-bitrate stop | `core-tests/fit_retry.rs` (3 tests) | pass |
| Principle II, repeatable bytes | `a_fit_is_deterministic` | pass, after R13 |
| SC-008 (004's), cancel in pass 1 and pass 2 within 2 s, clean | `core-tests/cancel.rs` `cancelling_a_fit_in_either_pass_is_fast_and_clean` | pass |
| argv | `core-tests/snapshots_fit_args.rs` (3 snapshots) | accepted after review |
| CLI result line, JSON `fit`, exit codes 2/3 | `cli-tests/fit.rs` (2 tests) | pass |
| planner invariant: `predicted ≤ cap × 0.98` over a grid | `fit::plan` unit tests (10) | pass |
