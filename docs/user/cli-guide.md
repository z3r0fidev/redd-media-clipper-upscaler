# Command-Line Guide

## Overview

The `rmedia` command cuts clips from videos without opening the desktop app. Use it to:

* cut one clip from times you already know;
* cut many clips from a clip-list file;
* cut the same clips from every video in a folder;
* run a batch manifest that lists many videos and their clips;
* find the videos in a folder tree;
* use rmedia from scripts, with plans and results in JSON.

This guide is organised by task. For every option and the exact output formats, see the reference
documents:

* [CLI Reference](../api/cli-reference.md)
* [File Formats Reference](../api/file-formats.md): clip lists, JSON and CSV manifests
* [JSON Output Reference](../api/json-output.md)

## Before You Begin

* `rmedia` is installed and on your `PATH`. See [Getting Started](getting-started.md#where-the-command-line-tool-ends-up).
* `rmedia doctor` reports `status:   ready`.

## The Commands

| Command | Purpose |
|---|---|
| `rmedia clip` | Cut clips from one or more videos or folders: one clip with `-s`/`-e`/`-d`, or many with `-l LIST`. |
| `rmedia upscale` | Make videos phone-ready: scale and reframe each whole video to a device preset. Unreleased (0.2.0-dev). |
| `rmedia batch` | Run a batch manifest (`.json` or `.csv`) that lists many videos and their clips. |
| `rmedia scan` | Find the videos in folders (recursively) and list them. |
| `rmedia doctor` | Show the video engine in use and its capabilities. |

Run `rmedia --help` or `rmedia <command> --help` for the built-in help.

## Write Times

Every time value (`-s`, `-e`, clip lists and manifests) uses the same format:

| You write | Meaning |
|---|---|
| `90`, `90.5` | Seconds, with up to 6 decimal places |
| `1:30`, `01:30.5` | Minutes and seconds |
| `00:01:30.500` or `00:01:30,500` | Hours, minutes, seconds and fractions (a comma also works as the decimal mark) |
| `1h2m3.5s`, `1m30s`, `90s`, `500ms` | Units: `h`, `m`, `s`, `ms`, largest first, each used once |
| `end` | The end of the video (end times only) |
| `+15`, `+1:00` | A duration rather than an end point (end times only) |

Negative times are not allowed. Time `0` is the first video frame.

## Cut One Clip

```bash
# From 1:30 to 2:00
rmedia clip game.mkv -s 1:30 -e 2:00

# 15 seconds starting at 90 s
rmedia clip game.mkv -s 90 -d 15

# From 12:00 to the end of the video
rmedia clip game.mkv -s 12:00
```

* `-s, --start TIME` is required (unless you use `-l`).
* `-e, --end TIME` sets the end. It also accepts `end`.
* `-d, --duration DURATION` sets the length instead. You can write `15` or `+15`.
* If you give neither `-e` nor `-d`, the clip runs to the end of the video.
* `-e` and `-d` cannot be used together.

If the end time is past the end of the video, rmedia cuts to the end and warns you:

```text
warning: end 00:01:30.000 is past the end of the video; clip ends at 00:01:00.000
```

Choose fast or accurate mode with `--mode fast` (default) or `--mode accurate`. See
[Fast mode and accurate mode](getting-started.md#fast-mode-and-accurate-mode).

## Cut Many Clips From a Clip List

A clip list is a UTF-8 text file with one clip per line:

```text
# highlights for match 12
0:45          1:10   opening goal
00:03:15.250  +20    save
12:00,500     end
```

Each line is `START END [NAME]` or `START +DURATION [NAME]`:

* Fields are separated by spaces or a comma.
* `END` may be `end`.
* `NAME` is optional and is the rest of the line. It can contain spaces.
* A line that starts with `#` is a comment. A `#` with a space on each side starts a comment at the
  end of a line. A `#` inside a name, as in `goal #3`, is kept.
* In `12:00,500`, the comma is a decimal mark (a comma directly after `:SS` and followed by exactly
  3 digits).

Run it with `-l`:

```bash
rmedia clip game.mkv -l highlights.txt
```

`-l` cannot be combined with `-s`, `-e` or `-d`. The list applies to **every** input, so you can
use one list for several videos.

rmedia checks the whole list before it cuts anything. If any line is wrong, it reports the file and
line number, cuts nothing, and exits with code `3`:

```text
error: /home/me/bad.txt:1: invalid time "0:1x": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s
```

Full format details: [File Formats Reference](../api/file-formats.md).

## Cut Clips From Every Video in a Folder

Give a folder instead of a file. rmedia searches it and its sub-folders for real videos and applies
the same clip, or clip list, to each one:

```bash
rmedia clip ~/Videos/matches -l intro.txt
rmedia clip ~/Videos/matches -s 0 -d 10
```

While it searches, rmedia prints a summary such as:

```text
found 2 video(s) in 1 folder(s), 1 file(s) rejected
```

You can mix files and folders. A video reached twice (for example, as a file and through its
folder) is clipped only once.

To control the search, use the same options as `rmedia scan`. See
[Find videos in folders](#find-videos-in-folders).

### Re-runs do not pick up your own clips

* Without `-o`, rmedia writes into a `clips` folder next to each video, and skips every folder named
  `clips` when it searches. It tells you when it does this:

  ```text
  note: skipped 1 folder(s) named "clips" (rmedia's default output folder); pass such a folder directly to use its videos as sources
  ```

* With `-o`, rmedia skips the output folder. The output folder cannot be one of the folders you are
  searching. If it is, rmedia stops with an error that suggests a sub-folder. See
  [The output folder is also a scanned input folder](troubleshooting.md#the-output-folder--is-also-a-scanned-input-folder).

### Many videos into one output folder

When you clip more than one video **and** give `-o`, each video's clips go into their own sub-folder
named after the video:

```text
out/a/a_001_00-00-00.000_00-00-02.000.mp4
out/b/b_001_00-00-00.000_00-00-02.000.ts
```

This happens only when you have not set `--name` yourself.

## Find Videos in Folders

`rmedia scan` lists the real videos in one or more folders:

```bash
rmedia scan ~/Videos
```

Each line shows duration, resolution, frame rate, codec, size and path. A summary goes to the error
output (stderr):

```text
00:01:00.000   1280x720   25.00  h264      21.1 MB  /home/me/Videos/a.mp4
00:00:30.023    640x360   25.00  h264       3.3 MB  /home/me/Videos/sub/b.ts
2 video(s), 1 rejected (-v lists them); skipped 1 hidden, 1 temp, 0 symlink, 0 excluded, 0 online-only (0.0 s)
```

Add `-v` to list each rejected file and the reason.

### Search options

These work with `rmedia scan` and with folder inputs to `rmedia clip`.

| Option | Effect |
|---|---|
| `--no-recursive` | Only the top level of each folder. |
| `--max-depth N` | At most N levels deep (`1` = top level only). |
| `--include GLOB` | Only paths that match, relative to the folder, for example `"*.mp4"`. Can be repeated. |
| `--exclude GLOB` | Skip paths that match, for example `"**/drafts/**"`. Can be repeated. |
| `--hidden` | Include hidden files and folders. |
| `--follow-symlinks` | Follow symbolic links (loops are detected). |

Patterns are not case-sensitive on any platform. Quote them so that your shell does not expand them.

The search always skips partial downloads and temporary files, system and trash folders, and files
that the video engine cannot read or that have no video stream. On Windows it also skips online-only
cloud files.

## Pipe a Video List Into `clip`

`--inputs-from FILE` reads extra inputs from a file, one path per line. Use `-` to read from
standard input. This lets you combine `scan` with `clip`:

```bash
rmedia scan ~/Videos --include '*.mp4' --paths | rmedia clip --inputs-from - -s 0 -d 10
```

* `scan --paths` prints only the paths, one per line. It skips, with a warning, any path that
  cannot be listed safely one per line (a name containing a line break, or one that is not valid
  UTF-8).
* In the input file, empty lines and lines that start with `#` are ignored. To use a file whose
  name starts with `#`, write `./#name`.
* Relative paths are resolved from the folder of the list file, or from the current folder when
  reading standard input.

## Run a Batch Manifest

A manifest describes many videos and their clips in one file. Use JSON for full control or CSV for
a spreadsheet-friendly format.

**JSON:**

```json
{ "version": 1,
  "defaults": { "mode": "fast", "output_dir": "clips" },
  "videos": [
    { "input": "match12.mkv", "clips": [ { "start": "0:45", "end": "1:10", "name": "goal" } ] },
    { "input": "match13.mp4", "options": { "mode": "accurate" },
      "clips": [ { "start": 30, "duration": 12.5 }, { "start": "12:00", "end": "end" } ] } ] }
```

**CSV** (header row required, one row per clip):

```csv
input,start,end,duration,name,mode,output
match12.mkv,0:45,1:10,,goal,,
match13.mp4,30,,12.5,,accurate,
```

Run it:

```bash
rmedia batch jobs.json
rmedia batch jobs.csv
```

* The file must end in `.json` or `.csv`.
* Relative paths in the manifest are relative to the manifest file's folder.
* Options are applied in this order, with later ones winning: built-in defaults, then command-line
  options, then the manifest `defaults`, then the options for a video, then the options for a clip.
  Command-line options such as `--mode`, `-o`, `--name`, `--container` and `--on-exists` therefore
  only fill in what the manifest leaves open.
* If a video in the manifest does not exist, rmedia stops before cutting anything (exit code `3`).
  To skip missing videos and run the rest, add `--continue-on-missing`. rmedia then prints a warning
  for each one, for example:

  ```text
  warning: /home/me/jobs.json:videos[1].input: /home/me/missing.mp4 not found; skipping its 1 clip(s)
  ```

The desktop app's **Export…** button produces a manifest that you can run this way. Full format
details: [File Formats Reference](../api/file-formats.md).

## Preview a Run Without Writing Anything

Add `--dry-run` to `clip` or `batch`. rmedia checks everything and shows what it would do, but writes
nothing:

```text
#1   would write: /home/me/clips/game_001_opening goal.mp4
     from /home/me/game.mp4  00:00:05.000 → 00:00:10.000  [fast]
     fast mode starts at the keyframe at 00:00:04.000 (00:00:01.000 early)
#2   would write: /home/me/clips/game_002_save.mp4
     from /home/me/game.mp4  00:00:20.000 → 00:00:23.000  [fast]
2 clip(s) planned, 0 would be skipped. Nothing was written (dry run).
```

A dry run shows:

* each output path;
* clips that would be skipped, and why;
* in fast mode, the keyframe each clip will start on;
* warnings (for example, a format change or dropped subtitles).

Add `-v` to also see the exact FFmpeg command for each clip.

## Get Machine-Readable Output

Add `--json` to make standard output contain only JSON:

| Command | Output |
|---|---|
| `clip`/`batch --dry-run --json` | One JSON document: the plan |
| `clip`/`batch --json` | One JSON event per line (NDJSON) while running, ending with a summary |
| `scan --json` | One JSON document |
| `doctor --json` | One JSON document |

```bash
rmedia batch jobs.csv --dry-run --json > plan.json
rmedia batch jobs.csv --json > run.ndjson
```

Warnings and errors still go to the error output (stderr), so standard output stays valid JSON.
Every document includes `"schema": 1`. Times are in seconds. For the full structure, see the
[JSON Output Reference](../api/json-output.md).

## Name Output Files

By default, clips are saved in a `clips` folder next to each source. Use `-o DIR` to choose another
folder. rmedia creates it if needed.

The default file name pattern is `{source}_{index:03}_{name-or-range}.{ext}`. Use `--name` to
change it:

| Placeholder | Becomes |
|---|---|
| `{source}` | The source file name without its extension |
| `{index}` | The clip number within its source, starting at 1 |
| `{index:03}` | The clip number padded with zeros to the width given (1–9 digits), for example `007` |
| `{name}` | The clip name (empty if the clip has none) |
| `{name-or-start}` | The clip name, or the start time if the clip has none |
| `{name-or-range}` | The clip name, or `start_end` if the clip has none |
| `{start}`, `{end}` | Start or end time, written as `HH-MM-SS.mmm` (safe in file names) |
| `{range}` | `{start}_{end}` |
| `{ext}` | The output extension |

Examples:

```bash
rmedia clip game.mkv -l highlights.txt --name '{name-or-start}.{ext}'
rmedia clip game.mkv -l highlights.txt --name '{source}/{index:02}-{range}.{ext}'
```

* A `/` in the pattern creates a sub-folder inside the output folder.
* The pattern must give a relative path. Empty parts, `.` and `..` are not allowed.
* Characters that are not allowed in file names on Windows or Linux (`< > : " / \ | ? *` and
  control characters) are replaced with `_`. Reserved Windows names such as `CON` get a leading `_`.
  Very long names are shortened.
* An unknown placeholder is an error, for example
  `unknown placeholder {foo} in name template "{foo}.{ext}"`.

### Change the output format

`--container EXT` sets the output format, for example `--container mp4` or `--container mkv`. By
default the output uses the same format as the source. In accurate mode, formats that cannot be
re-encoded in place (such as AVI) are written as MP4, with a warning.

## Choose What Happens When a File Already Exists

`--on-exists` decides what to do when an output file is already there:

| Value | Effect |
|---|---|
| `skip` (default) | Leave the existing file and skip the clip. The clip is reported as `skipped: output already exists`. |
| `rename` | Save the new clip as `name_1.ext`, `name_2.ext`, and so on. |
| `overwrite` | Replace the existing file. rmedia warns `existing output will be replaced`. |

rmedia never overwrites a **source** video, whatever you choose. If two clips in the same run would
get the same name, rmedia stops before cutting and asks you to add `{index}` to the pattern or use
`--on-exists rename`.

## Choose Audio and Subtitle Streams

By default, rmedia keeps all video, audio and subtitle streams.

| Option | Effect |
|---|---|
| `--no-audio` | Remove all audio. |
| `--audio 1,2` | Keep only these audio tracks. Tracks are numbered from 1, in the order they appear in the source. |
| `--no-subs` | Remove subtitles. |

```bash
# Keep only the second audio track (for example, commentary)
rmedia clip game.mkv -s 1:30 -e 2:00 --audio 2
```

If a track does not exist, rmedia stops with an error such as
`audio track 2 does not exist (source has 1)`.

Some formats cannot store some subtitles. rmedia drops those subtitles and warns you (for example
`1 subtitle stream(s) cannot be stored in .ts; dropped`):

* MKV keeps all subtitles.
* MP4, M4V and MOV keep text subtitles, converted to the MP4 text format. Image-based subtitles
  are dropped.
* WebM keeps text subtitles, converted to WebVTT. Image-based subtitles are dropped.
* Other formats (such as TS and AVI) drop all subtitles.

Cover art and data streams are never copied.

## Control Speed and Output Detail

| Option | Effect |
|---|---|
| `-j, --jobs N` | Cut N clips at the same time (1–64). Default: half the processor cores, from 1 to 4. |
| `-v, --verbose` | More detail: FFmpeg commands in dry runs, FFmpeg's error output for failed clips, and rejected files for `scan`. |
| `-q, --quiet` | Only errors and the final summary. |
| `--ffmpeg PATH`, `--ffprobe PATH` | Use a specific FFmpeg/FFprobe instead of the bundled one or the one on `PATH`. |

The engine can also be set with the environment variables `RMEDIA_FFMPEG` and `RMEDIA_FFPROBE`.

## Cancel a Run

Press `Ctrl-C` once to cancel cleanly. rmedia prints:

```text
cancelling… (press Ctrl-C again to quit immediately)
```

It then stops the clips in progress, removes their temporary files, prints the summary and exits
with code `130`. Finished clips are kept.

Press `Ctrl-C` a second time to quit immediately. rmedia tries to remove temporary files first. Any it
cannot remove (for example, files that are still locked on Windows) are cleaned up the next time
rmedia runs.

`Ctrl-C` also cancels a running `scan`.

## Check the Exit Code

| Code | Meaning |
|---|---|
| `0` | Every clip succeeded or was skipped. |
| `1` | At least one clip failed or was cancelled. The others completed. |
| `2` | Usage error (for example, an unknown option, or `-l` together with `-s`). |
| `3` | Invalid input. Nothing was cut. |
| `4` | The video engine is missing, too old, or lacks an encoder that accurate mode needs. |
| `130` | The run was cancelled with `Ctrl-C`. |

A skipped clip (for example, because its output already exists) does not make the exit code
non-zero.

```bash
rmedia batch jobs.json || echo "rmedia exited with $?"
```

## Make Videos Phone-Ready

> **Availability:** `rmedia upscale` is in development (0.2.0-dev, branch `002-mobile-upscale`)
> and is not in the 0.1.0 packages. Smart reframe, GPU and AI quality tiers, upscaling from
> `clip` or `batch`, and the desktop upscale view are planned and not available. See the
> [Roadmap](../roadmap.md).

`rmedia upscale` scales each video to the screen size of a phone and re-encodes it, so it fills
the screen without black bars. Each video is processed whole. To upscale only a part of a video,
cut it first with `rmedia clip`.

```bash
# the default preset (phone-fill, 1080x2340)
rmedia upscale game.mp4

# a folder, for Reddit's full-screen player
rmedia upscale ~/Videos/clips --preset reddit-916 -o ~/Videos/reddit

# see what would happen, with an estimate of size and time
rmedia upscale game.mp4 --dry-run
```

Results are written to an `upscaled/` folder next to each source (or the `-o` folder), named
`{source}_{preset}.mp4`, for example `game_phone-fill.mp4`. Outputs are always `.mp4`. rmedia never
changes the source, and an existing output is skipped unless you pass `--on-exists`.

### Choose a Preset

```bash
rmedia upscale --list-presets
```

| Preset | Size | Codec | For |
|---|---|---|---|
| `phone-fill` (default) | 1080x2340 | HEVC | iPhone 15 to 18, Galaxy S22 to S26, Pixel 8 to 11 |
| `phone-fill-hd` | 1440x3120 | HEVC | 1440p phones |
| `pixel-tall` | 1080x2400 | HEVC | Pixel and tall 20:9 Android phones |
| `reddit-916` | 1080x1920 | H.264 | Reddit full-screen player |
| `reddit-feed-45` | 1080x1350 | H.264 | Reddit feed |
| `fold-inner` | 1080x1200 | HEVC | Foldable inner screens |
| `duo` | 1080x1566 | HEVC | iPhone Duo |

You can add your own presets as JSON files in the `presets` folder of the config directory (see
[Configuration](../administration/configuration.md#file-locations)). `--list-presets` prints a
warning for any file it ignores.

### Choose How the Picture Fills the Screen

| `--reframe` | Result |
|---|---|
| `blur` | The whole frame, centred over a blurred copy of itself |
| `center` | The middle of the frame, cropped to fit |
| `smart` (default) | Planned: follow the subject. Today it behaves like `blur` and prints a warning |

### Other Options

- `--hdr sdr` (default) converts HDR videos to standard range. `--hdr keep` keeps HDR on presets that
  support it; it stops with an error on other presets. HDR mastering metadata is not carried over.
- `--crf N` overrides the preset's quality (lower is better and bigger).
- `--tier auto|cpu` selects the quality tier. Only the CPU tier exists today; `gpu` and `ai` stop
  with `the gpu tier is not available on this machine`.
- Folder scanning options, `-o`, `--name` (also `{preset}`), `--on-exists`, `--no-audio`,
  `--audio`, `--no-subs`, `-j`, `--dry-run` and `--json` work as for `clip`. There is no `--mode`.

### Warnings

- Reddit presets warn when the predicted size or the duration would exceed Reddit's limit.
- rmedia warns when the output folder has less free space than the outputs need.
- A source already at least as big as the preset is not enlarged.

Press Ctrl-C to cancel; no partial file is left. Details:
[CLI Reference](../api/cli-reference.md#rmedia-upscale).

## Check the Video Engine

```bash
rmedia doctor
rmedia doctor --json
```

`doctor` shows which FFmpeg and FFprobe programs rmedia uses and where they came from, their
version, whether the encoders that accurate mode and upscaling need are present (including
`libx265`), whether the filters that upscaling needs are present (`filters:` line), and which
formats accurate mode can write. It exits with code `0` when the status is `ready` and `4` when it is not. rmedia looks for
the engine in this order:

1. `--ffmpeg` / `--ffprobe`;
2. the `RMEDIA_FFMPEG` / `RMEDIA_FFPROBE` environment variables;
3. the bundled `rmedia-ffmpeg` / `rmedia-ffprobe` next to the `rmedia` program;
4. `ffmpeg` / `ffprobe` on your `PATH`.

FFmpeg 6.0 or newer is required.

## Troubleshooting

| Symptom | See |
|---|---|
| Exit code `4`, `… not found: not bundled with rmedia and not on PATH` | [Video engine missing or too old](troubleshooting.md#video-engine-missing-or-too-old) |
| The clip starts before the time you chose | [The clip starts before the time I chose](troubleshooting.md#the-clip-starts-before-the-time-i-chose-fast-mode) |
| `no keyframe found before …` | [No keyframe found](troubleshooting.md#no-keyframe-found-before--use---mode-accurate-for-this-clip) |
| `skipped: output already exists` | [Output already exists](troubleshooting.md#skipped-output-already-exists) |
| `no input videos (folders contained no videos)` | [No input videos](troubleshooting.md#no-input-videos-folders-contained-no-videos) |
| `smart reframe is not available yet`, `the gpu tier is not available`, or other `rmedia upscale` messages | [Upscaling](troubleshooting.md#upscaling) |
| Warnings about subtitles, high bit depth, or `.mp4` instead | [Warnings](troubleshooting.md#warnings-that-do-not-stop-the-clip) |

## Related Information

* [Getting Started](getting-started.md)
* [Desktop App Guide](desktop-guide.md)
* [Troubleshooting](troubleshooting.md)
* [CLI Reference](../api/cli-reference.md)
* [File Formats Reference](../api/file-formats.md)
* [JSON Output Reference](../api/json-output.md)
