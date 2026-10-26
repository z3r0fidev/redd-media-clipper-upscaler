# rmedia

Cut clips from videos on **Linux and Windows**, from the command line or a desktop app:

- **One clip** from manual timestamps, **many clips** from a clip-list text file, or a **batch** of
  many videos from a JSON/CSV manifest.
- **Folders** are scanned for real videos (non-videos, cover art and partial downloads are skipped).
- **Fast** mode cuts without re-encoding (instant, lossless, starts on the keyframe at or before
  your start time and tells you so). **Accurate** mode re-encodes and is frame-exact.
- Source files are never modified; existing outputs are never overwritten unless you ask.

**Phone-ready upscaling** (0.2.0, release candidate): `rmedia upscale` makes a video fill a phone
held upright, using a device preset (default 1080x2340):

- **Smart reframe** turns landscape into portrait by following faces, cutting at scene changes
  and showing the whole frame over a blurred background where no face fits. `center` and `blur`
  are the alternatives.
- **Three quality tiers**: classical filters on the CPU, the GPU through Vulkan, and an AI
  upscaler (Real-ESRGAN) on graphics cards that pass its self-test. `auto` picks what this machine
  runs best and never the AI tier.
- Cut and upscale in one pass (`clip --upscale`), upscale from batch manifests, and a desktop
  **Upscale for phone** view with a before/after preview.

**GIF export for Reddit** (0.3.0, in development): `rmedia gif` makes a GIF that fits a Reddit
post or comment, or a silent looping "Reddit MP4" that is usually five or more times smaller:

- A bounded size-budget search lowers the least visible settings first and reports what it
  changed; a dry run predicts the size from short sample encodes before the GIF is made.
- GIF targets as data (`reddit-post`, `reddit-comment`, `max-quality`, `flat-ui`, or your own);
  Reddit's limits are estimates, labelled unverified.
- Optional gifsicle and gifski engines for smaller and better-looking GIFs, bundled where the
  licence allows; everything works without them.
- Portrait and square GIFs with 002's face-following reframe, `clip --gif`, `batch --gif`, and a
  desktop **GIF for Reddit** view. See [`docs/releases/0.3.0-dev.md`](docs/releases/0.3.0-dev.md).

Progress: [`docs/roadmap.md`](docs/roadmap.md), `specs/` and `research/`.

## Install

| Platform | Package |
|---|---|
| Debian/Ubuntu | `sudo apt install ./rmedia_<version>_amd64.deb` |
| Other Linux | `chmod +x rmedia_<version>_amd64.AppImage && ./rmedia_<version>_amd64.AppImage` |
| Windows 10/11 | `rmedia_<version>_x64-setup.exe` (per-user) or the `.msi` |
| CLI only | `rmedia-cli-<version>-<platform>` archive: unpack anywhere |

Every package includes the `rmedia` command-line tool and its own FFmpeg (installed as
`rmedia-ffmpeg` / `rmedia-ffprobe`, so it never conflicts with a system FFmpeg):

- **deb**: `rmedia` is on your PATH (`/usr/bin`).
- **Windows**: it is in the install folder (`%LOCALAPPDATA%\rmedia\` for the per-user setup);
  add that folder to PATH, or use the CLI archive. The setup downloads Microsoft WebView2 if it is
  missing (preinstalled on Windows 11 and current Windows 10).
- **AppImage**: the CLI stays inside the image — use the CLI archive for command-line use.

Check with:

```console
$ rmedia doctor
ffmpeg:   /usr/bin/rmedia-ffmpeg (bundled)
version:  n9.0.2-31-gc89d6ee1ca-20261021
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

# make a video phone-ready (default preset phone-fill, 1080x2340)
rmedia upscale game.mp4            # writes upscaled/game_phone-fill.mp4
rmedia upscale --list-presets
rmedia upscale talk.mp4 --preset reddit-916 --reframe smart --dry-run   # shots, crop, tier, estimates
rmedia clip game.mkv -s 1:30 -d 20 --upscale   # cut and upscale in one pass
rmedia doctor --recheck            # which tiers this machine can run (CPU, GPU, AI)

# GIFs for Reddit (0.3.0, in development)
rmedia gif clip.mp4                # writes gifs/clip_reddit-post.gif, under 8 MB
rmedia gif talk.mp4 -s 0:12 -e 0:18 --target reddit-comment --shape 9:16
rmedia gif clip.mp4 --format mp4   # the silent looping Reddit MP4
rmedia clip game.mkv -s 1:30 -d 6 --gif   # cut and make a GIF in one pass
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
| `←` / `→` or `,` / `.` | previous / next frame (`Shift`: 1 s) |
| `Home` / `End` | jump to start / end |
| `Enter` or `A` | add the clip to the queue |

Edit times and names in the queue, choose audio track / subtitles per video, then **Run**.
**Export…** saves the queue as a manifest you can run with `rmedia batch` — the results are
identical. Formats the system player can't show are previewed through a lightweight copy (or exact
still frames); clips are always cut from the original.

Switch the player to **Upscale for phone** to see what a phone would show: the kept part of the
picture follows faces during playback, the timeline marks blurred-background parts, and a
phone-shaped before/after compares the result at the playhead. **Upscale now** runs it at once;
**Add to queue** keeps it for later.

## Building from source

Requirements: Rust 1.94 (see `rust-toolchain.toml`), Node 24, and on Linux
`libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev patchelf`.

```bash
cargo xtask fetch-tools                 # pinned, SHA-256 verified FFmpeg, gifsicle and gifski into binaries/ (--no-gifski: without gifski)
cargo xtask fetch-ncnn                  # pinned ncnn for the AI worker (crates/rmedia-sr)
cargo xtask fetch-models                # face detector and AI models into models/
cargo build --workspace                 # CLI: target/debug/rmedia; AI worker: target/debug/rmedia-sr
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

### Releasing

Push a `v*` tag (for example `git tag v0.1.0 && git push origin v0.1.0`). The release workflow
(also runnable by hand) builds the installers and CLI archives on Linux and Windows and uploads
them as workflow artifacts. Publishing a GitHub release and code signing are manual steps.
Before tagging: bump `version` in `Cargo.toml`, `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml` and `apps/desktop/src-tauri/tauri.conf.json`; run
`cargo xtask notices`; and check `cargo xtask check-pins` (every pinned archive mirrored, the
FFmpeg corresponding source published). The release workflow also runs
`cargo xtask check-release-gate`, which fails while gifski is bundled and the app licence is
undecided; a manual run with the `gifski` input off builds without it. Details:
`docs/operations/build-and-release.md`.

Other tasks: `cargo xtask gen-bindings` (TypeScript types from the core), `cargo xtask notices`
(third-party notices), `cargo xtask pin-tools` (move to a newer FFmpeg build),
`cargo xtask archive` / `mirror <tag>` / `check-pins` (keep, mirror and check the pinned
third-party archives), `cargo xtask pin-gif-tools <run-id>` (pin a `gif-tools.yml` build of
gifsicle and gifski), `cargo xtask check-release-gate` (the gifski licence gate).

## Project layout

```text
crates/rmedia-core     all media logic (timestamps, scanning, planning, ffmpeg, previews)
crates/rmedia-cli      the `rmedia` command
crates/rmedia-sr       the AI upscaling worker (ncnn on Vulkan), run as a separate process
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
- Bundled models and libraries: the YuNet face detector (MIT), Real-ESRGAN models (BSD-3-Clause)
  and ncnn (BSD-3-Clause) — texts in `licenses/`.
- Bundled GIF engines (0.3.0), separate programs: gifsicle 1.98 (GPL v2,
  `licenses/GIFSICLE-NOTICE.md`) and gifski 1.34.0 (AGPL v3 or later,
  `licenses/GIFSKI-NOTICE.md`; shipped only once the app licence allows it).
- Third-party Rust and JavaScript components: `THIRD_PARTY_NOTICES.md`.
