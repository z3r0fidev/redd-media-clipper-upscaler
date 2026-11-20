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
004's final pass also ran the senior-code-reviewer and security-auditor agents, which are
offered for 005 too.

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
| User target files read capped | `data_files::load`: regular files only, ≤ 64 KiB, ≤ 64 files (the listing itself stops at 256), duplicates reported |
| Every target field validated | `VideoTarget::validate`: id `[a-z0-9-]{1,40}`, caps 64 KiB–4 GiB, durations, sizes, fps, CRF, bpp and audio ranges; problem text sanitised and cut to 300 characters |
| Overrides | `parse_size` within 4 GiB; an override may only lower a limit (exit 2 on the CLI, exit 3 in a manifest) |
| Pass files | `-passlogfile fit`, a fixed relative name; every pass (and the AI pipeline) runs with the job's scratch folder as working directory; the folder is journaled and removed |
| No shell | every pass through `exec::process`, argv only, `file:` paths |
| Desktop | `fit_plan` requires an opened path, plans only (no encode, nothing written), cancels the older call; `list_fit_targets` returns the validated listing |

No high or medium security findings.

## Open

- All 47 tasks are done. Version 0.5.0 is set; `docs/releases/0.5.0.md` is a draft until the tag.
- SC-008 is an owner check: a real Reddit upload with sound of a `reddit-video` output and a
  `reddit-916` output (`docs/operations/fit-owner-checks.md`); the result sets `verified` in
  `reddit-video.json`. Then tag `v0.5.0`.
- Optional: the senior-code-reviewer and security-auditor agent passes, as 004's T059 had.
