# `rmedia` CLI Reference

This page documents every command and flag of the `rmedia` command-line tool, version 0.1.0.
Flags come from the clap definitions in `crates/rmedia-cli/src/main.rs`. Behaviour comes from
`crates/rmedia-cli/src/commands/*.rs`. The help text below was captured from
`target/debug/rmedia` built from the working tree after commit `bd1eef8`.

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
Cut clips from videos — fast (no re-encode) or frame-accurate.

Usage: rmedia [OPTIONS] <COMMAND>

Commands:
  clip    Cut clips from each input video: one with -s/-e/-d, or many with -l LIST
  batch   Run a batch manifest (.json or .csv) listing many videos and their clips
  scan    Find the videos in folders (recursively) and list them
  doctor  Show the video engine in use and its capabilities
  help    Print this message or the help of the given subcommand(s)

Options:
      --ffmpeg <PATH>   Path to the ffmpeg executable (default: $RMEDIA_FFMPEG, then bundled, then PATH)
      --ffprobe <PATH>  Path to the ffprobe executable (default: $RMEDIA_FFPROBE, then bundled, then PATH)
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
  - Without `-o`, every folder named `clips` (case-insensitive) is skipped, and a note is printed.
- A video reached twice (as a file and through a folder, for example) is clipped once.
- If no inputs remain: `error: no input videos (folders contained no videos)`, exit 3.

### Option Defaults

| Option | Default |
|---|---|
| `--mode` | `fast` |
| `-o` | `<source folder>/clips` |
| `--name` | `{source}_{index:03}_{name-or-range}.{ext}`. With more than one source **and** `-o`: `{source}/{source}_{index:03}_{name-or-range}.{ext}` (one subfolder per source). |
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
  subfolder per source, as in `clip`.
- `--continue-on-missing`: sources whose files do not exist are dropped with a stderr warning:
  `warning: <origin>: <path> not found; skipping its N clip(s)`. If none remain, exit 3. Without
  this flag, a missing source is a `SOURCE_MISSING` plan error and nothing runs (exit 3).
- `-j` is 1–64. The default is the same as `clip`.

```bash
rmedia batch jobs.json
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
version:  n9.0.2-22-g46d8f462ee-20261005
encoder:  aac         yes
encoder:  libopus     yes
encoder:  libvpx-vp9  yes
encoder:  libx264     yes
accurate mode can write: mp4, m4v, mov, mkv, ts, webm
status:   ready
```

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
| `3` | Invalid input; nothing ran | Clip list / manifest errors, end not after start, missing source, unsupported container, invalid glob, scan root missing |
| `4` | Engine missing, too old, or a required encoder absent | `ENGINE_MISSING` (plans whose only errors are engine errors also return 4) |
| `130` | Cancelled (Ctrl-C) | First Ctrl-C after the summary, or immediately on the second |
