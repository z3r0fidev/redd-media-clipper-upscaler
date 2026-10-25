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
  blame ffmpeg (unverified without a Windows run).
- The 300 s pipe stall counts from spawn, before ffmpeg's first progress block.
- Over the hard limit, a GIF fails verification until US2's budget search lowers the settings.
- `GIFSKI_LOCK_UPDATES` is listed in both gif-tools.yml and xtask; the pin's lock comparison
  catches any drift.
