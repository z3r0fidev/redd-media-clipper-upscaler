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

## US2 checkpoint (T031–T039, 2026-11-16)

| Criterion | Result |
|---|---|
| SC-004 every risky source falls back with a code | `smart_us2::risky_sources_fall_back`: `encoder_missing`, `container`, `profile` (High 10), `interlaced`, `vfr`, `rotation` (to `.ts`), `no_full_gop`, `open_gop` and `codec` (HEVC) each give a frame-exact accurate clip, `copied_frames: 0`, the reason in the outcome, the `SMART_FALLBACK` notice with `detail.reason`, and the same warning as an event |
| Run-time fallbacks (scenario 3) | `run_time_fallback`: a resolution change inside the copied GOPs → `params_changed`; parts joined in the wrong order (test hook) → `seam_check`; the joined file is discarded and the clip redone in accurate mode (85 / 435 frames), no `.part` left |
| Strict mode (FR-009) | `strict_mode`: `SMART_UNAVAILABLE` planning issue with `detail.reason` and no job; at run time the job fails with `SMART_UNAVAILABLE` and nothing is written. CLI `no_smart_fallback_exit_codes`: 0 / 3 (open GOP) and 0 / 1 (resolution change) |
| FR-013 not applied | `not_applied_with_upscale_or_gif`: `SMART_NOT_APPLIED` notice, and the same commands and other warnings as accurate mode |
| Rule order (R2) | `smart::tests`: one unit case per rule, a source with several problems reports the first |
| Doctor (FR-017) | `smart cut: available (libx264)`; JSON `"smart": {"available": true, "reason": null}` |

Reviews: self-review of the diff. The senior-code-reviewer pass the checkpoint lists was not run
(agents are spawned only when asked). The quickstart's dry-run line for a fallback comes with US3.

## US3 checkpoint (T040–T042, 2026-11-16)

| Criterion | Result |
|---|---|
| Per-job dry-run lines | CLI `batch::smart_dry_run`: `smart: head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s (18 fr) re-encoded · 92% copied`, a copy-only job (`100% copied`), and `smart → accurate: <reason>` for the open-GOP and VFR sources |
| Batch line | `smart: 3 jobs; fallback: 2 (open_gop 1, vfr 1)` |
| JSON contract | `contract_clip_json::smart_plan_json` snapshot: the job's `smart` block, the five argv in `pipeline` (head, copy, tail, audio, join), `cwd`, and the plan's `smart` totals. The fallback job's block matches contracts/json-output.md exactly |
| Strict mode in the dry run | `--no-smart-fallback --dry-run --json` exits 3 with two `SMART_UNAVAILABLE` issues, `detail.reason` `open_gop` and `vfr` |
| Nothing written | no clips folder and no `.rmedia-` scratch folder after either dry run |
| Core | `smart_us2`: every fallback's `SmartSummary` and `SmartTotals`; upscale and GIF jobs have no summary and count as `not_applied` |

Reviews: self-review of the diff; the senior-code-reviewer pass was not run (agents are spawned
only when asked).

## US4 checkpoint (T043–T046, 2026-11-16)

| Criterion | Result |
|---|---|
| JSON = CSV, levels | `us3_batch::smart_json_and_csv_manifests_give_identical_outputs`: smart in `defaults`, `accurate` on a video, `smart` on one of its clips and `smart_fallback: false` on another; both forms plan smart, strict smart, accurate, smart (clip beats video beats defaults) and give byte-identical outputs |
| SC-009 0.3.0 manifests | `smart_absent_from_0_3_0_manifests`: no `smart` field anywhere in the plan; JSON and CSV outputs identical. The 0.3.0 command snapshots are unchanged since the baseline commit `f286b44` (only smart snapshots were added) |
| CSV writer | `smart_us4::csv_writer_round_trip`: `smart_fallback` follows `mode` only when a row is strict; smart-only and 0.3.0 exports keep the 0.3.0 header; write → read gives the same options |
| Desktop parity (FR-019) | `bridge::smart_parity`: the app's run of a smart queue (one strict clip, one accurate) and `rmedia batch` of its JSON and CSV exports give byte-identical files |

Reviews: self-review of the diff; the senior-code-reviewer pass was not run (agents are spawned
only when asked).
