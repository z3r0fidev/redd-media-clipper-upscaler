# Closing a Development Session

This runbook is the checklist for ending a development session on rmedia. It leaves the
repository, the docs and the project notes consistent, so the next session can start from the
handoff without rediscovering state. It is run by the `script-session-closer` agent, or by
hand in the same order.

## 0. Wait for Green

Close only when everything on `main` is green and nothing is still running:

```bash
git status -sb                                   # clean, and level with origin/main
gh run list --branch main --limit 5              # the latest CI run passed
gh run list --workflow release.yml --limit 2     # any tag or dispatched build finished
gh pr list --state open                          # Dependabot PRs: merge only when green
```

If a run is in progress, wait for it and report the result. Don't close over a red run: fix it,
or record it as the first item of the handoff.

## 1. Review the Code (Optional, When Asked)

At a milestone, such as a tagged release, run the reviews as separate agents before closing.
Each is read-only, and its findings go into the roadmap's pending list:

| Agent | Scope |
|---|---|
| `senior-code-reviewer` | Architecture against the constitution, correctness, cancellation, test gaps, tech debt |
| `security-auditor` | Tauri IPC and capabilities, the media server, paths, subprocesses, workflows, supply chain |
| `product-strategy-advisor` | The next features, ranked, with a scope for `/speckit-specify` |

## 2. Update the Repository Docs

Bring every status document up to the current state. Each fact should live in one place, and
the others should link to it:

| File | What to update |
|---|---|
| `docs/roadmap.md` | The feature table, the dated update and the single **Pending** list (the source of truth for open work) |
| `CLAUDE.md` | The active feature, owner checks left, where things are |
| `specs/NNN-*/progress.md`, `tasks.md` | Task ticks; tasks are never renumbered once written |
| `specs/NNN-*/checklists/validation.md` | Results of owner checks and release runs, with run IDs and digests |
| `docs/releases/X.Y.Z.md` | `-dev` while unreleased; renamed and finalised when the tag is pushed |
| `README.md`, `docs/README.md` | The feature list and the docs index (add any new page) |

Use the docs-agent standards in `~/.claude/docs-agent` (`standards/`, `templates/`) for new or
rewritten pages.

### Numbering Must Stay Consistent

Check these before committing. A mismatch is fixed, not explained:

- **Features** are `001`, `002`, `003`… in `specs/NNN-name/`, and named the same in the roadmap,
  CLAUDE.md and the Obsidian notes. The next feature takes the next free number.
- **Versions** follow features: 001 → 0.1.0, 002 → 0.2.0, 003 → 0.3.0. The version in
  `Cargo.toml`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/package.json` and
  `tauri.conf.json` agree. Tags are `vX.Y.Z`, annotated, on the commit that was tested.
- **Tasks** are `T001`… per feature; **requirements and criteria** are `FR-NNN` and `SC-NNN` per
  spec. They are never reused or renumbered.
- **Owner-check steps** in the runbooks match their results sheets.

```bash
grep -rn "0\.[0-9]\.0-dev" README.md docs CLAUDE.md   # only an unreleased version may be -dev
git tag -l 'v*'                                        # one tag per released version
```

## 3. Update the Obsidian Notes

The vault folder is `rmedia/`. Use the MCP_DOCKER `obsidian_*` tools:

| Note | Update |
|---|---|
| `rmedia/Overview.md` | Status line, versions, active feature |
| `rmedia/Project_Management/ROADMAP.md` | Mirror of `docs/roadmap.md`: features done and planned |
| `rmedia/Project_Management/TODO.md` | Mirror of the roadmap's Pending list |
| `rmedia/Project_Management/Next Tasks.md` | Where the next session starts, in order |
| `rmedia/Project_Management/Session Log YYYY-MM-DD.md` | A new note for this session: what was done, commits, decisions, open items |
| `rmedia/DevOps/CI-CD and Packaging.md` | Workflows, release runs, tags |
| `rmedia/Architecture/`, `Product/`, `Security/`, `Desktop/` | Only when the session changed them |

Known quirks:

- If `obsidian_patch_content` or `obsidian_append_content` fails, delete the note and write it
  again whole.
- Pass frontmatter values unquoted: a quoted value is stored with its quotes.
- If the REST endpoint refuses connections, Obsidian is usually not running. Start it, then run
  `/mcp`.

The TODO, ROADMAP and Next Tasks notes must agree with `docs/roadmap.md`.

## 4. Commit and Push

Use Conventional Commits, for example `docs: close the session (YYYY-MM-DD)`. When SSH has no
agent, push over HTTPS with gh's credential helper:

```bash
git -c credential.helper= -c credential.helper='!gh auth git-credential' \
  push -q https://github.com/z3r0fidev/redd-media-clipper-upscaler.git main
git update-ref refs/remotes/origin/main HEAD
```

Then wait for CI on the pushed commit and confirm it is green.

## 5. Leave Nothing Running

```bash
pgrep -a 'ffmpeg|rmedia|cargo|vite|tauri|Xvfb'    # expect nothing from this session
```

Stop leftovers by PID. Never use `pkill -f` with a pattern that also matches your own
command line. Stop background shells and agents. Remove scratch files outside the repo.

## 6. Hand Off

End with a short report:

- what was done and the commits;
- the CI and release state;
- open owner actions (release publishing, signing, owner checks);
- **where the next session starts**: the first item of `Next Tasks.md`, which matches the
  roadmap.

## Related

[Roadmap](../roadmap.md) · [Build and Release](build-and-release.md) ·
[Development Guide](../developer/development-guide.md)
