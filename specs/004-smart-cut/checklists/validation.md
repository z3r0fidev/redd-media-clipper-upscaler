# Validation: Smart Cut

Results per task and checkpoint for feature 004. Engine: the bundled FFmpeg n9.0.2
(`binaries/rmedia-ffmpeg-*`), with `RMEDIA_REQUIRE_FFMPEG=1` and `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE`
pointing at it.

## Baseline (T001, 2026-11-15, commit `f286b44`)

- `cargo test --workspace`: **505 passed, 0 failed, 15 ignored**.
  Without `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` the tests use the system ffmpeg, which has no libvmaf,
  and `gif_gifski::gifski_beats_the_built_in_engine_on_vmaf` panics (by design under
  `RMEDIA_REQUIRE_FFMPEG=1`).
- Desktop (previous session's count, same code): 5 unit + 13 bridge + 4 other = 22 passed.
- Insta snapshots: `crates/rmedia-core/tests/snapshots/` and `crates/rmedia-cli/tests/snapshots/`;
  none may change before US1 (T021 adds new ones).

## Phase 2 (T005–T014)

- Spikes S1–S3: `research.md` R3 and R5.
- `--mode smart` runs as accurate (T008): `rmedia clip src.mp4 -s 3.32 -e 14.72 --mode smart`
  gives the 285-frame accurate clip.
