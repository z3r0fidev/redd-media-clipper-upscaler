# rmedia Testing

## Overview

This reference lists every rmedia test suite: what it proves, how to run it, and what it needs. It
is for engineers who change rmedia and reviewers who assess release readiness.

Sources: the test files listed below, `crates/rmedia-testkit/src/lib.rs`,
`.github/workflows/ci.yml`, and `specs/001-video-clipping/checklists/validation.md` (validation run
of 2026-10-06).

For build setup, see the [Development Guide](development-guide.md).

## Testing Principles

From the constitution (Principle III, non-negotiable) and `CLAUDE.md`:

- Tests are written before the implementation and must fail first.
- Media behavior is verified against **real video generated at test time** with FFmpeg `lavfi`
  sources. Outputs are checked with `ffprobe` (duration within tolerance, resolution, codec, stream
  presence), not only by exit code.
- Parsers (timestamps, clip lists, manifests) have table-driven unit tests that include malformed
  input.
- **No binary fixtures are committed.** The committed `crates/rmedia-core/tests/data/probe/*.json`
  files are ffprobe JSON text used by the argv snapshot tests.

## Run the Tests

```bash
cargo xtask fetch-tools     # optional: test against the shipped engine
export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
       RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu \
       RMEDIA_REQUIRE_FFMPEG=1

cargo test --workspace                                   # core unit + integration + CLI
cargo test --workspace --release -- --ignored bench      # timing benches
cargo build -p rmedia-cli
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # desktop backend
cd apps/desktop && npm ci && npm test && npm run check   # vitest + svelte-check
```

Run the benches in release mode. Their thresholds assume optimized builds.

### Test environment variables

| Variable | Used by | Effect |
|---|---|---|
| `RMEDIA_FFMPEG`, `RMEDIA_FFPROBE` | testkit and the code under test | FFmpeg used to generate fixtures and to run the tests. Defaults to `ffmpeg` / `ffprobe` on PATH. |
| `RMEDIA_REQUIRE_FFMPEG` | testkit | `1` makes media tests panic when FFmpeg is missing, instead of skipping. CI sets it. |
| `RMEDIA_STATE_DIR` | CLI tests | Each CLI test points the crash journal at a temporary directory. |
| `RMEDIA_CONFIG_DIR` | desktop `bridge.rs` | Set once per test process to a temp folder so tests never touch the developer's real `settings.json`. |
| `RMEDIA_CLI_BIN` | desktop `bridge.rs` | Path to the `rmedia` binary for the CLI parity test. Default: `target/debug/rmedia`. |

## Test Kit (`crates/rmedia-testkit`)

`fixture(Fixture::…)` generates a media file once into `target/rmedia-fixtures/<hash>/` (written to
a temp file and renamed, so parallel test processes are safe). The hash covers the file name and the
FFmpeg arguments, so changing a recipe produces a new fixture. `require_ffmpeg!()` skips a test when
FFmpeg is unavailable (or panics under `RMEDIA_REQUIRE_FFMPEG=1`).

| Fixture | Content | Exercises |
|---|---|---|
| `Gop50Mp4` | 320x240, 25 fps, 60 s, keyframe every 2.000 s, B-frames, sine AAC | Keyframe snapping, frame accuracy |
| `Hd60Mp4` | 1920x1080, 30 fps, 60 s, keyframe every 2 s | SC-001 timing |
| `MultiMkv` | MKV, 2 audio tracks + SRT subtitle, 20 s | Stream selection, subtitle conversion |
| `SmallWebm` | VP9/Opus WebM, 10 s | WebM handling |
| `NoAudioMp4` | Video only, 10 s | Missing audio streams |
| `VfrMp4` | Variable frame rate MP4, about 20 s | VFR timing |
| `OffsetTs` | MPEG-TS starting at 10 s | Timestamp offsets |
| `CoverArtM4a` | Audio with cover art | Discovery skips non-videos |
| `MislabelledTs` | MPEG-TS named `.mp4` | Approximate seeking despite the extension |
| `OddDimsMp4` | 321x241 H.264, yuv444p | Even-size requirement of 4:2:0 encoders |
| `FakeMp4` | Text file named `.mp4` | Discovery rejects fakes |
| `PhotoMp4` | JPEG named `.mp4` | Discovery rejects still images |

Helpers: `probe_json`, `duration`, `stream_types`, `sha256` (source-unchanged checks), and
`first_frame_psnr` (frame-accuracy check that decodes the reference sequentially without seeking,
so it does not share seek imprecision with the code under test).

## Suite Inventory

Counts are from the source files. The core unit test count is from `cargo test -p rmedia-core --lib`
(54 passed on 2026-10-06).

### Core unit tests (`crates/rmedia-core/src`, 54 tests)

In-module `#[cfg(test)]` tests. They need no FFmpeg unless noted.

| Module | Proves |
|---|---|
| `time.rs` | Timestamp and duration grammar (`90`, `1:30`, `00:01:30.500`, `1m30s`, `+15`) |
| `input/cliplist.rs`, `input/manifest_json.rs`, `input/manifest_csv.rs`, `input/pathlist.rs` | Parsers, including malformed input reported by line, column, or JSON path |
| `request.rs` | Option layering (`PartialOptions::apply` / `diff`) |
| `plan/naming.rs` | Output name templates and sanitizing |
| `tools.rs` | Version parsing (6.0 minimum, git builds accepted, 4.x and 5.x rejected); explicit path must exist; bundled beats PATH |
| `doctor.rs` | Encoder list parsing |
| `probe.rs`, `discover.rs` | Probe interpretation, discovery rules |
| `exec/progress.rs` | FFmpeg `-progress` parsing and throttling |
| `exec/commit.rs` | No-clobber commit |
| `exec/journal.rs` | A live (locked) journal is never purged and cleans up on drop; a stale journal deletes only its own `.part` files; symlinked entries are never followed |
| `settings.rs` | Settings round trip; `{}` equals defaults; unknown fields rejected |
| `paths.rs`, `exec/clip.rs` | Path handling, clip execution helpers |

### Core integration tests (`crates/rmedia-core/tests`)

All use real generated media and verify outputs with ffprobe.

| File | Tests | Proves |
|---|---|---|
| `us1_clip.rs` | 12 | US1: accurate clips are frame-exact and the source is unchanged (SC-002); fast clips snap to the keyframe and report it; duration form and end clamping; invalid requests rejected before processing; collision policies; timestamp offsets and VFR; exact keyframe start for MPEG-TS; stream and container selection; unwritable output fails cleanly; mislabelled TS stays frame-exact; odd dimensions encode in accurate mode |
| `us2_cliplist.rs` | 3 | US2: three clips named or numbered; collisions inside one plan; one failure does not stop the others |
| `us3_batch.rs` | 4 | US3: relative paths across 3 videos × 2 clips; a missing video is reported before anything runs; equivalent JSON and CSV give identical plans; SC-004 (100 clips, one isolated failure) |
| `us4_discover.rs` | 4 | US4: exactly the real videos are found; include/exclude/depth filters; file roots and duplicates; scan cancellation |
| `cancel.rs` | 2 | SC-007: cancelling a run is fast (under 2 s), leaves no partial files and no child engine processes; cancelling one job keeps the others |
| `preview.rs` | 2 | Preview frames, proxies, and thumbnails; time offset equals the first video frame |
| `snapshots_ffargs.rs` | 8 | Exact FFmpeg argv for each mode × container × stream layout, from committed ffprobe JSON. Paths are normalized so snapshots are platform-independent (`insta`) |
| `bench.rs` | 2 (ignored) | SC-001: fast 1080p clip under 5 s; SC-003: scan of 10,000 files with 1,000 videos under 60 s |

### CLI contract tests (`crates/rmedia-cli/tests`)

These run the real `rmedia` binary (`assert_cmd`) against `specs/001-video-clipping/contracts/cli.md`.

| File | Tests | Proves |
|---|---|---|
| `clip_single.rs` | 7 (platform-gated) | Odd paths and `--json` output; `--dry-run` writes nothing; exit codes; a second run skips the existing output; **Linux:** SIGKILL leaves no orphans and the next run purges the partials, SIGINT cancels gracefully; **Windows:** long paths |
| `clip_list.rs` | 3 | A malformed list reports its lines and writes nothing; a list applies to every input; `-l` conflicts with `-s` |
| `scan.rs` | 8 | Scan JSON matches the contract; `--paths` feeds `clip --inputs-from -`; clipping a folder twice never clips its own outputs; scan errors; an output folder with glob characters is still excluded; the output folder cannot be the scanned folder; hostile file names; the same video reached twice is clipped once |
| `batch.rs` | 2 | A CSV batch runs, including `--continue-on-missing`; dry run with CLI defaults |

### Desktop backend tests (`apps/desktop/src-tauri/tests`)

These run without a webview. CI runs them on Linux only.

| File | Tests | Proves |
|---|---|---|
| `bridge.rs` | 6 | Only user-chosen paths are usable; preview chooses direct playback or a proxy; **SC-008:** GUI and CLI outputs are byte-identical (fast and accurate); the page cannot set engine paths through settings; outputs, lists, and manifests are confined; a JSON export carries the Settings defaults, an unpicked output folder is refused, and a saved output folder still works after a restart |
| `media_server.rs` | 3 | Over raw TCP: only registered files are served, and only with the token; a registered file swapped for a symlink is not served; the server binds to loopback only |

### Desktop frontend tests (`apps/desktop/src/lib/stores/*.test.ts`)

Vitest in a Node environment (`vite.config.ts`). `npm run check` runs svelte-check.

| File | Tests | Proves |
|---|---|---|
| `queue.test.ts` | 5 | Sources added once and clips kept in order; end and duration are mutually exclusive and empty names are dropped; immutability; per-source options; imports merge and only sources with clips are planned |
| `run.test.ts` | 3 | Job state from queued to done; run progress never moves backwards and finishes with the summary; failure and cancellation mapping |

## Where Each Suite Runs in CI

| Suite | `test` (Linux + Windows, bundled engine) | `minimum-engine` (FFmpeg 6.1) | `desktop` (Linux + Windows) | Release workflow |
|---|---|---|---|---|
| Core unit + integration, CLI | Yes | Yes | No | Yes (`cargo test --workspace`) |
| Benches (`--ignored bench`) | Yes (release mode) | No | No | No |
| Desktop backend | No | No | Linux only | No |
| Frontend vitest + svelte-check | No | No | Yes | No |
| Bindings and notices up to date | No | No | Bindings on both; notices on Linux | Notices (Linux) |

## Validation Status (2026-10-06)

From `checklists/validation.md`, all automated suites passed against both the distribution FFmpeg
6.1.1 and the bundled n9.0.2 on Linux.

| Success criterion | Result |
|---|---|
| SC-001 fast 1080p clip under 5 s | 0.3 s (4 s clip), 0.73 s (50 s clip) |
| SC-002 accurate within one frame | First frame matches (about 48 dB PSNR) for MP4, MPEG-TS, mislabelled TS |
| SC-003 10k files / 1k videos under 60 s | 6.3 s |
| SC-004 100 clips, one failure isolated | 99 ok, 1 failed, exit 1 |
| SC-005 first clip within 5 minutes of install | Partial: Linux deb and AppImage verified; Windows pending |
| SC-006 malformed input reported before processing | Verified |
| SC-007 cancel under 2 s, no partials, no orphans | Linux verified; Windows Job Objects pending |
| SC-008 identical output from CLI and app | Byte-identical |

### Not covered by automated tests

- Windows behavior (installers, Job Object cancellation, long paths, cloud placeholders) is
  exercised only in CI on `windows-latest`. No manual Windows run has been recorded.
- The desktop UI by hand: file dialogs, drag and drop, `I`/`O`/`Enter` marking, run and cancel from
  the UI (`quickstart.md` §4).
- The release workflow does not run the desktop or frontend tests.

> **Count drift:** The validation document lists the desktop bridge suite as 5 tests. `bridge.rs`
> now has 6 (`export_carries_settings_defaults_and_saved_output_survives_restart` was added after
> the validation run). All other counts match the source; `clip_single.rs` is listed there as
> "6 (+1 Windows-only)".
