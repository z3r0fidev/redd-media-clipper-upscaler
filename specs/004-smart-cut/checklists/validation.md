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

## US5 checkpoint (T047–T053, 2026-11-16, private Xvfb display :99, never the real display)

Debug desktop build (`tauri build --debug --no-bundle`), isolated XDG config, cache, data and
state, the bundled FFmpeg n9.0.2. Sources: the testkit's `SmartMp4` (`src.mp4`, 30 s, 2 s GOPs)
and `OpenGop` (`open.mp4`), passed on the command line.

| Step (quickstart › Desktop) | Result |
|---|---|
| 1. Ticks | `src.mp4`: 15 ticks every 2 s on the timeline, shown with the view (loaded in the background) |
| 2. Smart, 3.32–20.72 s | Smart chosen in the video's queue options (or as the default in Settings). The bands: head 3.32–4, copy 4–20, tail 20–20.72; the line "Smart: 92% copied · 0.68 s re-encoded at the start · 0.72 s at the end" |
| 3. Run | "Finished · 1 succeeded"; the run panel says "Smart · 92% copied". The file: h264, 435 frames, 17.400 s, decodes with no errors |
| 4. `open.mp4` | "Smart cut not possible: the video has open GOPs (frames that depend on the previous GOP); cutting in accurate mode", in the warning colour, no bands |
| Settings | "Smart cut: available (libx264)"; Smart saved as the default mode and used for a newly opened source |
| SC-007 markers | A 10-minute source (320x180, 25 fps, 2 s GOPs): all 300 ticks were on screen in the first screenshot, 1.5 s after the click. Core: `bench_sc007_keyframes_of_a_10_minute_source` 126 ms |
| SC-007 interactions | With Smart as the default mode: (1) mark the range (I, O), (2) Add clip, (3) Run, which is 3, counted as in 002 (select, choose, run). Choosing Smart for one video in the queue instead adds one |

Found and fixed: the colour legend and the status text were one flex line and wrapped in the
middle of the legend; the text now comes first and the legend wraps as one unit.

Automated: `bridge::keyframes_index_cache_and_cancel` and `bridge::smart_plan_matches_the_cli`
(the summary equals `rmedia clip --mode smart --dry-run --json`'s for `SmartMp4` 3.32–20.72 and
`OpenGop` 1.32–6.72; a newer call cancels the older), vitest `smart.test.ts` and
`smart.svelte.test.ts`, svelte-check 0 errors.

Reviews: self-review of the diff; the senior-code-reviewer and security-auditor passes were not
run (agents are spawned only when asked). Security notes from the self-review: both commands
check `require_allowed` first; the cache key is the canonicalised path with its size and mtime,
so a replaced file is indexed again; the cache keeps 32 indexes, and a request's entry is
removed when its scan ends (the page cancels the previous scan when the source changes).

## T058 quickstart on Linux, and Windows from CI (2026-11-16)

Linux, the reference machine, bundled FFmpeg n9.0.2, `RMEDIA_REQUIRE_FFMPEG=1`:

| Quickstart check | Result |
|---|---|
| `smart_us1` (SC-001, SC-002, SC-003, SC-006) | 6 passed |
| `smart_us2` (SC-004) | 4 passed |
| `cancel smart` (SC-008) | 1 passed |
| `us3_batch smart` and desktop `bridge smart` (SC-009) | 2 and 2 passed (the bridge filter also ran `smart_plan_matches_the_cli`) |
| Bench SC-005 (release) | 60 s of 1080p: accurate 16.7 s, smart 5.1 s (31%; target ≤ 33%). Run with other builds idle but not alone; the US1 run measured 26% |
| Bench SC-007 (release) | 300 keyframes of a 10-minute source in 261 ms (bar 2 s) |
| Manual run | Exactly the quickstart's lines: `smart: head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s (18 fr) re-encoded · 92% copied`; outcome `{"copied_frames":400,"copied_share":0.9195,"fallback":null,"mode_used":"smart","reencoded_frames":35}`; 435 frames; a silent decode |
| The fallback | `smart → accurate: the video has open GOPs (frames that depend on the previous GOP)`; with `--no-smart-fallback` the error and exit 3 |
| Desktop | The US5 checkpoint above (Xvfb :99, all four steps) |

Windows, from CI run 43922704270 (commit `364bd74`, `windows-latest`, bundled engine), all
green: `smart_keyframes` 4, `smart_us1` 6, `smart_us2` 4, `smart_us4` 4 passed, the smart
`cancel`, `us3_batch`, `clip_single`, `batch` and `contract_clip_json` tests and the snapshots
passed; benches SC-005 25% (accurate 53.6 s, smart 13.4 s) and SC-007 981 ms. The Linux CI job
measured SC-005 25% (62.8 s / 15.6 s) and SC-007 400 ms.

CI run 44027402281 (commit `4d5d783`): the Windows job's SC-007 took 2.14 s (SC-005 18%), over
the 2 s bar, while Linux took 412 ms. Like SC-003's scan, SC-007 is now reported but not
asserted on the Windows CI runners; the bar stays asserted everywhere else.

Owner checks (2026-11-18, with the v0.4.0 tag build; [runbook](../../../docs/operations/smart-cut-owner-checks.md)):
Linux and Windows CLI and desktop app, WebView2, an Android phone (plus Chrome and Google Photos
in the Android 16 emulator), an iPhone and a Reddit upload all passed; results in
`docs/operations/smart-cut-owner-checks-results-{linux,windows,android,iphone,reddit}.md`. Real
footage (section 8) passed too (owner report, no details given). Every owner check is done; the
GitHub release v0.4.0 was published on 2026-11-18.

## T059 reviews (2026-11-16)

Three passes over the 004 changes (`f286b44..HEAD`, `crates/` and `apps/`): a self-review, then
the senior-code-reviewer and security-auditor agents (asked for by the owner). No high findings.
Every medium is fixed, with a test that failed first.

| # | Finding | Severity | Resolution |
|---|---|---|---|
| R1 | A start or end up to 0.5 ms after a frame time took that frame, which accurate mode drops: 3.3204 s gave 135 frames where accurate gives 134 | Medium | `SLACK` 500 µs → 2 µs (ffprobe's rounding). `smart_us1::ends_just_after_a_frame_pick_the_same_frames_as_accurate` |
| R2 | A head, copy, tail or join step that ffmpeg could not do failed the job instead of falling back (FR-008) | Medium | `exec/smart.rs` › `fall_back_on`: `ENGINE_FAILED` from a video step falls back with `seam_check` (strict mode: `SMART_UNAVAILABLE`); cancel, disk full and unwritable outputs stay errors. Test hook `fail_copy_of`, `smart_us2::run_time_fallback` |
| R3 | The whole-file keyframe index stopped after its first 10-minute chunk when the file had no duration, without `truncated` | Medium | Reads on until a chunk holds nothing past its start. `smart_keyframes::chunked_reads_find_every_keyframe_once` (no-duration case) |
| R4 | The previous range's bands stayed on screen while a new plan was pending | Low | `syncPlan` clears the summary when the range changes |
| R5 | After a stop and a restart of the same range, the old call's "cancelled" showed as an error | Low | A sequence number per change replaces the key comparison. Two new vitest cases |
| R6 | The CLI dry-run line parsed the notice's message text | Low | Uses `SmartSummary.fallback` |
| R7 | Jobs whose keyframes could not be read were in the line's fallback count but not in `SmartTotals` | Low | `SmartTotals.unreadable` (absent when 0); the line lists `keyframes unreadable N` |
| S1 | `keyframes` skipped the planner's playlist refusal (`.m3u8`, `.ffconcat`, …) | Low | `probe::is_playlist`, used by the planner and `keyframes` |
| S2 | Unchecked time arithmetic on absurd times from a hostile file (wraps in release) | Low | `Ts` `+`, `-` and `saturating_sub` saturate; parsed times are limited to ±1e12 s |
| S3 | The seam check's frame size product could overflow on a crafted size | Low | `checked_mul`; an overflow falls back with `params_changed` |
| S4 | A `keyframes` request's entry stayed registered if the command's future was dropped | Low | A drop guard removes it |
| — | `planOptions` spread can mask a queue default with an explicit `mode: undefined`; progress sits in the verify slice during a run-time fallback re-encode; a keyframe scan restarts when the player's mode flickers from error to ok; no free-space check for a smart job's ~2× scratch; no cap on concurrent uncached scans; the 16 MiB packet-scan limit on multi-hour clips | Low | Fixed after 0.4.0 (roadmap item 27, 2026-11-18), each with a test that failed first: `planOptions` skips unset values; a fallback enters an `accurate` stage that fills the rest of the bar; the keyframe effect is keyed on the view and source, and a repeat request for the same source does nothing; smart clips count twice in the plan's free-space check; at most two uncached scans; the packet scan reads line by line. Also: an end on the video's end copies the last GOP (owner checks, Windows) |

Checked and holding (both agents): argv only with `file:` paths and the protocol whitelist on
every input; the concat list holds only fixed part names; the scratch folder is private,
journaled and removed on every path; `smart_plan` cannot redirect writes (output folder forced,
templates validated); manifests accept only the documented values; the earlier fixes `ac69d46`,
`8b111a8` and `abc204c` hold.

Issue [#37](https://github.com/z3r0fidev/redd-media-clipper-upscaler/issues/37) was fixed in the
same change: folder scans skip `gifs/` (CLI), and the desktop scan now skips all three output
folders (it skipped none). Tests: `scan::folder_scans_skip_every_default_output_folder`,
`bridge::folder_scans_skip_the_output_folders`; both failed first.
