# Progress: 005 Budgeted Reddit Video

`tasks.md` is the source of truth: a task is done when its box is `[X]`. This page tracks phases,
reviews and what is still open. Test and SC results are in `checklists/validation.md`.

Recount the boxes with:
```bash
awk '/^## Phase/{p=$0} /^- \[[ X]\] T[0-9]+/{t[p]++; if($0~/^- \[X\]/)d[p]++} END{for(k in t) printf "%-60s %3d/%3d\n",k,d[k],t[k]}' specs/005-budgeted-reddit-video/tasks.md | sort
```

## Phases

| Phase | Tasks | Status | Reviews |
|---|---|---|---|
| 1 Setup, 2 Foundational | T001–T010 | done 2026-11-19 | self-review |
| 3 US1 MVP: `clip --fit` | T011–T022 | done 2026-11-19 | self-review |
| 4 US2 Length and resolution limits | T023–T029 | done 2026-11-19 | self-review |
| 5 US3 Know the size before running | T030–T033 | done 2026-11-19 | self-review |
| 6 US4 Device shapes and batches | T034–T038 | done 2026-11-19 | self-review |
| 7 US5 Desktop | T039–T043 | done 2026-11-20 | self-review; Xvfb check |
| 8 Polish | T044–T047 | done 2026-11-20 | T046 below |

## T046 reviews (2026-11-20)

Scope: `core/fit/` (`mod.rs`, `plan.rs`, `target.rs`), `core/exec/fit.rs`, the planner's fit
code, `plan/ffargs.rs` (`fit_args`, `fit_tail`, the upscale and AI pass args), `plan/model.rs`,
the CLI's fit flags and rendering, and the desktop's `ops/fit.rs`. This pass was a self-review;
the agent passes follow in the next section.

### Code review

| # | Severity | Finding | Status |
|---|---|---|---|
| 1 | medium | The dry run showed the wrong commands for a fit job: `PlannedJob::args()` and `commands()` ignored the fit, so `--dry-run --json` `argv` and `-v` showed an accurate-mode encode (`-crf 18`, `-b:a 192k`) that never runs (Principle II). Upscale fit jobs had the same gap through `upscale_args`. | fixed: `commands()` lists every pass, `args()` is the one that writes the output; the JSON adds `pipeline` and `cwd` for fit jobs. Tests `fit_us1.rs` `the_plan_shows_the_fit_passes`, `cli/tests/fit.rs` |
| 2 | medium | `{target}` in a name template was refused for fit jobs (`unknown placeholder`), though contracts/cli.md allows it. | fixed: the planner passes the video target id. Test `cli/tests/fit.rs` |
| 3 | low | `RANGE_TOO_LONG` rounded both lengths to whole seconds, so a range up to 0.5 s over read "the range (15:00) is longer than … allows (15:00)". | fixed: tenths when the two would read the same (`clock_pair`); unit test |
| 4 | low | A quality-mode encode that overshot re-entered a `pass2` stage (index 2 of 2, the same as `verify`) that the job never planned. | fixed: the fallback restarts `encode`. Test `fit_retry.rs` `a_quality_overshoot_retries_within_its_encode_stage` |
| 5 | low | A trimmed clip keeps the default name of the range that was asked for (`…_00-00-25.000.mp4` for a 10 s output), as GIF trims do (003). | kept and documented (CLI reference, CLI guide); roadmap |

Checked and fine: the planning order (length, size, quality or budget, ladder, audio, trim or
refuse) against research R2–R5; the retry bound (at most 4 encodes, an early stop below
`min_bpp`); the size measured on the `.part` before the commit; quality mode's VBV excess in
`predicted_bytes`; the `test-support` overshoot hook only in test builds (a dev-dependency
feature).

### Security review

| Check | Result |
|---|---|
| User target files read capped | `data_files::load`: regular files only, ≤ 64 KiB, ≤ 64 files (at review time the listing stopped at 256; since the agent review, the first 64 by name), duplicates reported |
| Every target field validated | `VideoTarget::validate`: id `[a-z0-9-]{1,40}`, caps 64 KiB–4 GiB, durations, sizes, fps, CRF, bpp and audio ranges; problem text sanitised and cut to 300 characters |
| Overrides | `parse_size` within 4 GiB; an override may only lower a limit (exit 2 on the CLI, exit 3 in a manifest) |
| Pass files | `-passlogfile fit`, a fixed relative name; every pass (and the AI pipeline) runs with the job's scratch folder as working directory; the folder is journaled and removed |
| No shell | every pass through `exec::process`, argv only, `file:` paths |
| Desktop | `fit_plan` requires an opened path, plans only (no encode, nothing written), cancels the older call; `list_fit_targets` returns the validated listing |

No high or medium security findings.

## Agent reviews (2026-11-20)

The senior-code-reviewer and security-auditor agents reviewed `092171f..c7fbb54` (read-only).
No high findings. Each finding was checked against the code before it was fixed.

| # | From | Severity | Finding | Status |
|---|---|---|---|---|
| A1 | code | medium | A trimmed clip could last longer than the length limit: the limit was not a whole number of frames, and the last frame ended past it (29.97 fps, a 10 s limit gave 10.010 s; a range exactly at the limit did the same). The size was measured, the length was not. | fixed: the limit is whole output frames (`plan_fit`: 299 frames, 9.977 s); the cut ends 1 ms early so a frame at the end is not kept. Tests `plan.rs` `the_length_limit_is_whole_frames`, `fit_us2.rs` `a_length_limit_holds_at_a_fractional_frame_rate` (new fixture `FitNtsc`); T024's check no longer allows a frame over |
| A2 | code | medium | A plan trimmed or lowered to fit sits at the target's least bitrate (`min_bpp`), so its first retry fell below it and the job failed `FIT_FAILED` without retrying. | fixed: a retry may go down to 90% of the least bitrate (`RETRY_FLOOR`); further down is a runaway overshoot and still fails. Tests `exec/fit.rs` `a_plan_at_the_least_bitrate_may_still_retry`; `fit_retry.rs`'s early-stop test now plans 5% above the minimum |
| A3 | security | low | A user target with `long_side` 4294967295 overflowed `long_side * 9 / 16` before its range check was read: debug builds panicked while loading targets (release builds wrapped and still refused the file). | fixed: computed in `u64`. Test: the case in `every_rule_refuses_its_bad_value_with_a_reason` |
| A4 | code | low | A picture too small for 1 kb/s planned `-b:v 0k`. | fixed: at least 1 kb/s. Test in `the_length_limit_is_whole_frames` |
| A5 | code | low | `--fit-max-size 1` read "cannot fit 0 kB". | fixed: sizes below 1 kB read in bytes (`human_bytes`). Test `sizes_below_a_kilobyte_are_bytes` |
| A6 | code | low | The surround warning said the extra channels were "dropped"; they are mixed down (`-ac 2`). | fixed: "mixed down to stereo" |
| A7 | security | info | A user target file's name is shown unfiltered (its problem text is filtered). The user names their own files. | fixed: a file whose name has control or text-direction characters is not read; the problem names the folder and shows the first three cleaned names by name. Test `data_files` `files_with_unsafe_names_are_skipped_and_never_printed_raw` |
| A8 | security | info | CSV exports do not neutralise cells starting `= + - @`. The fit columns hold only ids and numbers; the path columns predate 005. | fixed: such cells are written with a leading `'`, which the reader drops again (round trip kept). Test `manifest_csv` `formula_cells_are_guarded_and_round_trip` |
| A9 | security | info | With more than 256 files in the folder, which 64 are read depends on the listing order; the rest are reported. | fixed: the whole listing is walked and the first 64 names kept. Test `the_first_64_by_name_are_read_however_many_there_are`. Device presets (002), which had no file cap, now load through the same code (`at_most_64_preset_files_are_read`) |

Checked and fine by both: the planning order and its math, never committing over the cap, the
retry bound, cancellation and scratch cleanup, `-threads 1` in every pass, argv-only spawning
with no user text in options or filters, target file reading (`O_NOFOLLOW`, size and count
caps, every field validated), overrides that may only lower, and the desktop's `fit_plan`
(opened paths only, plans without encoding).

## Open

- All 47 tasks are done. Version 0.5.0 is set; `docs/releases/0.5.0.md` is a draft until the tag.
- SC-008 passed 2026-11-23 (owner): `reddit-video` is verified (version 2); Reddit's limits are
  1 GB and 15 minutes, and video posts are not allowed in 18+/NSFW/adult or private communities.
  Next: tag `v0.5.0` and publish the release.
