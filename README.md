# rmedia

Cut clips from videos on **Linux and Windows**, from the command line or a desktop app:

- **One clip** from manual timestamps, **many clips** from a clip-list text file, or a **batch** of
  many videos from a JSON/CSV manifest.
- **Folders** are scanned for real videos (non-videos, cover art and partial downloads are skipped).
- **Fast** mode cuts without re-encoding (instant, lossless, starts on the keyframe at or before
  your start time and tells you so). **Accurate** mode re-encodes and is frame-exact.
- Source files are never modified; existing outputs are never overwritten unless you ask.

Coming next: phone-optimized upscaling with smart reframing (feature 002) and high-quality GIF
export for Reddit (feature 003) — see `specs/` and `research/`.

## Install

| Platform | Package |
|---|---|
| Debian/Ubuntu | `sudo apt install ./rmedia_<version>_amd64.deb` |
| Other Linux | `chmod +x rmedia_<version>_amd64.AppImage && ./rmedia_<version>_amd64.AppImage` |
| Windows 10/11 | `rmedia_<version>_x64-setup.exe` (per-user) or the `.msi` |
| CLI only | `rmedia-cli-<version>-<platform>` archive: unpack anywhere |

Every package includes the `rmedia` command-line tool and its own FFmpeg (installed as
`rmedia-ffmpeg` / `rmedia-ffprobe`, so it never conflicts with a system FFmpeg). Check with:

```console
$ rmedia doctor
ffmpeg:   /usr/bin/rmedia-ffmpeg (bundled)
version:  n9.0.2-22-g46d8f462ee-20261005
status:   ready
```

## Command line

```bash
# one clip (times: 90, 1:30, 00:01:30.500, 1m30s; durations: +15)
rmedia clip game.mkv -s 1:30 -e 2:00
rmedia clip game.mkv -s 90 -d 15 --mode accurate -o clips/

# many clips from a list (one per line: START END|+DURATION [NAME], '#' comments)
rmedia clip game.mkv -l highlights.txt

# a folder: every video in it, recursively, each with the same list
rmedia clip ~/Videos/matches -l intro.txt

# a batch manifest (JSON or CSV), see below
rmedia batch jobs.json

# list the videos in folders; --paths pipes into --inputs-from
rmedia scan ~/Videos --include '*.mp4' --paths | rmedia clip --inputs-from - -s 0 -d 10

# preview what would happen, machine-readable
rmedia batch jobs.csv --dry-run --json
```

Useful options: `--mode fast|accurate`, `-o DIR`, `--name TEMPLATE`
(`{source} {index:03} {name} {name-or-range} {start} {end} {ext}`), `--container mp4`,
`--on-exists skip|rename|overwrite`, `--no-audio`, `--audio 2`, `--no-subs`, `-j N`.
Ctrl-C cancels cleanly (press twice to quit immediately); no partial files are left behind.

Exit codes: `0` ok · `1` some clips failed · `2` usage · `3` invalid input (nothing ran) ·
`4` video engine missing · `130` cancelled. Full reference: `specs/001-video-clipping/contracts/cli.md`.

### Clip lists

```text
# highlights for match 12
0:45          1:10   opening goal
00:03:15.250  +20    save
12:00,500     end
```

### Batch manifests

```json
{ "version": 1,
  "defaults": { "mode": "fast", "output_dir": "clips" },
  "videos": [
    { "input": "match12.mkv", "clips": [ { "start": "0:45", "end": "1:10", "name": "goal" } ] },
    { "input": "match13.mp4", "options": { "mode": "accurate" },
      "clips": [ { "start": 30, "duration": 12.5 }, { "start": "12:00", "end": "end" } ] } ] }
```

```csv
input,start,end,duration,name,mode,output
match12.mkv,0:45,1:10,,goal,,
match13.mp4,30,,12.5,,accurate,
```

Relative paths are relative to the manifest. Options apply as: built-in defaults < command-line
flags < manifest `defaults` < per video < per clip. Schema: `specs/001-video-clipping/contracts/`.

## Desktop app

Add videos or a folder (or drop them anywhere, or pass them on the command line:
`rmedia-desktop video.mp4`), pick one, mark the range and queue it:

| Key | Action |
|---|---|
| `Space` | play / pause |
| `I` / `O` | set in / out |
| `←` / `→` | previous / next frame (`Shift`: 1 s) |
| `Enter` | add the clip to the queue |

Edit times and names in the queue, choose audio track / subtitles per video, then **Run**.
**Export…** saves the queue as a manifest you can run with `rmedia batch` — the results are
identical. Formats the system player can't show are previewed through a lightweight copy (or exact
still frames); clips are always cut from the original.

## Building from source

Requirements: Rust 1.94 (see `rust-toolchain.toml`), Node 24, and on Linux
`libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev patchelf`.

```bash
cargo xtask fetch-tools                 # pinned, SHA-256 verified FFmpeg into binaries/
cargo build -p rmedia-cli               # CLI: target/debug/rmedia
(cd apps/desktop && npm ci && npx tauri dev)   # desktop app (dev)

# release packages
cargo xtask stage-cli
(cd apps/desktop && npx tauri build)    # deb + AppImage on Linux, nsis + msi on Windows
```

Tests (real media generated at test time with FFmpeg; nothing binary is committed):

```bash
export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
       RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
cargo test --workspace                          # core + CLI
cargo test --workspace --release -- --ignored bench   # timing targets
cargo build -p rmedia-cli && cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
(cd apps/desktop && npm test && npm run check)
```

Other tasks: `cargo xtask gen-bindings` (TypeScript types from the core), `cargo xtask notices`
(third-party notices), `cargo xtask pin-tools` (move to a newer FFmpeg build).

## Project layout

```text
crates/rmedia-core     all media logic (timestamps, scanning, planning, ffmpeg, previews)
crates/rmedia-cli      the `rmedia` command
crates/rmedia-testkit  test fixtures generated with FFmpeg
apps/desktop           Tauri 2 + Svelte 5 desktop app (thin layer over the core)
xtask                  build tasks (engine fetch, bindings, notices, CLI staging)
specs/  research/      Spec Kit specifications, plans, contracts and research notes
```

## Licensing

- rmedia's own license: **to be decided by the owner** (Cargo manifests currently declare
  `MIT OR Apache-2.0` as a placeholder).
- Bundled FFmpeg: GPL v3 or later, shipped as separate programs — see
  `licenses/FFMPEG-NOTICE.md` for the build and the corresponding-source offer.
- Third-party Rust and JavaScript components: `THIRD_PARTY_NOTICES.md`.
