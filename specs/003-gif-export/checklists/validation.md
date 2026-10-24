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
