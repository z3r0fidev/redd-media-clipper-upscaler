# Validation: 001 Video Clipping Base

**Date**: 2026-10-06 · **Machine**: Linux x86_64 (Ubuntu 24.04, 12 threads, SSD, no NVIDIA GPU) ·
**Engines**: distro FFmpeg 6.1.1 (minimum supported) and bundled FFmpeg n9.0.2 (shipped)

Legend: ✅ verified here · 🟡 verified indirectly / partially · ⏳ needs Windows CI or a manual check

## Automated suites (all green on both engines)

| Suite | Count |
|---|---|
| rmedia-core unit tests | 54 |
| core integration: US1 clip (incl. MPEG-TS, mislabelled TS, VFR, odd sizes) | 12 |
| core: US2 clip lists · US3 manifests (incl. SC-004) · US4 discovery · preview · cancel | 3 · 4 · 4 · 2 · 2 |
| argv snapshots | 8 |
| CLI contract tests: clip (incl. SIGINT, SIGKILL) · lists · scan · batch | 6 · 3 · 8 · 2 |
| desktop backend: media server security · bridge (incl. SC-008 parity, path policy) | 3 · 5 |
| desktop frontend (vitest) · svelte-check | 8 · 0 errors |
| benches (`--ignored`) | SC-001, SC-003 |

## Success criteria

| SC | Result | Evidence |
|---|---|---|
| SC-001 fast 1080p clip < 5 s | ✅ 0.3 s (4 s clip), 0.73 s (50 s clip) | `tests/bench.rs` |
| SC-002 accurate within one frame | ✅ first frame matches the requested frame (≈48 dB) for MP4, MPEG-TS, mislabelled TS | `tests/us1_clip.rs` |
| SC-003 10k files / 1k videos < 60 s | ✅ 6.3 s | `tests/bench.rs` |
| SC-004 100 clips, one failure isolated | ✅ 99 ok, 1 failed, exit 1 (7.3 s) | `tests/us3_batch.rs` |
| SC-005 first clip within 5 min of install | 🟡 Linux: `.deb` and AppImage install/extract, `rmedia doctor` reports the bundled engine, a clip succeeds with PATH emptied; GUI opens and plays. ⏳ Windows installer | manual run below |
| SC-006 malformed lines reported before processing | ✅ clip lists, CSV (by line), JSON (by line:column / JSON path); nothing written | parser + CLI tests |
| SC-007 cancel < 2 s, no partials, no orphans | ✅ Linux (core cancel, CLI SIGINT, SIGKILL + PDEATHSIG + journal purge). ⏳ Windows Job Objects in CI | `tests/cancel.rs`, `clip_single.rs` |
| SC-008 identical output CLI vs app | ✅ byte-identical (fast and accurate clips) | `apps/desktop/src-tauri/tests/bridge.rs` |

## Functional requirements

| FR | Status | Notes |
|---|---|---|
| 001–003 single clip, time grammar, validate-all-first | ✅ | |
| 004–005 clip lists with line errors | ✅ | comment rule refined (` # `) |
| 006–009 manifests (JSON + CSV), layering, relative paths, list × many videos | ✅ | `+` handling identical across formats |
| 010–012 discovery, skip rules, progress/cancel/export | ✅ | export via `--paths` (newline-safe) and the app's queue export |
| 013–014 fast/accurate, report actual span | ✅ | exact keyframe start for every container |
| 015 stream selection | ✅ | CLI flags, manifest options, per-source selects in the app |
| 016–018 naming, container, never touch sources / no overwrite | ✅ | default template `{name-or-range}` (spec amended) |
| 019–022 dry run, concurrency, progress/ETA, cancel, summary + exit codes | ✅ | |
| 023 CLI + app on Linux and Windows | 🟡 ⏳ Windows build/run via CI artifacts |
| 024 machine-readable output | ✅ | JSON contract tests |
| 025 app: preview, in/out, editable queue, import/export, drag-drop, progress/cancel | 🟡 | backend tested; UI rendered headless (direct + proxy playback). Dialogs, drag-drop and keyboard marking need a hands-on check |
| 026 diagnostics | ✅ | `rmedia doctor`, Settings › Video engine |
| 027 works out of the box, licence notices | 🟡 | Linux packages verified; notices + FFmpeg source offer bundled. ⏳ owner licence decision |

## Packaging run (Linux)

| Artifact | Size | Check |
|---|---|---|
| `rmedia_0.1.0_amd64.deb` | 140 MiB | `/usr/bin/{rmedia, rmedia-desktop, rmedia-ffmpeg, rmedia-ffprobe}`, `/usr/lib/rmedia/licenses/*`; depends on webkit2gtk + GStreamer plugins; extracted CLI finds the bundled engine with PATH emptied and cuts a clip |
| `rmedia_0.1.0_amd64.AppImage` | 284 MiB | includes GStreamer (WebKitGTK playback); app launched under Xvfb and played an MKV via its preview copy |

Found and fixed during this run: the AppImage could not play any video (GStreamer plugins were
not bundled — `bundleMediaFramework`), and a player effect re-ran loading in a loop when falling
back to still frames.

## Final review (T073)

The release-readiness review found, and this commit fixes: a Windows-only bug that would have
failed every clip at commit time (`sync_all` on a read-only handle — FlushFileBuffers needs write
access), the desktop CI job not staging sidecars, notices generated for the host platform only
(now the union of Linux + Windows), missing GStreamer packages on the release runner, `\\?\`
verbatim prefixes in user-visible Windows paths, and contract/README drift. Windows fixes are
type-checked here and first exercised by CI on `windows-latest`.

## Open items

- ⏳ Windows: installers, Job Object cancellation, long paths and cloud placeholders run only in CI
  (no Windows machine here).
- ⏳ Manual GUI pass on a desktop session: file dialogs, drag-and-drop, `I`/`O`/`Enter` marking,
  run + cancel from the UI (quickstart §4).
- Owner decisions: rmedia's own licence; confirm GPL FFmpeg redistribution; installer size
  (static FFmpeg ≈ 2 × 170 MB uncompressed) vs a trimmed custom build; code signing.
- GStreamer plugin-scanner warnings in the AppImage log are cosmetic (playback works).
