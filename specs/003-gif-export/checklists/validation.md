# Validation log: 003 GIF Export for Reddit

## Baseline before 003 (T001)

- Date: 2026-10-24, commit 6cb2236, bundled engine n9.0.2, `RMEDIA_REQUIRE_FFMPEG=1`.
- `cargo test --workspace`: **300 passed, 0 failed, 10 ignored** (benches, GPU and real-model
  tests).
- Desktop: `cargo test` in `apps/desktop/src-tauri` **16 passed**; `npm test` (vitest)
  **24 passed** in 4 files.
- insta snapshots (10), which the Phase 2 refactors must leave unchanged:
  - `contract_clip_json__clip_dry_run_job_document`, `contract_clip_json__clip_run_outcome_events`;
  - `snapshots_ffargs__` `mkv_fast_copies_everything`,
    `mkv_to_mp4_accurate_second_audio_converts_subs`, `mkv_to_ts_drops_subtitles_with_warning`,
    `mp4_accurate_single_seek`, `mp4_fast_two_stage_keyframe_cut`, `ts_accurate_copyts_trim`,
    `video_only_accurate_has_no_audio_codec`, `webm_accurate_vp9_opus`.
- Every Phase 2 refactor must keep these passing.

## Phase 2 security review (2026-10-24)

security-auditor over the GIF engine supply chain, pipelines and engine lookup: no critical
findings. Fixed before the first gif-tools run:
- HIGH: `pin-gif-tools` now verifies attestations with `--source-ref refs/heads/main`,
  `--source-digest`/`--signer-digest` = the run's commit and `--deny-self-hosted-runners`,
  matches `build_run:` on a line of its own and requires the release to target that commit.
  Every job of `gif-tools.yml` is guarded to `main`.
- The sources and the vendor tarball are attested too; `pin` checks every vendored package's
  `.cargo-checksum.json` against the pinned `Cargo.lock`.
- gifski is built (and pinned) outside the repository tree; job outputs reach `run:` through
  `env:` only; archives with links or special files are refused; the toolchain is pinned.
- Banner checks run in a neutral folder and their own process group; pipelines have a
  no-progress watchdog and bounded stderr joins; "broken pipe" is read from the last stderr line
  only.
- Target and preset files are opened `O_NOFOLLOW|O_NONBLOCK` and re-checked on the handle;
  directory listing and problem messages are bounded and sanitised (controls and bidi);
  `max_duration` ≤ 3600 s.
- Accepted / deferred: the PATH refusal for input and scratch folders is wired by the planner
  (T039); a vendored build script could still tamper with its own job (the binary is built from
  pinned sources, and the attestation states the workflow and commit, nothing more); a second
  run on the same day fails at `gh release create` (the tag exists).

## US1 checkpoint (2026-10-25)

Quickstart › US1 and Doctor pass with the pinned engines staged (Ctrl-C during a gifski encode:
exit 130 after 52 ms, no `.part`/`.d`, no engine processes left). senior-code-reviewer and
security-auditor over `fe90eba..31cb853`: no critical or high findings.

Fixed:
- Supply chain: the Debian comparison allows release-only autotools output by exact path, not
  basename; Debian's tarball (not hash-pinned) and every crate tarball are scanned for links and
  escaping paths before extraction; a missing Debian copy fails the pin unless
  `--without-debian`; the shipped gifski lock may hold only crates.io sources; and every vendored
  package is compared file by file with its `.crate` from crates.io, checked against the lock
  (`.cargo-checksum.json` is self-declared). Re-pinning run 40211502841: 102 packages identical,
  lock and notices unchanged.
- `strip_loop_block` reads only the file's head and streams the rest into a newly created
  `<file>.strip`, off the async runtime (no 4 GiB read for a play-once GIF).
- The gifsicle post-pass has the size watchdog and a 15 min limit; sample encodes (built-in,
  gifski, gifsicle) have a 512 MiB watchdog and a 120 s limit, and cover at most 3 s (ranges up
  to 3 s are encoded whole, 150 frames at 50 fps, contracts/tauri-commands.md).
- The cross-file-system fallback of `move_into` copies into a newly created temp file, never
  through an entry planted at the `.part` path.
- A producer that lost its reader while gifski exited 0 is a gifski failure (it stopped reading),
  not a success. gifski outputs are verified by total display time, as after gifsicle.
- The size model no longer collapses at 1 fps (every later frame starts a window); the progress
  file is read once more after the pipe ends; `SizeWatch` and the new `Deadline` stop their task
  on drop; the release's no-gifski merge patch must drop exactly one sidecar and three licences.
- Checked, no change: ffprobe reports GIF delays as written (1 cs at 100 fps), so the minimum
  delay check works; a test pins it.

Accepted / deferred:
- gifsicle's autotools output (`configure`, `Makefile.in`, …) is covered only by the release's
  sha256 pin; regenerating it with `autoreconf` from the Debian-matched tree is a follow-up.
- An engine found through settings, `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI` or PATH is checked by its
  banner once and spawned later (not content-pinned); only the bundled sidecar is pinned.
- On Windows, a dead gifski may surface as an ffmpeg write error that is not "Broken pipe" and so
  blame ffmpeg. Closed 2026-10-26: on Windows, ffmpeg's write/mux lines ending in "Invalid
  argument" or "End of file" also count as a lost reader, and
  `gif_gifski::a_gifski_that_dies_is_blamed_not_ffmpeg` runs on both OSes in CI. Confirmed
  2026-10-27: it passed on `windows-latest` (CI run 40445970256 on `a69089a`).
- The 300 s pipe stall counts from spawn, before ffmpeg's first progress block.
- Over the hard limit, a GIF fails verification until US2's budget search lowers the settings.
- `GIFSKI_LOCK_UPDATES` is listed in both gif-tools.yml and xtask; the pin's lock comparison
  catches any drift.

## US2 calibration (2026-10-25)

- SC-002, SC-003, SC-004 and SC-005 (same size) pass; numbers in tasks.md › T053.
- **Open (owner): SC-009.** With gifsicle the default `reddit-post` GIF is 6.4% smaller over the
  synthetic set (12.3% at the maximum lossy level), not 15%, with at most 0.76 VMAF lost. The
  bench asserts a provisional 5% floor until the owner either sets an automated bar for the
  synthetic set (as 002 did for its VMAF target) with 15% as a real-footage check, or changes
  the target's default lossy level.

## US2/US3 review (2026-10-25)

senior-code-reviewer over `3fd4283..1789478`: no high findings. Fixed:
- A trimmed range is encoded at 90% of the length that fits `hard`, in whole frames, and a
  trimmed full encode over `hard` retries shorter (the reported `fits_secs` keeps no margin).
- The retry writes its own files (`enc2.*`/`opt2.*`); the first encode is kept when the retry
  fails or ends over `hard` (`Found.attempt`).
- The MP4 is tagged BT.709 limited (frames via `setparams`, as the encoder takes the frames'
  tags over its own options); the MP4 hint samples at the MP4's own rate and CRF, over the
  trimmed range when trimmed, and runs before verify; the MP4's width floor is even; the CRF
  default is the target's; the search stage reports progress per sample.
- Accepted: the retry's progress fills only the rest of the current stage; x264 output is
  byte-identical per machine (T013), not across thread counts; the dry run predicts the search
  but not a retry (it never sees the full encode); the MP4 sample model scales linearly (its
  window-start I-frames make it conservative).

## US4/US5 review (2026-10-25)

senior-code-reviewer over `1789478..f4729ec`: no high findings. Fixed: a retried full encode is
kept only when it is smaller; smart framing keeps the analysed crop window at every width the
search tries; a cancelled analysis no longer switches the job to blur; CSV rows reject
`gif_budget=none` with `gif_hard_limit` and a GIF `gif_fps` above 50 (with the row number); the
CLI and manifest contracts say what the code does (`--gif --upscale` skips the upscale; `batch`
fills). Accepted: smart-framing fallbacks are warnings without an issue code; sample windows'
commands assume whole-second windows (under a frame of drift); a whole-range blur fallback still
writes its unused command file.

## US6 / SC-007 (2026-10-25, private Xvfb display :99, never the real display)

Debug desktop build (`tauri build --debug --no-bundle`), isolated config, state and cache, bundled
FFmpeg n9.0.2 and gifsicle. Opened `land.mp4` (Land6, 1280x720, 6 s) from the command line, then:
(1) **GIF for Reddit**: the view shows "Reddit post (limits unverified)", "aim 8.0 MB, at most
20.0 MB" and, within a few seconds, "≈ 2.1 MB · MP4 ≈ 465 kB"; the default target needs no
click; (2) **Export now**: "Finished · 1 succeeded", `gifs/land_reddit-post.gif` 2 092 390 bytes
(the estimate's 2.1 MB), playing in the result card. Two interactions after opening the video
(the bar is three). Found and fixed: the result card overflowed the panel, hiding its size line
(the caption now comes first and the picture is smaller).

## T079 quickstart on Linux (2026-10-25)

`quickstart.md` run in a scratch folder with the bundled FFmpeg n9.0.2, gifsicle 1.98 and gifski
1.34.0 (debug `rmedia`, Ubuntu 24.04, X11).

| Step | Result |
|---|---|
| US1 dry run | 720x405, 15 fps (≈ 6.67 cs, approximate note), 8 MB / 20 MB, estimate ~2.2 MB, MP4 hint ~496 kB, nothing written |
| US1 encode | 2,127,275 B; delays 30 × 6 cs and 60 × 7 cs (none under 2); 720,405,90 frames |
| `--fps 30` | note "3–4 cs frame delays"; 4.1 MB |
| `--start 1 --end 3` | 30 frames (2 s at 15 fps) |
| `--width 1600` | "kept source width 1280 (a GIF is never enlarged)" |
| `max-quality` | gifski; with `RMEDIA_GIFSKI=/nonexistent`: built-in + "gifski is not available …" |
| Cancel during the gifski pipe | exit 130 in 0.04 s after SIGINT; no `.part`/`.d`; no ffmpeg or gifski left |
| US2 `--budget 3MB` | 2,771,320 B, 8 encodes, fit soft, lowered lossy 30→90, dither bayer 4→5, colours 256→128, fps 15→10, width 720→… (ladder order) |
| Determinism | the rename-run output is byte-identical (`cmp`) |
| `--budget 150k --hard-limit 200k` | fails: "smallest 2.0 MB; about 0.9 s fits (use --allow-trim to trim)", nothing written |
| … `--allow-trim` | 139,290 B, 0.8 s |
| `--budget 10k --no-sample` | exit 3 at plan time: below the smallest possible GIF (65,536 B) |
| `RMEDIA_GIFSICLE=/nonexistent` | still fits: 3.0 MB, no lossy rung |
| US3 MP4 | `h264,video` only; moov before mdat; same width/fps as the GIF: 5.4× smaller (2,127,275 vs 390,930 B) |
| US4 | `--shape 4:5 --framing smart` plans 480x600; `--shape 9:16 --framing blur` encodes. Run on the synthetic `land6` (no face, so smart falls back to blur); the real-face pan is covered by `gif_us4.rs`'s moving-marker fixture |
| US5 | manifest: 3 GIFs, the second `reddit-comment`; no intermediate `.mp4`; `clip --gif --shape 1:1`, `clip -l --gif` (two files), `clip --gif --upscale` dry run: one job, "upscale skipped" |
| Doctor | gif and reddit mp4 yes, gifsicle 1.98 and gifski 1.34.0 found; with neither: both "no (not found …)", status still ready |
| US6 | covered by the SC-007 run above and the desktop/CLI GIF parity test (`bridge.rs`) |

Quickstart corrections made: the rename suffix is `_1` (not `-1`), and manifest videos use
`input` (not `path`).

## T080 final reviews (2026-10-25)

Range `e02b26a..HEAD`. Neither review found a High.

**senior-code-reviewer**

| # | Finding | Outcome |
|---|---|---|
| M1 | The MP4 refused `--fps` 51–60 (`plan_gif` used the GIF limit) | Fixed: `timing::validate_for(fps, mp4)`; CLI test `an_mp4_dry_run_shows_its_own_encode_and_takes_60_fps` |
| M2 | MP4 dry runs showed the palette/GIF argv and GIF colours/timing | Fixed: `PlannedJob::args`/`commands` return the one `mp4_args` encode; the human line reads "WxH, N fps, H.264, silent loop" (same test) |
| M3 | The GIF view showed the target's hard limit when only "Aim for" was set | Fixed: `resolveBudget` mirrors `BudgetSpec::resolve` (vitest) |
| L4 | `--allow-trim` could keep a one-frame GIF | Fixed: fewer than two frames is `BUDGET_UNREACHABLE` (unit test) |
| L7 | "Export now" ignored skipped and cancelled jobs | Fixed: the view says "Not exported: …" or "Export cancelled" |
| L5 | No integration test reaches the executor's retry path | Fixed 2026-10-26: `gif_us2::a_full_encode_that_misses_soft_is_retried_once` on the new `HiddenBusy10` fixture (flat inside the sample windows) |
| L6 | The plan-time free-space check uses `rough_bytes` (hard limit) and is not re-run after sampling | Fixed 2026-10-26: before the first full encode the job warns `DISK_FULL` from the sampled size (× 1.2, × 2 with gifsicle) |
| L8 | `GifTarget::width_for`, `GifTargetRegistry::label` used only by tests | Fixed 2026-10-26: the planner uses `width_for`; `--list-targets` uses `label` (`[user override]`) |
| L9 | The 50/60 fps rule lives in four places | Fixed 2026-10-26: the CLI and CSV call `timing::validate_for`, and their ranges use `MP4_MAX_FPS` |

**security-auditor**

| # | Finding | Outcome |
|---|---|---|
| M1 | `pin-gif-tools` used a predictable folder in the shared temp dir (pre-created by another user → writes through planted links) | Fixed: `tempfile` random 0700 folder, removed on drop |
| L1 | The gifski progress tailer re-read the whole file every 250 ms | Fixed: one handle, only new bytes are read |
| L4 | Engine stderr kept bidi/format characters | Fixed: `target::unsafe_char` in `tool_failed` and the pipe's stderr tail |
| L2 | `budget=none` jobs can use about 8 GiB of scratch each (4 GiB cap × 2 files), no free-space preflight | Fixed 2026-10-26: the size watchdog's limit is capped at the output folder's free space (less 64 MiB, split across the copies); passing it stops the job with `DISK_FULL` |
| L3 | The release gate checks staged file names, not the built bundles | Fixed 2026-10-26: `check-release-gate --scan DIR` checks built files and NSIS/WiX scripts; `release.yml` scans `target/release` and `dist` after packaging |

## Docs audit (docs-agent, 2026-10-26)

Report: `~/.claude/docs-agent/reports/documentation-audit.md`. Stale pre-003 pages, help output,
GIF names, the MP4 frame-rate cap per target and contract details were fixed in the docs. Two
gaps between `contracts/tauri-commands.md` and the code:
- **Settings had no GIF engine lines or gifsicle/gifski pickers** (the backend `pick_engine(kind)`
  existed). Built: **Settings › GIF export** (availability, each engine's version/path/source,
  **Choose …** and **Use automatic**, target-file problems, the "limits unverified" note).
- **The `partial` refining estimate** was never built; the contract now says the estimate is
  returned once, with progress on the channel.

## T078 release run and T079 Windows results (2026-10-26)

**T078.** `cargo xtask notices` gave no diff, `check-pins --online` passed, and
`check-release-gate` fails as expected while gifski is bundled and there is no root `LICENSE`.
`release.yml` was dispatched with `gifski=false` (owner approval 2026-10-26): run 40345344064 on
`723bbbe`, both `package` jobs succeeded and each printed `release gate: ok`. gifsicle 1.98 was
staged from the pinned `gif-tools-2026-10-25.40211502841` release and fetched with `--no-gifski`.

| Artifact | Size | Digest |
|---|---|---|
| `rmedia-x86_64-unknown-linux-gnu` | 636,294,351 B | `sha256:7224e520680eafc816e9aea14fb9866f03d8d3213b93516fc3b58cf654555d84` |
| `rmedia-x86_64-pc-windows-msvc` | 411,422,934 B | `sha256:f1fa7fdf7abcce0504dc5ed25d6b4cc0eedfe4c42870a72d59f820055e81bc36` |

The artifacts were not downloaded or installed here. Installing them on Windows hardware is an
owner check.

**T079 (Windows).** CI run 40345454414 on `83af317`, `test (windows-latest, bundled engine)`
with `RMEDIA_REQUIRE_GIF_TOOLS=1`: 482 passed, 0 failed, 15 ignored (benchmarks and GPU tests).
This covers:
- US1–US5;
- gifsicle, and the gifski pipe including its cancel test (`cancelling_the_gifski_pipe_is_fast_and_clean`);
- the gifski fallback;
- the release-gate test.

The release-mode benchmark step also ran the SC-009 bench. `desktop app (windows-latest)`
passed. The earlier note that a dead gifski on Windows might be blamed on ffmpeg was closed on
2026-10-27 (see below).

## Windows blame, release gate scan and Ctrl-C before the run (2026-10-27)

- **Dead gifski on Windows.** CI run 40445970256 on `a69089a`: `test (windows-latest, bundled
  engine)` passed, including `gif_gifski::a_gifski_that_dies_is_blamed_not_ffmpeg` and the unit
  test `windows_closed_pipe_lines_are_recognised_only_on_write_errors`. CI run 40443486456 on
  `475ccf4` (the T080 follow-ups) passed all 8 jobs.
- **Release run 40452999345** on `789d7e9` (`a69089a` + docs), gifski off, owner approval
  2026-10-27: both `package` jobs succeeded and printed `release gate: ok` before staging,
  after staging and on the built bundles (`check-release-gate --scan
  apps/desktop/src-tauri/target/release --scan dist`), the first real run of the scan. This is
  the build for the Windows hardware checks ([runbook](../../../docs/operations/windows-owner-checks.md)).

| Artifact | Size | Digest |
|---|---|---|
| `rmedia-x86_64-unknown-linux-gnu` | 636,305,532 B | `sha256:56cef6c5421c3e0a5bea9cddf20e91e58e68dbbd506d405d2dc8d7f8923b9823` |
| `rmedia-x86_64-pc-windows-msvc` | 411,470,509 B | `sha256:68075b99d243932f7ce4ed35b47ec39095c5024e90b3945fa4fa117427cd7e27` |

- **Flaky cancel test.** On the minimum-engine job of run 40445970256,
  `ctrl_c_cancels_with_exit_130_and_no_partial_files` signalled after a fixed 1.5 s, while the
  CLI was still planning and did not catch Ctrl-C yet: the process died by the signal (exit
  status none). The test now signals after the first `job_stage` event (`fae5d47`).
- **Ctrl-C before the run (found from that failure).** `clip`, `upscale`, `gif` and `batch`
  caught Ctrl-C only around single steps, so a press while locating the engine or planning
  killed the process without the `summary` line the NDJSON contract promises; and once tokio had
  listened for Ctrl-C (the scan of a folder input, the GPU check), a press between those steps
  was ignored. Fixed: these commands catch Ctrl-C from their start with one listener
  (`commands::catch_ctrl_c`); scanning, the GPU check, planning, samples and the run all watch
  it; a cancel before the run exits 130 with a `summary` line without jobs; a press after
  planning cancels every job. Test: `gif::ctrl_c_before_the_run_still_ends_with_a_summary`
  (a 2 s ffprobe wrapper; fails without the fix with exit status none).
- **Housekeeping found:** `tauri.conf.json` still says `0.2.0`, so the 0.3.0 installers are named
  `…0.2.0…`; bump it when 0.3.0 is released.
