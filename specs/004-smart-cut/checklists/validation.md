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

## US1 checkpoint (T015–T030, 2026-11-15)

| Criterion | Result |
|---|---|
| SC-001 frame-exact | `smart_us1::frame_exact_ends`: mp4, mov, mkv, ts and Baseline (no B-frames, no audio) give exactly the requested frames (435 / 335), the first frame matching the source at 3.32 s |
| SC-002 copied share | 550 of 585 frames copied (94%) for a 23.4 s clip; every copied frame identical (framemd5) |
| SC-003 seams | 0 decode errors, an even 40 ms frame step, audio within 20 ms of video, on mp4, mkv, ts and `OtherEncoder` (all smart-cut, none fell back) |
| SC-005 speed | `bench_sc005_smart_vs_accurate` (release, alone on the machine): 60.4 s of 1080p, accurate 24.4 s, smart 6.4 s (26%; target ≤ 33%) |
| SC-006 quality | the re-encoded head scores within 0.5 VMAF of accurate mode for the same frames (`OtherEncoder`) |
| SC-008 cancel | cancelled during each of head, copy, tail, audio, join and verify: under 2 s, no `.part`, no scratch folder, no engine process |
| R10 determinism | two smart cuts of the same range are byte-identical |
| JSON contract | `contract_clip_json::clip_smart_outcome_events` snapshot (`smart` block); human line `(smart, 94% copied)` |

Found and fixed on the way (research R3, R7):
- The segment muxer's split times depend on where a demuxer seek lands; Matroska lands a GOP
  early, so the "middle" began a GOP early. The seam check caught it (copied frames not the
  source's). The copy now works as fast mode does, ended by a packet count.
- MPEG-TS window reads that start with a seek can begin inside a GOP; the index now ignores
  packets before the first keyframe.
- `-autoscale 0` is not valid (`-noautoscale` is); the first build's every seam check failed and
  fell back cleanly to accurate mode.

Reviews: self-review of the diff (a pure copy now also gets its first GOP compared with the
source). The senior-code-reviewer / security-auditor passes the checkpoint lists were not run in
this session (agents are spawned only when asked).
