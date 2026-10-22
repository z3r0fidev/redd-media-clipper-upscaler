# `rmedia` CLI Reference

This page documents every command and flag of the `rmedia` command-line tool, version 0.1.0, plus
the upscaling of feature 002 (`rmedia upscale`, `clip --upscale`, `doctor --recheck`), which is
unreleased (0.2.0).
Flags come from the clap definitions in `crates/rmedia-cli/src/main.rs`. Behaviour comes from
`crates/rmedia-cli/src/commands/*.rs`. The help text below was captured from
`target/debug/rmedia` built from the working tree after commit `bd1eef8`; the `upscale` additions
were captured on 2026-10-15 from commit `2bfe3f0`.

Related: [File Formats](file-formats.md) · [JSON Output](json-output.md) ·
[Architecture Overview](../architecture/overview.md).

## Output Discipline

- Results go to **stdout**. Progress bars, warnings and errors go to **stderr**.
- With `--json`, stdout carries only JSON documents ([JSON Output](json-output.md)).
- `--continue-on-missing` warnings always go to stderr, even with `--json`.
- Text that comes from media or file names is stripped of control characters before printing
  (`render::clean`).

## Global Options

Global options are accepted before or after the subcommand.

```text
$ rmedia --help
Cut clips from videos and make them phone-ready: upscaled and reframed for portrait screens.

Usage: rmedia [OPTIONS] <COMMAND>

Commands:
  clip     Cut clips from each input video: one with -s/-e/-d, or many with -l LIST
  upscale  Make videos phone-ready: upscale and reframe to a device preset (default 1080x2340)
  batch    Run a batch manifest (.json or .csv) listing many videos and their clips
  scan     Find the videos in folders (recursively) and list them
  doctor   Show the video engine in use and its capabilities
  help     Print this message or the help of the given subcommand(s)

Options:
      --ffmpeg <PATH>   Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH)
      --ffprobe <PATH>  Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH)
      --recheck         Run the GPU self-tests again now (normally they run only when the hardware, driver or app version changes)
  -v, --verbose...      More detail (show ffmpeg commands with -v)
  -q, --quiet           Only print errors and the final summary
  -h, --help            Print help
  -V, --version         Print version
```

| Flag | Effect |
|---|---|
| `--ffmpeg PATH`, `--ffprobe PATH` | Use this engine. The path must exist. Reported with source `config`. |
| `-v`, `--verbose` (repeatable) | Dry runs print the exact ffmpeg command per job. `scan` lists rejected files. Failed jobs print ffmpeg's stderr tail. |
| `-q`, `--quiet` | Hides progress bars and plan warnings; errors and the final summary still print |
| `-V`, `--version` | Prints `rmedia 0.1.0` |

### Engine Lookup Order

`tools::locate` searches in this order (`crates/rmedia-core/src/tools.rs`):

1. `--ffmpeg` / `--ffprobe`.
2. The environment variables `RMEDIA_FFMPEG` / `RMEDIA_FFPROBE`.
3. `rmedia-ffmpeg[.exe]` / `rmedia-ffprobe[.exe]` in the same folder as the running executable.
4. `ffmpeg` / `ffprobe` on `PATH`.

The ffmpeg version must be 6.0 or later; git or nightly version strings are accepted. Otherwise
the command fails with `ENGINE_MISSING` (exit 4).

## `rmedia clip`

Cuts one clip (`-s` with optional `-e` or `-d`) or every clip in a clip list (`-l`) from each
input video. Inputs may be files, folders (scanned for videos) and paths read with
`--inputs-from`.

```text
$ rmedia clip --help
Cut clips from each input video: one with -s/-e/-d, or many with -l LIST

Usage: rmedia clip [OPTIONS] [INPUT]...

Arguments:
  [INPUT]...  Input video files and/or folders (folders are scanned for videos)

Options:
      --ffmpeg <PATH>          Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH)
      --inputs-from <FILE>     Read more inputs from a file (one path per line), or "-" for stdin
      --ffprobe <PATH>         Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH)
      --no-recursive           Only look at the top level of each folder
      --max-depth <N>          Maximum folder depth (1 = top level only)
  -v, --verbose...             More detail (show ffmpeg commands with -v)
      --include <GLOB>         Only include matching paths (glob, relative to the folder; repeatable), e.g. "*.mp4"
  -q, --quiet                  Only print errors and the final summary
      --exclude <GLOB>         Exclude matching paths (glob; repeatable), e.g. "**/drafts/**"
      --hidden                 Include hidden files and folders
      --follow-symlinks        Follow symbolic links (loops are detected)
  -s, --start <TIME>           Clip start: 90, 1:30, 00:01:30.500, 1m30s …
  -l, --list <FILE>            Clip list file: one clip per line, "START END|+DURATION [NAME]" (applied to every input)
  -e, --end <TIME>             Clip end (a time, or "end"). Defaults to the end of the video
  -d, --duration <DURATION>    Clip length instead of an end time: 15, +15, 1:00 …
      --mode <MODE>            fast = no re-encode, starts at the keyframe before START; accurate = frame-exact re-encode [default: fast] [possible values: fast, accurate]
  -o, --output-dir <DIR>       Output folder (default: a "clips" folder next to each source)
      --name <TEMPLATE>        Output name template: {source} {index} {index:03} {name} {name-or-range} {name-or-start} {start} {end} {range} {ext}
      --container <EXT>        Output container/extension (default: same as the source)
      --on-exists <ON_EXISTS>  What to do when an output file already exists [default: skip] [possible values: skip, overwrite, rename]
      --no-audio               Drop all audio
      --audio <TRACKS>         Keep only these audio tracks (1-based), e.g. 1,2
      --no-subs                Drop subtitles
  -j, --jobs <N>               How many clips to process at once (default: half the CPU cores, max 4)
      --dry-run                Show what would be done without writing anything
      --json                   Machine-readable output on stdout (NDJSON events, or one document for --dry-run)
  -h, --help                   Print help
```

### Clip and Upscale in One Pass

`--upscale` makes every clip phone-ready while it is cut: one ffmpeg process per clip, no
intermediate file. It accepts the upscale flags of [`rmedia upscale`](#upscale-options) (`--preset`,
`--reframe`, `--tier`, `--hdr`, `--motion`, `--crf`); `--crop-path` and `--no-analyze` are
`upscale`-only. Those flags without `--upscale` are a usage error (exit 2:
`error: --preset needs --upscale`). `--mode` does not apply (upscaling always
re-encodes) and outputs are always `.mp4`.

- Names: `{source}_{index:03}_{name-or-range}_{preset}.mp4` in `clips/` next to the source. With
  `-o` and several sources: `{source}/{source}_{index:03}_{name-or-range}_{preset}.mp4`.
- A clip from 0 to the end of the video is a whole-video upscale: `{source}_{preset}.mp4` in
  `upscaled/`.
- Folder scans skip both `clips/` and `upscaled/` (or the `-o` folder).

```bash
rmedia clip land.mp4 -s 0 -e 4 --upscale --preset reddit-feed-45
# ✓ …/clips/land_001_00-00-00.000_00-00-04.000_reddit-feed-45.mp4   (1080x1350, H.264)
```

### Argument Rules (from clap)

| Rule | Detail |
|---|---|
| Inputs | At least one `INPUT`, unless `--inputs-from` is given |
| Clip source | `-s` is required unless `-l` is given. `-l` conflicts with `-s`, `-e` and `-d`. |
| End | `-e` conflicts with `-d`. With neither, the clip runs to the end of the video. |
| `-d` | A leading `+` is optional (`15` and `+15` are the same) |
| `-s`, `-e`, `-d` | Accept values starting with `-` so the time parser, not clap, reports negatives |
| `--no-audio` | Conflicts with `--audio` |
| `--audio` | Comma-separated, 1-based audio track numbers in source order |
| `-j` | Integer 1–64 |
| `--max-depth` | Integer ≥ 1 |

### Input Expansion

Implemented in `commands/scan.rs` › `expand_inputs`:

- Each `INPUT` is made absolute. Lines from `--inputs-from FILE` resolve against the list file's
  folder; lines from `--inputs-from -` resolve against the working directory. See
  [File Formats › Path lists](file-formats.md#path-lists---inputs-from).
- Folders are scanned with the scan flags (`--no-recursive`, `--max-depth`, `--include`,
  `--exclude`, `--hidden`, `--follow-symlinks`).
- rmedia's own outputs are excluded from folder scans so re-runs never clip earlier clips:
  - With `-o DIR`, that folder is excluded by real path. If `-o` is one of the scanned folders,
    the command fails with exit 3.
  - Every folder named `clips` or `upscaled` (rmedia's default output folders, case-insensitive)
    is skipped, with or without `-o`, and a note is printed. Pass such a folder directly to use
    its videos.
- A video reached twice (as a file and through a folder, for example) is clipped once.
- If no inputs remain: `error: no input videos (folders contained no videos)`, exit 3.

### Option Defaults

| Option | Default |
|---|---|
| `--mode` | `fast` |
| `-o` | `<source folder>/clips` |
| `--name` | `{source}_{index:03}_{name-or-range}.{ext}`. With more than one source **and** `-o`: `{source}/{source}_{index:03}_{name-or-range}.{ext}` (one sub-folder per source). |
| `--container` | Source extension. In accurate mode, containers without an encode profile fall back to `mp4` with a warning. |
| `--on-exists` | `skip` |
| Streams | All video, audio and subtitle streams the output container supports |
| `-j` | clamp(CPU cores / 2, 1, 4) |

### Examples

```bash
rmedia clip game.mkv -s 1:30 -e 2:00
rmedia clip game.mkv -s 90 -d 15 --mode accurate -o clips/
rmedia clip game.mkv -l highlights.txt --dry-run
rmedia clip ~/Videos/matches -l intro.txt
rmedia scan ~/Videos --include '*.mp4' --paths | rmedia clip --inputs-from - -s 0 -d 10
```

Dry run (real output, paths shortened):

```text
$ rmedia clip v/a.mp4 -s 10.5 -e 15.5 --dry-run
#1   would write: /work/v/clips/a_001_00-00-10.500_00-00-15.500.mp4
     from /work/v/a.mp4  00:00:10.500 → 00:00:15.500  [fast]
     fast mode starts at the keyframe at 00:00:10.000 (00:00:00.500 early)
1 clip(s) planned, 0 would be skipped. Nothing was written (dry run).
```

Re-running when the output exists (default `--on-exists skip`):

```text
- /work/v/clips/a_001_00-00-10.500_00-00-15.500.mp4 skipped: output already exists
0 succeeded, 1 skipped, 0 failed, 0 cancelled
```

A completed run prints one line per job: `✓ <output>`, with `(actual S → E, requested S → E)`
when the produced span differs by more than 50 ms; `- <output> skipped: <reason>`;
`✗ <output> failed: <message>`; or `✗ <output> cancelled`. A totals line follows.

### Cancellation

The first Ctrl-C prints `cancelling… (press Ctrl-C again to quit immediately)`, cancels the run,
and still prints the summary (exit 130). A second Ctrl-C makes a best-effort deletion of the temp
files and exits 130 at once. Children die with their process group / PDEATHSIG (Linux) or Job
Object (Windows). Anything left behind is purged by the next run's journal check
(`commands/clip.rs` › `execute`).

## `rmedia batch`

Runs a JSON or CSV manifest. The format is chosen by the file extension (`.json` or `.csv`; any
other extension is exit 3). See [File Formats › Manifests](file-formats.md#json-manifest-v1).

```text
$ rmedia batch --help
Run a batch manifest (.json or .csv) listing many videos and their clips

Usage: rmedia batch [OPTIONS] <MANIFEST>

Arguments:
  <MANIFEST>  Manifest file: .json (structured) or .csv (one row per clip)

Options:
      --continue-on-missing    Skip videos that do not exist instead of stopping before anything runs
      --ffmpeg <PATH>          Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH)
      --ffprobe <PATH>         Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH)
      --mode <MODE>            Default mode for clips the manifest does not specify [possible values: fast, accurate]
  -o, --output-dir <DIR>       Default output folder for clips the manifest does not specify
  -v, --verbose...             More detail (show ffmpeg commands with -v)
      --name <TEMPLATE>        Default name template
  -q, --quiet                  Only print errors and the final summary
      --container <EXT>        Default output container/extension
      --on-exists <ON_EXISTS>  What to do when an output file already exists (default: skip) [possible values: skip, overwrite, rename]
  -j, --jobs <N>               How many clips to process at once
      --dry-run                Show what would be done without writing anything
      --json                   Machine-readable output on stdout
  -h, --help                   Print help
```

Notes:

- Only flags actually given act as defaults, and the manifest's own values win. The layering is
  built-in < CLI flags < manifest `defaults` < per video < per clip. See
  [File Formats › Option layering](file-formats.md#option-layering).
- `batch` has **no** stream-selection flags (`--no-audio`, `--audio`, `--no-subs`). Use the
  manifest's `audio` / `subtitles` options (JSON only).
- With more than one source, `-o` given, and no `--name`, the default template switches to one
  sub-folder per source, as in `clip`.
- `--continue-on-missing`: sources whose files do not exist are dropped with a stderr warning:
  `warning: <origin>: <path> not found; skipping its N clip(s)`. If none remain, exit 3. Without
  this flag, a missing source is a `SOURCE_MISSING` plan error and nothing runs (exit 3).
- `-j` is 1–64. The default is the same as `clip`.
- **Upscaling** (feature 002) follows the manifest's `upscale` options
  ([File Formats › Upscaling](file-formats.md#upscaling-in-manifests)). The upscale flags
  `--preset`, `--reframe`, `--tier`, `--hdr`, `--motion` and `--crf` fill the fields of the videos
  that the manifest upscales, under the manifest's own values; on their own they never turn
  upscaling on. `--upscale` turns it on for every video (a manifest `"enabled": false` still wins).
  The GPU check runs only when some job is upscaled on a tier other than a forced `cpu`.

```bash
rmedia batch jobs.json
rmedia batch phone.json --preset reddit-916      # fills videos with "upscale": {}
rmedia batch jobs.csv --upscale --tier cpu       # every row upscaled on the CPU
rmedia batch jobs.csv --dry-run --json
rmedia batch jobs.csv --json > run.ndjson
```

## `rmedia scan`

Lists real videos in folders. Files can also be given as roots, and file roots bypass the
extension and glob filters.

```text
$ rmedia scan --help
Find the videos in folders (recursively) and list them

Usage: rmedia scan [OPTIONS] <PATH>...

Arguments:
  <PATH>...  Folders (or files) to scan

Options:
      --ffmpeg <PATH>    Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH)
      --no-recursive     Only look at the top level of each folder
      --ffprobe <PATH>   Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH)
      --max-depth <N>    Maximum folder depth (1 = top level only)
      --include <GLOB>   Only include matching paths (glob, relative to the folder; repeatable), e.g. "*.mp4"
  -v, --verbose...       More detail (show ffmpeg commands with -v)
      --exclude <GLOB>   Exclude matching paths (glob; repeatable), e.g. "**/drafts/**"
  -q, --quiet            Only print errors and the final summary
      --hidden           Include hidden files and folders
      --follow-symlinks  Follow symbolic links (loops are detected)
      --paths            Print only the paths of found videos, one per line (pipe into `clip --inputs-from -`)
      --json             Print the result as one JSON document
  -h, --help             Print help
```

| Flag | Notes |
|---|---|
| `--include`, `--exclude` | Globs matched against the path relative to the root, case-insensitive on every OS. `**` spans folders. An invalid glob is exit 3. |
| `--paths` | Conflicts with `--json`. Implies quiet. Skips (with a stderr warning) names containing a line break or invalid UTF-8, so piping into `--inputs-from -` cannot inject extra entries. |
| `--hidden` | Also enters dot-folders and Windows Hidden/System entries |
| `--follow-symlinks` | Loops are reported as `inaccessible` rejections |

A root that does not exist fails with exit 3 before the engine is located. Ctrl-C cancels the
scan (exit 130).

Human output (real run, paths shortened). Each video is one stdout row: duration, resolution,
fps, codec, size, path. The totals line goes to stderr:

```text
$ rmedia scan v
00:00:30.000    640x360   25.00  h264       3.1 MB  /work/v/a.mp4
00:00:20.000    320x240   25.00  h264     718.1 KB  /work/v/sub/b.mkv
2 video(s), 1 rejected (-v lists them); skipped 0 hidden, 0 temp, 0 symlink, 0 excluded, 0 online-only (0.1 s)
```

## `rmedia upscale`

Makes each input video phone-ready: it scales and reframes the whole video to a device preset's
screen size, re-encodes it and verifies the result. Added in the unreleased 0.2.0
([release notes](../releases/0.2.0.md)). The CPU, GPU and AI tiers, smart reframe, manifest
upscaling (`batch`) and `clip --upscale` exist ([Roadmap](../roadmap.md)).

```text
rmedia upscale [OPTIONS] [INPUT]...
rmedia upscale --list-presets [--json]
```

Each input is processed whole, one job per video. Inputs are files, folders (scanned with the
`scan` options) or `--inputs-from FILE|-`. `INPUT` is required unless `--inputs-from` or
`--list-presets` is given. To upscale part of a video, use `rmedia clip … --upscale` (one pass,
no intermediate file) or a manifest.

### Upscale Options

| Flag | Values | Default | Notes |
|---|---|---|---|
| `--preset ID` | an id from `--list-presets` | `phone-fill` | An unknown id is an input error (exit 3, `PRESET_UNKNOWN`) |
| `--reframe` | `smart`, `center`, `blur` | `smart` | `smart` follows faces (needs the YuNet model, otherwise `blur` with a warning). `center` crops the middle; `blur` keeps the whole frame over a blurred background |
| `--tier` | `auto`, `cpu`, `gpu`, `ai` | `auto` | `auto` takes the GPU tier when `doctor` found a usable Vulkan GPU that is not slower than the CPU and has not failed since, otherwise `cpu`; it never takes `ai` (it suggests it under 720p). `ai` needs a GPU that passed the AI self-test (60 s of 480p predicted in 10 min or less) and SDR output. A forced tier the machine cannot run stops with `TIER_UNAVAILABLE` (exit 3) |
| `--hdr` | `sdr`, `keep` | `sdr` | `sdr` tone-maps HDR sources. `keep` needs an HDR-capable preset, otherwise `HDR_UNSUPPORTED` (exit 3) |
| `--motion` | `calm`, `normal`, `fast` | `normal` | Smart-reframe camera speed |
| `--crf N` | 0 to 51 | the preset's value | Lower is better quality and a bigger file |
| `--crop-path FILE` | path | none | Writes the per-frame crop path of the one smart-reframe video as `{"schema":1,"fps","crop":[w,h],"x":[…]}`. Analyses first (with or without `--dry-run`); exit 2 when the run has no or several smart videos, or with `--no-analyze`; exit 3 when FILE is one of the run's videos |
| `--no-analyze` | flag | off | With `--dry-run`, skips the smart-reframe analysis (shown as `pending`) |
| `--list-presets` | flag | off | Lists presets and exits |

Scan options (`--no-recursive`, `--max-depth`, `--include`, `--exclude`, `--hidden`,
`--follow-symlinks`) and the shared options `-o`, `--name`, `--container`, `--on-exists`,
`--no-audio`, `--audio`, `--no-subs`, `-j`, `--dry-run`, `--json`, `--ffmpeg`, `--ffprobe`, `-v`
and `-q` are accepted, with these differences from `clip`:

- There is no `--mode`: upscaling always re-encodes.
- Outputs are always `.mp4`. Another `--container` is ignored with the warning
  `upscaled outputs are always .mp4`.
- The default output folder is `upscaled/` next to each source (`-o` replaces it). While
  scanning folders, folders named `upscaled` or `clips` and the `-o` folder are excluded.
- The default name is `{source}_{preset}.mp4`. The name template also accepts `{preset}`, which
  is valid only for upscale jobs.

### Presets

`rmedia upscale --list-presets` (real output, no user presets):

```text
phone-fill       1080x2340  HEVC   Phone fill (default)
                 for: iPhone 15–18 (all sizes, incl. Air), Galaxy S22–S26, Pixel 8–11 (≈1% bars)
phone-fill-hd    1440x3120  HEVC   Phone fill HD
                 for: 1440p phones (Galaxy S Ultra, Pixel Pro XL), iPhone Pro Max
pixel-tall       1080x2400  HEVC   Pixel tall
                 for: Pixel 8–11 (incl. 10 Pro XL), Tall 20:9 Android phones
reddit-916       1080x1920  H.264  Reddit 9:16
                 for: Reddit full-screen player
reddit-feed-45   1080x1350  H.264  Reddit feed 4:5
                 for: Reddit feed
fold-inner       1080x1200  HEVC   Fold inner
                 for: Galaxy Z Fold inner screen, Pixel Fold / Pixel 9–10 Pro Fold inner screen
duo              1080x1566  HEVC   iPhone Duo
                 for: iPhone Duo (foldable)
```

User presets are `*.json` files in a `presets` folder inside the config directory
([Configuration](../administration/configuration.md#file-locations)). They follow
`specs/002-mobile-upscale/contracts/preset.schema.json`. A file that is invalid is skipped; the
text listing prints `warning: preset file … ignored: …` and `--json` lists it under `problems`.
User presets are tagged `[user]` in the text listing. The JSON form is documented in
[JSON Output](json-output.md#rmedia-upscale---list-presets---json).

### Planning and Running

- The plan shows the preset, reframe, tier and the reason for the tier choice, an estimated size
  and time, and any warnings. Example (real dry run):

  ```text
  #1   would write: /work/upscaled/t_phone-fill.mp4
       from /work/t.mp4  00:00:00.000 → 00:00:02.000  [upscale]
       preset phone-fill 1080x2340, reframe smart, tier cpu (auto: the classical CPU tier)
       reframe: smart, 2 shot(s); blurred background 00:00–00:10
       estimate ~5.9 MB, ~7 s
  ```

- Presets with platform limits (for example the Reddit presets) produce a `LIMIT_EXCEEDED`
  warning when the predicted size or the duration exceeds the limit. For size, the video bitrate
  is capped to fit.
- rmedia warns when an output folder has less free space than the summed estimates plus 20%.
- A source already at or above the target size is flagged (`no_upscale_needed`).
- After encoding, rmedia probes the output; a bad output fails the job with `VERIFY_FAILED`.
- The engine must provide the preset's encoder (`libx265` for HEVC presets, `libx264` for H.264
  presets) and the filters upscaling uses; otherwise planning reports `ENGINE_MISSING` and the
  command exits 4. Check with [`rmedia doctor`](#rmedia-doctor).
- Ctrl-C cancels as for `clip`; no partial output is left.

Exit codes are those in [Exit Codes](#exit-codes).

## `rmedia doctor`

Reports the engine in use and its capabilities (FR-026).

```text
$ rmedia doctor --help
Show the video engine in use and its capabilities

Usage: rmedia doctor [OPTIONS]

Options:
      --ffmpeg <PATH>   Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH)
      --json            Print the report as JSON
      --ffprobe <PATH>  Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH)
  -v, --verbose...      More detail (show ffmpeg commands with -v)
  -q, --quiet           Only print errors and the final summary
  -h, --help            Print help
```

Real output with the engine selected through `RMEDIA_FFMPEG` / `RMEDIA_FFPROBE` (hence `config`;
an installed package reports `bundled`):

```text
$ rmedia doctor
ffmpeg:   /work/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu (config)
ffprobe:  /work/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu (config)
version:  n9.0.2-31-gc89d6ee1ca-20261021
encoder:  aac         yes
encoder:  libopus     yes
encoder:  libvpx-vp9  yes
encoder:  libx264     yes
encoder:  libx265     yes
filters:  all upscaling filters present
tier:     cpu  yes, 35 fps
tier:     gpu  yes, 21 fps
tier:     ai   no, 0.3 fps (too slow on Intel(R) Graphics (RPL-U) (integrated): about 106 min for 60 s of 480p (limit 10 min))
gpu:      0 Intel(R) Graphics (RPL-U) (integrated) — usable
gpu:      1 llvmpipe (LLVM 20.1.2, 256 bits) (software) — not used: software renderer
model:    face_detection_yunet_2026may.onnx ok
model:    realesr-animevideov3 (x2, x3, x4) ok
model:    rmedia-sr ok
checked:  2026-10-19T05:51:14Z
accurate mode can write: mp4, m4v, mov, mkv, ts, webm
status:   ready
```

The `filters:` line reports whether the filters that `rmedia upscale` uses are present
(`zscale`, `cas`, `hqdn3d`, `bwdif`, `gblur`, `tonemap`, `sendcmd`, `scdet`, `libplacebo`,
`libvmaf`). When some are missing it reads `filters:  missing <names> (upscaling needs some of
these)`. A missing filter does not change `status` or the exit code; `rmedia upscale` checks
the filters it needs when it plans. `--json` lists every filter under `filters`.

The `tier:`, `gpu:`, `model:` and `checked:` lines come from the machine capability check
(added in 0.2.0). It lists the Vulkan devices, rejects software renderers (llvmpipe,
lavapipe, SwiftShader), and times a short CPU and GPU self-test that includes the preset's
encoder (`fps`). The result is stored in `capability.json` in the state folder and reused until
the hardware, driver, engine or app version changes; the first check takes a few seconds, later
ones about 0.1 s. `--recheck` runs it again. `rmedia upscale` runs the same check. A tier that
failed after being chosen automatically is listed as `demoted:` and is not chosen again until
the next check. A missing GPU never changes `status` or the exit code.

Problems print to stderr as `problem:  <text>`. The exit code is 0 when the engine was found and
its encoder list read; otherwise 4. "No encoders for accurate mode" is reported as a problem, but
the status stays `ready`, because fast mode still works.

## Exit Codes

From `crates/rmedia-cli/src/main.rs` › `exit`, `RunSummary.exit_code`, and clap. Verified by
running the binary (see [JSON Output](json-output.md)).

| Code | Meaning | Typical causes |
|---|---|---|
| `0` | Every job succeeded or was skipped; a dry run that planned without errors; `scan`/`doctor` succeeded; `--help`/`--version` | — |
| `1` | At least one job failed, or a single job was cancelled while the run continued | `ENGINE_FAILED`, `VERIFY_FAILED`, `DISK_FULL`, `OUTPUT_UNWRITABLE`, `INTERNAL` |
| `2` | Usage error (clap) | Unknown flag, `-e` together with `-d`, missing required argument |
| `3` | Invalid input; nothing ran | Clip list / manifest errors, end not after start, missing source, unsupported container, invalid glob, scan root missing; for `upscale`: unknown preset, `--hdr keep` with a preset that cannot keep HDR, a forced `--tier gpu` or `ai` |
| `4` | Engine missing, too old, or a required encoder or filter absent | `ENGINE_MISSING` (plans whose only errors are engine errors also return 4); for `upscale` also a missing `libx265`/`libx264` or upscaling filter |
| `130` | Cancelled (Ctrl-C) | First Ctrl-C after the summary, or immediately on the second |
