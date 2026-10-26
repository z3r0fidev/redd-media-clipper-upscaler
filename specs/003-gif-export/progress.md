# Progress: 003 GIF Export for Reddit

`tasks.md` is the source of truth: a task is done when its box is `[X]`. This page tracks phases,
checkpoint reviews and what is still open. Review details are in `checklists/validation.md`.

Recount the boxes with:
```bash
awk '/^## Phase/{p=$0} /^- \[[ X]\] T[0-9]+/{t[p]++; if($0~/^- \[X\]/)d[p]++} END{for(k in t) printf "%-60s %3d/%3d\n",k,d[k],t[k]}' specs/003-gif-export/tasks.md | sort
```

## Phases

| Phase | Tasks | Done | Status | Reviews |
|---|---|---|---|---|
| 1 Setup | T001–T005 | 5/5 | done | — |
| 2 Foundational (types, targets, timing, engines, B1–B4, gif-tools supply chain) | T006–T032 | 27/27 | done | security review 2026-10-24 |
| 3 US1 MVP: `rmedia gif`, gifski | T033–T044 | 12/12 | done | both, US1 checkpoint |
| 4 US2 Budget search, gifsicle | T045–T053 | 9/9 | done | both (US2/US3 review) |
| 5 US3 Reddit MP4 | T054–T056 | 3/3 | done | with US2 |
| 6 US4 Shapes and smart framing | T057–T058 | 2/2 | done | both (US4/US5 review) |
| 7 US5 Manifests, `clip --gif`, `batch --gif` | T059–T064 | 6/6 | done | with US4 |
| 8 US6 Desktop GIF view, SC-007 | T065–T072 | 8/8 | done | security review of the desktop commands |
| 9 Polish | T073–T080 | 8/8 | done | T080 final: no High; mediums fixed |
| **Total** | | **80/80** | | |

## Open

All 80 tasks are done. T078: `release.yml` run 40345344064 (gifski off) packaged Linux and
Windows. T079: Windows CI 482 passed, 0 failed (run 40345454414). Both are recorded in `checklists/validation.md`.

- Owner decision: the app licence (gates bundling gifski). Decided 2026-10-26: SC-009's automated
  bar is 5% (15% on real footage is an owner check); the stale release `gif-tools-2026-10-25` was
  deleted; `release.yml` dispatched with gifski off (run 40345344064).
- Owner checks: Reddit's GIF limits in its Help Center, a real Reddit upload of a post and a
  comment GIF, Windows hardware.
- Follow-ups from T080 (low): an integration test of the executor's retry path, the free-space
  check after sampling, two test-only target helpers, one shared fps-limit function, a post-build
  bundle scan for the release gate.

## Measured

SC-002 20/20 · SC-003 23.8 s (bar 30 s) · SC-004 9/10 within 25% · SC-005 98.32 vs 66.23 VMAF ·
SC-006 6.1× / 18.2× · SC-007 2 interactions, estimate matched · SC-008 cancel clean ·
SC-009 6.4% (bar 5%; 15% real-footage owner check) · SC-010 99.98 vs 98.80, 84.63 vs 83.50 VMAF.
