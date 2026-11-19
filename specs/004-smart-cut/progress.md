# Progress: 004 Smart Cut

`tasks.md` is the source of truth: a task is done when its box is `[X]`. This page tracks phases,
checkpoint reviews and what is still open. Review details are in `checklists/validation.md`.

Recount the boxes with:
```bash
awk '/^## Phase/{p=$0} /^- \[[ X]\] T[0-9]+/{t[p]++; if($0~/^- \[X\]/)d[p]++} END{for(k in t) printf "%-60s %3d/%3d\n",k,d[k],t[k]}' specs/004-smart-cut/tasks.md | sort
```

## Phases

| Phase | Tasks | Done | Status | Reviews |
|---|---|---|---|---|
| 1 Setup | T001–T004 | 4/4 | done 2026-11-15 | — |
| 2 Foundational (types, keyframe index, spikes S1–S3) | T005–T014 | 10/10 | done 2026-11-15 | self-review |
| 3 US1 MVP: `--mode smart` | T015–T030 | 16/16 | done 2026-11-15 | self-review |
| 4 US2 Fallbacks and strict mode | T031–T039 | 9/9 | done 2026-11-16 | self-review |
| 5 US3 Dry run | T040–T042 | 3/3 | done 2026-11-16 | self-review |
| 6 US4 Manifests and parity | T043–T046 | 4/4 | done 2026-11-16 | self-review |
| 7 US5 Desktop Smart mode and keyframe markers | T047–T053 | 7/7 | done 2026-11-16 | self-review; Xvfb checkpoint |
| 8 Polish | T054–T059 | 6/6 | done 2026-11-16 | self-review, senior-code-reviewer and security-auditor: no high; 3 mediums fixed |
| **Total** | | **59/59** | | |

## Open

- T059 is done: both agent passes ran over all 004 changes at the end (the earlier checkpoints
  were self-reviewed). Lows left: roadmap item 27.
- The release: version 0.4.0 (`354ac15`) and tag `v0.4.0` on `092171f` (2026-11-17), built by
  `release.yml` run 44075993817; published as [v0.4.0](https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/tag/v0.4.0) on 2026-11-18 after the owner checks.
- Owner checks, all platforms, passed on 2026-11-18: `docs/operations/smart-cut-owner-checks.md`,
  results in `docs/operations/smart-cut-owner-checks-results-{linux,windows,android,iphone,reddit}.md`
  (real footage: owner report).

## Measured

SC-005 26% (reference machine), 25% (Linux and Windows CI), 31% locally with other work running;
SC-007 markers within 1.5 s on screen, the index 126–261 ms locally and 981 ms on the Windows CI
runner. Full results: `checklists/validation.md`.
