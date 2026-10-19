# Progress: 002 Mobile Upscaling & Smart Reframe

`tasks.md` is the source of truth: a task is done when its box is `[X]`. This page tracks phases,
checkpoint reviews and commits. Update it at every checkpoint.

Recount the boxes with:
```bash
awk '/^## Phase/{p=$0} /^- \[[ X]\] T[0-9]+/{t[p]++; if($0~/^- \[X\]/)d[p]++} END{for(k in t) printf "%-60s %3d/%3d\n",k,d[k],t[k]}' specs/002-mobile-upscale/tasks.md | sort
```

## Phases

| Phase | Tasks | Done | Status | senior-code-reviewer | security-auditor | Commit |
|---|---|---|---|---|---|---|
| 1 Setup | T001–T006 | 6/6 | done | — | — | |
| 2 Foundational (A1–A5, probe, fixtures, presets) | T007–T021 | 15/15 | done | with US1 | with US1 | 76f2339, d7ad0f9, d2c0804, + |
| 3 US1 MVP: CPU tier, `rmedia upscale` | T022–T041 | 20/20 | done | done (1 high, 6 medium fixed) | done (1 medium, 5 low addressed) | see log |
| 4 US2 Smart reframe | T042–T061 | 20/20 | done | done (1 high, 6 medium fixed) | done (2 low, 3 info fixed) | 363b860, 68bd4b6, 4be74aa, + |
| 5 US3 GPU tier, capability | T062–T073 | 12/12 | done | done (3 high, 6 medium fixed) | done (3 medium, 6 low addressed) | (US3 commit) |
| 6 US4 Batch, `clip --upscale` | T074–T081 | 0/8 | ▶ next | required | required (discovery, paths) | |
| 7 US5 Desktop view | T082–T092 | 0/11 | pending | required | required (IPC surface) | |
| 8 US6 AI worker | T093–T105 | 0/13 | pending | required | required (FFI, pipes) | |
| 9 Polish | T106–T112 | 0/7 | pending | final pass | — | |
| **Total** | | **73/112** | | | | |

Status values: `pending`, `▶ next`, `in progress`, `checkpoint`, `done`.

## Gates at every checkpoint
- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace -- -D warnings`
- [ ] `cargo test --workspace` (bundled engine, `RMEDIA_REQUIRE_FFMPEG=1`)
- [ ] 001 baseline test count still passing (T001)
- [ ] Bindings and notices up to date (`cargo xtask gen-bindings`, `cargo xtask notices`)
- [ ] quickstart.md section for the story passes
- [ ] Reviews done and findings fixed

## Owner validation (cannot be automated)
- [ ] Two-phone gallery test (iPhone + Android): no bars, correct colours (SC-001)
- [ ] Reddit test upload of `reddit-916` and `reddit-feed-45`
- [ ] +3 VMAF over bicubic on real low-quality footage (SC-002 owner check)
- [ ] Discrete-GPU run of the AI tier tests (T098, SC-002 AI, SC-009)

## Decisions
- 2026-10-14 (owner): the automated SC-002 CPU bar is +1.5 VMAF on synthetic fixtures (measured +2.78 at 480p, +2.89 at 360p); +3 on real footage is an owner check.
- 2026-10-19 (implementation, for owner review): SC-003/FR-007 amended. `auto` takes the GPU only when its self-test is at least as fast as the CPU's; on the reference iGPU it measured slower (x265 dominates), so auto stays on the CPU there. The GPU tier matches CPU quality (57.66 vs 57.63 VMAF).

Project-wide roadmap: `docs/roadmap.md` (must agree with this page).

## Log

| Date | Phase | What happened | Commit |
|---|---|---|---|
| 2026-10-14 | Design | tasks.md generated (111), analysis + senior review applied (112 tasks) | 6caa58d, a76f31e |
| 2026-10-14 | Setup + Foundational | setup, contract pin, A1–A5, fixtures/probe/presets | d957471, 76f2339, d7ad0f9, d2c0804, 77690c9 |
| 2026-10-14 | US1 | `rmedia upscale` CPU tier; SC-002 bar set to +1.5 on synthetic fixtures (owner); reviews fixed | 2bfe3f0 |
| 2026-10-15 | Session close | No code changes. Docs brought up to date (`docs/` CLI reference, JSON output, configuration, architecture, user guides, release notes 0.2.0-dev, new `docs/roadmap.md`); `cargo test --workspace` re-run: 182 passed, 0 failed, 3 ignored. Next: US2 (T042–T061), plan-mode pass first. No git remote and no `main` branch yet (owner to decide). | see git log |
| 2026-10-16 | US2 | Smart reframe: YuNet under tract, cuts, smoothed crop path, sendcmd graph, cache, `--crop-path`. SC-004 100% inside; SC-005 analysis 10.7 s vs 79 s render. Reviews fixed. | 3433696 |
| 2026-10-19 | US3 | GPU tier (libplacebo, VMAF parity), capability record and self-tests, auto/forced selection with speed guard, one CPU fallback with demotion, `doctor` tiers and `--recheck`, desktop Check again. Reviews fixed (async probes via `exec::process`, inconclusive checks not stored, demote after CPU success). 244 tests pass (6 ignored). | (this commit) |
