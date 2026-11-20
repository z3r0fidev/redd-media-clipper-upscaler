# rmedia documentation

rmedia cuts clips from videos on Linux and Windows, from the `rmedia` command line or the desktop
app. `rmedia upscale` (feature 002, 0.2.0) makes videos
phone-ready, and `rmedia gif` (feature 003, 0.3.0, tag `v0.3.0`) makes GIFs and silent MP4 loops
that fit Reddit. Smart cut (`--mode smart`, feature 004, 0.4.0, tag `v0.4.0`) makes frame-exact
clips that copy most of the video, and `--fit` (feature 005, 0.5.0) makes Reddit-ready MP4s with
sound that never exceed a size cap; see the [Roadmap](roadmap.md). Start with the page for your role.

## Users

| Page | What it covers |
|---|---|
| [Getting started](user/getting-started.md) | Install, check the engine, cut a first clip |
| [Desktop guide](user/desktop-guide.md) | Sources, player, marking, queue, run, export |
| [CLI guide](user/cli-guide.md) | `clip`, clip lists, folders, `batch`, `scan`, `upscale` (unreleased), `doctor` by task |
| [Troubleshooting](user/troubleshooting.md) | Common errors and what to do |

## Administrators

| Page | What it covers |
|---|---|
| [Installation](administration/installation.md) | Packages per platform, bundled FFmpeg, uninstall |
| [Configuration](administration/configuration.md) | Settings file, environment variables, folders |

## Reference

| Page | What it covers |
|---|---|
| [CLI reference](api/cli-reference.md) | Every command, option, default and exit code |
| [File formats](api/file-formats.md) | Times, clip lists, path lists, JSON/CSV manifests, name templates |
| [JSON output](api/json-output.md) | `--json` documents and NDJSON run events |
| [Desktop IPC](api/desktop-ipc.md) | Tauri commands, events, path policy, media server |
| [Glossary](reference/glossary.md) | Terms used across the project |
| [Smart cut codes](reference/smart-cut-codes.md) | The smart cut issue codes and the 12 fallback reasons |

## Developers and maintainers

| Page | What it covers |
|---|---|
| [Architecture overview](architecture/overview.md) | Core and surfaces, process model, key decisions |
| [Domains](architecture/domains.md) | Each module: responsibilities, types, invariants |
| [Security model](security/security-model.md) | Threats, controls, residual risks |
| [Development guide](developer/development-guide.md) | Setup, build, workflow, conventions |
| [Testing](developer/testing.md) | Test suites, fixtures, benchmarks |
| [Build and release](operations/build-and-release.md) | CI, packaging, pinned FFmpeg, releasing |
| [Windows owner checks](operations/windows-owner-checks.md) | Runbook: Windows hardware QA and SC-009 (gifsicle's quality cost) on real footage |
| [Windows checks over SSH](operations/windows-owner-checks-ssh.md) | The same runbook for an SSH session into the Windows machine, and the checks that need the screen |
| [Smart cut owner checks](operations/smart-cut-owner-checks.md) | Runbook for 0.4.0 on every platform: Linux, Windows and WebView2, Android, iPhone, a Reddit upload, real footage. Results: [Linux](operations/smart-cut-owner-checks-results-linux.md), [Windows](operations/smart-cut-owner-checks-results-windows.md), [Android](operations/smart-cut-owner-checks-results-android.md), [iPhone](operations/smart-cut-owner-checks-results-iphone.md), [Reddit](operations/smart-cut-owner-checks-results-reddit.md) |
| [Closing a session](operations/session-close.md) | Checklist for ending a development session: green CI, docs, numbering, Obsidian notes, push, handoff |
| [Roadmap](roadmap.md) | What is done, next and planned; owner validation items |
| [Release notes 0.1.0](releases/0.1.0.md) | What feature 001 delivers; known issues |
| [Release notes 0.2.0](releases/0.2.0.md) | Phone upscaling (CPU, GPU and AI tiers), smart reframe, `clip --upscale`, manifests and the desktop upscale view |
| [Release notes 0.3.0](releases/0.3.0.md) | GIF export for Reddit: `rmedia gif`, size budgets, the Reddit MP4, gifsicle and gifski, `clip --gif`, the desktop GIF view |
| [Release notes 0.4.0](releases/0.4.0.md) | Smart cut: `--mode smart`, fallbacks with reasons, strict mode, manifests, and the desktop Smart mode with keyframe markers |

Specifications, plans and contracts live in [`specs/`](../specs/); research notes are in
[`research/`](../research/).
