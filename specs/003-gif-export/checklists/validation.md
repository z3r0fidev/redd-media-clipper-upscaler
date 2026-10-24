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
