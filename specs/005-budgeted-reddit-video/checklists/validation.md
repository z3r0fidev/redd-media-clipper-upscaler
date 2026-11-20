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

## US2 (T023–T029), 2026-11-19

| Check | Test | Result |
|---|---|---|
| FR-008, `RANGE_TOO_LONG` with `detail {len, max_length}`; trim to the limit with `FIT_TRIMMED` | `core-tests/fit_us2.rs` `too_long_and_unreachable_ranges_are_refused_or_trimmed` (user target `short`, 10 s) | pass |
| FR-009, `BUDGET_UNREACHABLE` with R4's `min_cap`; trim at the floor size and audio | same | pass |
| FR-011, one ladder step 1920x1080 → 1440x810 with `FIT_LOWERED` | same | pass |
| SC-005, a trimmed run is the first 10 s (start ±1 frame); the lowered size is encoded | `trims_and_sizes_are_what_was_planned` | pass |
| FR-012, never enlarged (640x360 kept); 4K fitted to 1920x1080 | same (new fixture `FitUhd`) | pass |
| HDR10 → SDR BT.709 `yuv420p` (002's tone-map) | `awkward_sources_give_a_plain_reddit_mp4_or_are_refused` | pass |
| VFR capped at 24 fps by a user target, length ±1 frame; 120 fps → 60 | same | pass |
| 5.1 → stereo with a notice; audio-only refused (`PROBE_FAILED`, as for every clip) | same | pass |
| CLI: exit 3 with the messages, nothing written; an override above the target exits 2; `--json` `detail.min_cap` | `cli-tests/fit.rs` `refusals_exit_before_any_encode` | pass |

Fixed on the way:
- The JSON plan document replaced a fit's `BUDGET_UNREACHABLE` detail (`min_cap`) with GIF's
  static `min_bytes`; an issue's own detail now wins.
- `FIT_TRIMMED`'s detail is `{from, to}`, as contracts/json-output.md says (it was `{len,
  max_length}`).
- A fit without libx264 (or aac, with audio) is `ENGINE_MISSING` at plan time (Principle VI).
- The TypeScript bindings for the 005 types were regenerated (US1 had not).
- contracts/cli.md's refusal example now shows the real text format (origin, then message).

## US3 (T030–T033), 2026-11-19

| Check | Test | Result |
|---|---|---|
| FR-014, the dry-run line for budget mode, quality mode and a lowered size; nothing written | `cli-tests/fit.rs` `the_dry_run_shows_the_fit_and_its_prediction_holds` | pass |
| `--dry-run --json` `fit` block: exactly the contract's 15 keys | same (key set, no `jsonschema` in the repo) | pass |
| SC-006, prediction within 15% of the real size | same (`FitBusy` 2–27 s, 8 MB, budget mode) | pass |
| FR-004, `clip --list-fit-targets` text and `--json`, a user target, a broken file reported | `fit_targets_are_listed_and_reported_by_doctor` | pass |
| `doctor --json` `fit.targets`/`fit.problems`; a broken file leaves `ok` and the exit code alone | same | pass |

Changed on the way: `FIT_LOWERED` messages print sizes as `1920x1080` (they printed JSON arrays);
contracts/cli.md's examples now show the real output (decimal units: 1 GiB is 1.07 GB).

## US4 (T034–T038), 2026-11-19

| Check | Test | Result |
|---|---|---|
| FR-019, `reddit-916` + `reddit-video`: 1080x1920 H.264 High `yuv420p` BT.709 with AAC, 90–100% of a 3 MB cap | `core-tests/fit_us4.rs` `an_upscale_to_a_device_shape_fits_its_cap` (CPU tier, blur reframe) | pass |
| A tighter cap lowers the size after the preset's graph: 810x1440 within 1.2 MB | same | pass |
| `--hdr keep` with `--fit` refused (a fit is SDR H.264) | `hdr_keep_and_fit_are_refused_together` | pass |
| R7, the AI tier runs its pipeline in both passes; H.264 within the cap | `core-tests/us6_pipeline.rs` `a_fitted_ai_upscale_runs_the_pipeline_in_both_passes` (built-in nearest model) | pass |
| FR-017, JSON `fit` at every level, `enabled: false`, sizes as text or bytes, unknown keys refused, whole videos | `manifest_json` unit test `fit_at_every_level` | pass |
| CSV `fit`, `fit_max_size`, `fit_max_length`, `fit_allow_trim`, `none`, errors by line; CSV = JSON; round trip | `manifest_csv` unit tests `fit_columns`, `fit_csv_equals_json_and_round_trips` | pass |
| A batch: top-level target, one override, one off; each output within its own cap; smart mode noted `SMART_NOT_APPLIED`, cut accurate | `fit_us4.rs` `a_batch_fits_each_clip_to_its_own_cap` | pass |
| CLI: `upscale --fit` and `clip --upscale --fit` dry runs and a real run's result line; flag misuse and overrides exit 2 | `cli-tests/fit.rs` `upscales_can_be_fitted_from_the_cli` | pass |

Not run here: the GPU tier's "same size class" check (no usable Vulkan device on this machine;
the GPU tier shares the CPU tier's fit tail, `encode_tail`).

Design notes: the fit lives on `UpscaleSpec.cut.fit` (one field for clips and upscales). A lowered
size is a `scale` appended to the upscale graph's `[v]`, so every tier shares one tail. A trimmed
upscale is planned again for the shorter range. The dry-run time estimate doubles in budget mode.
`fit` with `gif` on one clip is refused by the planner (exit 3), as tested in US1.
