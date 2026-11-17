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
| 8 Polish | T054–T059 | 5/6 | T059 open | self-review, no high or medium findings |
| **Total** | | **58/59** | | |

## Open

- T059: the senior-code-reviewer and security-auditor passes over all 004 changes. Every
  checkpoint so far was self-reviewed only, because agents run only when the owner asks.
- The release: the version bump to 0.4.0 and the tag, following
  `docs/operations/build-and-release.md` › Cut a Release.
- Owner checks (spike S1): smart mp4 playback in WebView2, on an Android phone and an iPhone,
  and a Reddit upload. The desktop app's smart mode has not been run by hand on Windows.

## Measured

SC-005 26% (reference machine), 25% (Linux and Windows CI), 31% locally with other work running;
SC-007 markers within 1.5 s on screen, the index 126–261 ms locally and 981 ms on the Windows CI
runner. Full results: `checklists/validation.md`.
