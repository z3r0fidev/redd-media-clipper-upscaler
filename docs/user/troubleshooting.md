# Troubleshooting rmedia

Each entry below starts with the message or behaviour you see, then gives the cause and the fix.
Messages are quoted exactly as rmedia prints them. Parts in angle brackets, such as `<path>`, stand
for values that change.

On the command line, rmedia adds `error:` or `warning:` at the start of a message. When the problem
is in a particular file, the message includes its location: `command line`, a clip-list line
(`highlights.txt:4`), or a manifest entry (`jobs.json:videos[1].input`). In the desktop app, the
same messages appear in a pop-up message, in the **Fix these before running:** box, or next to a
clip in the run panel.

**Contents**

* [Video engine](#video-engine)
* [Desktop app: files and folders](#desktop-app-files-and-folders)
* [Desktop app: playback](#desktop-app-playback)
* [Clip timing](#clip-timing)
* [Output files](#output-files)
* [Warnings that do not stop the clip](#warnings-that-do-not-stop-the-clip)
* [Input errors (nothing was cut)](#input-errors-nothing-was-cut)
* [Clips that fail while cutting](#clips-that-fail-while-cutting)
* [Folders and scanning](#folders-and-scanning)
* [Installation](#installation)

---

## Video Engine

### Video engine missing or too old

You see one of these messages. On the command line the exit code is `4`, and `rmedia doctor` ends
with `status:   NOT READY`. In the desktop app, the top bar shows
**● Video engine missing — fix in Settings**.

```text
ffmpeg not found: not bundled with rmedia and not on PATH (use --ffmpeg PATH)
```

```text
ffprobe not found: not bundled with rmedia and not on PATH (use --ffprobe PATH)
```

```text
<path> does not exist
```

```text
ffmpeg <version> is too old; version 6.0 or newer is required
```

```text
<path> did not report an ffmpeg version
```

#### Cause

rmedia needs FFmpeg and FFprobe, version 6.0 or newer. It looks for them in this order:

1. `--ffmpeg` / `--ffprobe` options, or the engine chosen in the app's **Settings**;
2. the `RMEDIA_FFMPEG` / `RMEDIA_FFPROBE` environment variables;
3. `rmedia-ffmpeg` / `rmedia-ffprobe` in the same folder as the program;
4. `ffmpeg` / `ffprobe` on your `PATH`.

The messages mean:

* **`not found`**: none of these places had the program. For example, the bundled copy was removed,
  or you are running an `rmedia` binary on its own, without the files that come with it.
* **`<path> does not exist`**: a path given with `--ffmpeg`/`--ffprobe`, an environment variable,
  or **Settings** points to a missing file.
* **`too old`**: the FFmpeg that was found is older than 6.0. This usually happens when rmedia
  falls back to an old system FFmpeg.
* **`did not report an ffmpeg version`**: the file is not FFmpeg.

#### Resolution

1. Run `rmedia doctor` to see which programs rmedia found and where they came from.
2. Make sure `RMEDIA_FFMPEG` and `RMEDIA_FFPROBE` are not set to old or wrong paths.
3. In the desktop app, open **Settings**. If **Use automatic** is shown, select it to go back to the
   bundled engine.
4. If the bundled engine is missing, reinstall rmedia. For the command-line archive, keep
   `rmedia-ffmpeg` and `rmedia-ffprobe` in the same folder as `rmedia`.
5. Or point rmedia to FFmpeg 6.0 or newer: use `--ffmpeg <path> --ffprobe <path>`, or in the app use
   **Settings → Video engine → Choose ffmpeg…**.

#### Verify the fix

`rmedia doctor` ends with `status:   ready`, and the app no longer shows the red engine button.

### "the ffmpeg build lacks the `<encoder>` encoder needed for accurate mode"

```text
the ffmpeg build lacks the libx264 encoder needed for accurate mode
```

Or, from `rmedia doctor`:

```text
problem:  no encoders for accurate mode (fast mode still works)
```

#### Cause

Accurate mode re-encodes video. It needs `libx264` and `aac` for MP4, M4V, MOV, MKV and TS output,
and `libvpx-vp9` and `libopus` for WebM output. The FFmpeg in use does not include them. This
usually happens with a system FFmpeg or one you chose yourself, not with the bundled engine. On the
command line the exit code is `4`.

#### Resolution

1. Run `rmedia doctor` and look at the `encoder:` lines and the
   `accurate mode can write:` line.
2. Choose one:
   * Use fast mode for these clips.
   * Choose an output format that your FFmpeg can encode, with `--container` (for example
     `--container webm` if only the VP9 and Opus encoders are present).
   * Switch to the bundled engine or a full FFmpeg build (see the previous entry).

#### Verify the fix

`rmedia doctor` lists the format you need after `accurate mode can write:`.

### "choose the ffmpeg program (ffprobe must be in the same folder)"

```text
choose the ffmpeg program (ffprobe must be in the same folder)
```

```text
<path> was not found next to ffmpeg
```

#### Cause

In **Settings → Video engine → Choose ffmpeg…**, you chose a file whose name does not end in
`ffmpeg`, or there is no matching `ffprobe` program in the same folder.

#### Resolution

Choose the `ffmpeg` program itself (`ffmpeg.exe` on Windows), from a folder that also contains
`ffprobe`.

---

## Desktop App: Files and Folders

### "was not opened in rmedia (use Add files/folder or drag it in)"

```text
<path> was not opened in rmedia (use Add files/folder or drag it in)
```

#### Cause

For security, the desktop app only reads files that you chose yourself in this session:

* with **Add videos** or **Add folder** (including the videos found inside the folder);
* by dragging them onto the window;
* by passing them on the command line when the app starts;
* as videos listed in a manifest you opened with **Open manifest…**.

Any other path is refused. This can happen when a queue or manifest refers to a video that you did
not open in this way, or when a manifest lists a file that does not have a video file extension.

#### Resolution

1. Add the file or its folder with **Add videos**, **Add folder**, or by dragging it in.
2. Try the action again.

### "is outside the folders you chose; pick the output folder in Settings"

```text
<output path> is outside the folders you chose; pick the output folder in Settings
```

This appears in the **Fix these before running:** box, and nothing runs.

#### Cause

The desktop app writes clips only:

* next to the source video (the default `clips` folder, or sub-folders created by the file name
  pattern), or
* inside a folder you chose (with **Add folder** in this session, or **Choose…** in **Settings**; an
  output folder saved in **Settings** stays allowed after a restart).

The planned output is somewhere else. This usually happens when a manifest opened with
**Open manifest…** sets an `output_dir` that you have not chosen in the app.

#### Resolution

1. Open **Settings**.
2. Next to **Output folder**, select **Choose…** and pick the folder again (or select **Reset** to
   use the `clips` folder next to each video).
3. Select **Save**, and then select **Run** again.

If the folder comes from a manifest, you can also add that folder with **Add folder**.

### "open the clip list with Import list…" / "open the manifest with Open manifest…"

```text
open the clip list with Import list…
```

```text
open the manifest with Open manifest…
```

```text
choose a .json or .csv manifest
```

#### Cause

Clip lists and manifests can only be read when you open them with the matching button. A manifest
must end in `.json` or `.csv`.

#### Resolution

Use **Import list…** (in the **Queue** header) for clip lists, and **Open manifest…** (in the
**Sources** header) for manifests.

### "Select a source first"

You selected **Import list…** but no video is selected and the queue is empty. Add a video, select
it in **Sources**, and import the list again.

### "plan not found; plan the queue again"

```text
plan not found; plan the queue again
```

The run could not find the checked plan it was given, for example because it was already used.
Select **Run** again.

---

## Desktop App: Playback

### The player shows "Preview copy" or "Still frames"

The label next to the file name says **Preview copy** or **Still frames** instead of **Original**,
or the player shows:

```text
Preparing a preview copy for this format… <n>%
```

```text
Playback isn't available for this file here; showing exact still frames.
```

#### Cause

The app's player uses the system web engine: WebView2 on Windows and WebKitGTK on Linux. That engine
plays only some formats. rmedia plays the original file directly only for:

* MP4, M4V or MOV files with H.264 video and AAC audio (or no audio);
* WebM files with VP8, VP9 or AV1 video and Opus or Vorbis audio (or no audio).

For anything else, or if direct playback fails, rmedia makes a lightweight **preview copy**, first
in H.264/MP4 and then, if that cannot play, in VP8/WebM. If neither works, it shows exact **still
frames** that you can scrub through but not play.

On Linux, WebKitGTK uses GStreamer plugins to decode video. If the H.264 decoder plugin is missing,
H.264 files cannot play directly and the VP8 preview copy is used instead.

**Clips are always cut from the original file.** The playback mode does not affect the result.

#### Resolution

You do not need to do anything. If you want to improve playback:

1. On Debian/Ubuntu, make sure these packages are installed: `gstreamer1.0-plugins-base`,
   `gstreamer1.0-plugins-good`, `gstreamer1.0-libav`. The `.deb` package depends on them.
2. The AppImage includes its own GStreamer. If playback still fails there, try the `.deb` package
   on Debian/Ubuntu.
3. To avoid waiting for a preview copy, select **Use still frames instead**.
4. If preview copies are being made again and again, increase **Settings → Performance →
   Preview cache (MB)**. Older copies are removed when the cache is full.

> **Unverified:** The GStreamer package list comes from the `.deb` dependencies, and the AppImage
> setting `bundleMediaFramework` is in `tauri.conf.json`. Which plugins each Linux distribution
> needs was not tested.

### "Can't open this video"

The player shows **Can't open this video**, followed by a message, and a **Retry** button.

#### Cause

rmedia could not read the file or prepare it for preview. The message under the title gives the
reason: often one of the read errors listed under
[Clips that fail while cutting](#clips-that-fail-while-cutting), or
[… was not opened in rmedia](#was-not-opened-in-rmedia-use-add-filesfolder-or-drag-it-in).

#### Resolution

1. Read the message, and fix the problem it describes (for example, a missing or damaged file).
2. Select **Retry**.

---

## Clip Timing

### The clip starts before the time I chose (fast mode)

On the command line, the result line shows different actual and requested times:

```text
✓ <output>  (actual 00:00:10.000 → 00:00:20.080, requested 00:00:10.500 → 00:00:20.000)
```

A dry run shows:

```text
fast mode starts at the keyframe at 00:00:04.000 (00:00:01.000 early)
```

In the desktop app, the clip's row in the run panel shows **Starts on the keyframe at <n> s**.

#### Cause

This is expected in fast mode. Fast mode copies the video without re-encoding, so a clip must begin
on a keyframe (a complete picture stored in the video). rmedia uses the keyframe **at or before** your
start time. The end can also move slightly. See
[Fast mode and accurate mode](getting-started.md#fast-mode-and-accurate-mode).

rmedia reports the difference when the start or end moves by more than 0.05 seconds.

#### Resolution

* If the extra footage is acceptable, do nothing.
* For a frame-exact start and end, use accurate mode: `--mode accurate` on the command line, or
  **Frame-accurate** in the app (in **Settings** or the video's queue options).
* Run with `--dry-run` first to see where each fast-mode clip will start.

#### Verify the fix

In accurate mode the result line shows no `actual … requested …` note.

### "no keyframe found before …; use --mode accurate for this clip"

```text
no keyframe found before 00:00:01.000; use --mode accurate for this clip
```

#### Cause

Fast mode could not find a keyframe at or before the start time, so it cannot copy the clip without
re-encoding. Nothing is cut (exit code `3`).

#### Resolution

Cut this clip in accurate mode (`--mode accurate`, or **Frame-accurate** in the app).

---

## Output Files

### "skipped: output already exists"

```text
- <output> skipped: output already exists
```

In the desktop app, the clip shows **Skipped** with `output already exists`. A dry run shows
`would skip (output already exists)`.

#### Cause

A file with the planned name already exists. By default rmedia never overwrites an existing file,
so it skips the clip. This often happens when you run the same job twice. A skipped clip does not
count as a failure, so the exit code stays `0`.

A related message, `output appeared while the clip was being made`, means another program created
the file while rmedia was cutting. rmedia skipped the clip rather than overwrite that file.

#### Resolution

Choose one:

* Keep the existing file. Nothing needs to be done.
* Save the new clip under a numbered name: `--on-exists rename`, or **Settings → If the file
  exists → Add a number**.
* Replace the existing file: `--on-exists overwrite`, or **Replace it** in **Settings**. rmedia
  warns `existing output will be replaced`.
* Use a different output folder (`-o`) or file name pattern (`--name`).

### "two clips would write …"

```text
two clips would write <path>; use a name template with {index} or --on-exists rename
```

#### Cause

Two clips in the same run would get the same file name, for example because the `--name` pattern
does not include `{index}`, a time, or a clip name. Nothing is cut.

#### Resolution

Add `{index}`, `{index:03}`, `{range}` or `{name-or-range}` to the file name pattern, or use
`--on-exists rename`.

### "output … would overwrite a source video"

```text
output <path> would overwrite a source video
```

#### Cause

The planned output path is one of the source videos in this run. rmedia never writes over a source
file. Nothing is cut.

#### Resolution

Use a different output folder or file name pattern.

### No partial files after a cancelled or failed run

This is expected. Clips are written to hidden temporary files (named like
`.<name>.rmedia-<id>-<n>.part`) and are given their final name only after they have been checked. A
cancelled or failed clip leaves nothing behind. If rmedia was stopped suddenly (for example, by a
second `Ctrl-C` or a power cut) and a `.part` file remains, the next run removes it.

---

## Warnings That Do Not Stop the Clip

These warnings appear in a dry run, before the run starts (command line), and next to the clip in
the app. The clip is still cut.

### "accurate mode cannot write .… ; writing .mp4 instead"

```text
accurate mode cannot write .avi; writing .mp4 instead (choose --container to override)
```

**Cause:** Accurate mode can re-encode only into MP4, M4V, MOV, MKV, TS and WebM. For other source
formats (for example AVI, WMV, FLV, MPEG), it writes MP4.

**Resolution:** Nothing is required. To choose the output format yourself, use `--container`, for
example `--container mkv`. To keep the original format, use fast mode.

### "unknown container .…; writing .mkv"

```text
unknown container .<ext>; writing .mkv
```

**Cause:** rmedia does not recognise the source format's extension, so it writes MKV. If you ask
for an unsupported format with `--container`, rmedia instead stops with
`unsupported output container .<ext>`.

**Resolution:** Nothing is required, or choose a supported format with `--container` (for example
`mp4`, `mkv`, `mov`, `webm` or `ts`).

### "source is high bit depth …"

```text
source is high bit depth (yuv420p10le); accurate mode writes 8-bit 4:2:0 (HDR is not preserved — use fast mode to keep it)
```

**Cause:** The source uses 10-bit or higher video (often HDR). Accurate mode always writes standard
8-bit video, so colours and brightness can look different from the HDR original.

**Resolution:** Use fast mode to keep the original video exactly.

### Subtitles dropped

```text
1 subtitle stream(s) cannot be stored in .ts; dropped
```

```text
subtitle stream <n> (<codec>) cannot be stored in .mp4; dropped
```

```text
subtitles cannot be cut frame-accurately from this kind of source; dropped
```

**Cause:**

* The output format cannot store subtitles at all (for example TS or AVI), or cannot store this
  kind of subtitle. MP4, M4V, MOV and WebM keep only text subtitles. Image-based subtitles (such as
  DVD or Blu-ray subtitles) are dropped.
* In accurate mode, sources whose format does not support exact seeking (for example TS, AVI or
  MPEG files) are cut in a way that cannot include subtitles.

**Resolution:** To keep all subtitles, write MKV (`--container mkv`), which keeps every kind. For
the accurate-mode case, use fast mode if you need the subtitles. To silence the warning, remove
subtitles on purpose with `--no-subs` (or **No subtitles** in the app).

### "end … is past the end of the video"

```text
end 00:01:30.000 is past the end of the video; clip ends at 00:01:00.000
```

**Cause:** The end time is later than the video's length. The clip runs to the end of the video.

**Resolution:** Nothing is required. Use `end` as the end time if you mean the end of the video.

### "existing output will be replaced"

You chose `--on-exists overwrite` (or **Replace it**), and the output file exists. It will be
replaced when the new clip is complete.

---

## Input Errors (Nothing Was Cut)

On the command line, all of these stop the run before anything is cut, with exit code `3`. rmedia
reports every problem it finds at once, so you can fix them all before running again. In the app
they appear in the **Fix these before running:** box.

| Message | Cause | Resolution |
|---|---|---|
| `invalid time "<text>": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s` | A time is not in a format rmedia understands. | Use a format such as `90`, `1:30`, `00:01:30.500` or `1m30s`. See [Write times](cli-guide.md#write-times). |
| `invalid time "<text>": negative times are not allowed` | A time starts with `-`. | Use `0` or a positive time. |
| `missing end after "<start>" (use END, +DURATION or end)` | A clip-list line has a start but no end. | Add an end time, `+DURATION`, or `end`. |
| `the clip list contains no clips` | The clip list is empty or has only comments. | Add at least one clip line. |
| `not valid UTF-8 text` | The clip list is not saved as UTF-8. | Save the file as UTF-8. |
| `start (<time>) is at or beyond the end of the video (<length>)` | The start time is after the video ends. | Use an earlier start time. |
| `end (<time>) is not after start (<time>)` | The end is at or before the start. | Use a later end time. |
| `duration must be greater than zero` | The duration is `0`. | Use a positive duration. |
| `the video's duration is unknown; give an explicit end time` | rmedia cannot tell how long the video is, so `end` cannot be used. | Give an explicit end time or duration. |
| `audio track <n> does not exist (source has <count>)` | `--audio` names a track the video does not have. | Run `rmedia scan <file>` or check the audio list in the app, and use an existing track number. |
| `unsupported output container .<ext>` | `--container` names a format rmedia does not support. | Use a supported format such as `mp4`, `mkv`, `mov`, `webm` or `ts`. |
| `unknown placeholder {<name>} in name template "<template>"` | The `--name` pattern uses a placeholder that does not exist. | See [Name output files](cli-guide.md#name-output-files). |
| `name template "<template>" must produce a relative path without empty, '.' or '..' parts` | The pattern starts with `/`, contains `//`, or contains `.` or `..` parts. | Use a relative pattern such as `{source}/{index:03}.{ext}`. |
| `<path> is a playlist/stream description, not a video file` | The input is an `.m3u8`, `.m3u`, `.ffconcat` or `.sdp` file. | Use the video file itself. |
| `<path> has no video stream` | The file has only audio (or only cover art). | Use a video file. |
| `source not found: <path>` | The input file does not exist. In a batch manifest, relative paths are relative to the manifest's folder. | Correct the path. For batches, add `--continue-on-missing` to skip missing videos. |
| `<path>: manifest must be a .json or .csv file` | `rmedia batch` was given another kind of file. | Use a `.json` or `.csv` manifest. |
| `none of the manifest's videos exist` | With `--continue-on-missing`, every video was missing. | Check the paths in the manifest. |

A clip-list error is reported with the file and line number, for example:

```text
error: /home/me/highlights.txt:4: invalid time "1:7x": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s
```

> **Note:** Problems with the clip list file itself (bad times, missing ends) are reported first.
> Problems that need the video (such as an end before the start, or a start past the end of the
> video) are reported after the list has been read successfully.

---

## Clips That Fail While Cutting

A failed clip is marked `✗ <output> failed: <message>` on the command line, or **Failed** in the
app. The other clips continue, and the command-line exit code is `1`.

| Message | Cause | Resolution |
|---|---|---|
| `not enough disk space writing <path>` | The output drive is full. | Free up space or choose another output folder, and run again. |
| `cannot write output <path>: <reason>` | rmedia cannot create or write the output file or folder, for example because of permissions or a read-only drive. | Choose a folder you can write to (`-o`, or **Settings → Output folder**). |
| `cannot read media <path>: <reason>` | The video engine could not read the source. It may be damaged, incomplete, or not a video. `ffprobe timed out` means reading took more than 60 seconds. | Check that the file plays in another player. If the file is on a slow network drive, copy it locally first. |
| `ffmpeg failed: <message>` | FFmpeg reported an error while cutting. The message is the most specific line of FFmpeg's output. | Run with `-v` to see FFmpeg's full error output. Try accurate mode, or a different `--container`. |
| `output verification failed: the clip's video lasts <n> s but <n> s was requested; the source may be damaged or its timestamps unreliable` | In accurate mode, the finished clip was much shorter or longer than requested. rmedia discarded it. | The source may have damaged or unusual timestamps. Try fast mode, or re-save the source with another tool first. |
| `output verification failed: expected <n> streams, found <n>` or `output has no duration` | The finished clip did not pass rmedia's checks and was discarded. | Run with `-v` and check the source. Try a different `--container`. |

---

## Folders and Scanning

### "no input videos (folders contained no videos)"

```text
no input videos (folders contained no videos)
```

#### Cause

The folders you gave to `rmedia clip` contain no videos that rmedia accepts. Possible reasons:

* The files do not have a video file extension, or the video engine cannot read them. Run
  `rmedia -v scan <folder>` to list rejected files and the reasons.
* `--include`, `--exclude`, `--no-recursive` or `--max-depth` filtered them out.
* The files are hidden (add `--hidden`), symbolic links (add `--follow-symlinks`), or partial
  downloads.
* The videos are in a folder named `clips`, which `clip` skips when you do not use `-o`. rmedia
  prints a `note: skipped … folder(s) named "clips"` message in that case. Pass that folder
  directly to use its videos.

#### Resolution

Run `rmedia scan <folder>` with the same options. Its summary line shows how many files were
skipped and why, for example:

```text
2 video(s), 1 rejected (-v lists them); skipped 1 hidden, 1 temp, 0 symlink, 0 excluded, 0 online-only (0.0 s)
```

### "the output folder … is also a scanned input folder"

```text
the output folder <dir> is also a scanned input folder; clips would be mixed with the sources and picked up again. Use a sub-folder, e.g. -o <dir>/clips
```

#### Cause

With `rmedia clip <folder> -o <folder>`, the clips would be written into the folder being searched,
and would be picked up as sources on the next run.

#### Resolution

Use a sub-folder or a different folder for `-o`, as the message suggests.

### Cloud files are skipped (Windows)

The scan summary shows a non-zero `online-only` count, or videos you expect in a OneDrive (or
similar) folder are missing.

#### Cause

On Windows, rmedia skips files that are stored only online, because reading them would download
them.

#### Resolution

Make the files available offline in your cloud storage app (for example, **Always keep on this
device** in OneDrive), and then scan again.

> **Unverified:** The OneDrive menu wording is not from the rmedia source.

### "invalid pattern …"

```text
invalid pattern "<glob>": <reason>
```

An `--include` or `--exclude` pattern is not a valid glob. Check the brackets and braces in the
pattern, and quote it so that your shell does not change it.

### "skipping a path that cannot be listed one-per-line"

```text
warning: skipping a path that cannot be listed one-per-line: <path>
```

`rmedia scan --paths` found a file whose name contains a line break or is not valid UTF-8. That
name cannot be passed safely through `--inputs-from`, so it is left out. Rename the file, or pass it
to `rmedia clip` directly.

---

## Installation

### `rmedia` command not found

* **AppImage:** the command-line tool is inside the AppImage and is not on your `PATH`. Download the
  `rmedia-cli-<version>-<platform>` archive and unpack it.
* **Windows:** `rmedia.exe` is in the install folder (`%LOCALAPPDATA%\rmedia\` for the per-user
  setup). Add that folder to your `PATH`, or use the command-line archive.
* **Debian/Ubuntu:** the `.deb` puts `rmedia` in `/usr/bin`. Check that the package is installed.

### The desktop app does not start on Windows

#### Cause

The desktop app needs Microsoft WebView2. It comes with Windows 11 and current versions of
Windows 10. The setup program is expected to download it if it is missing.

#### Resolution

Install the Microsoft Edge WebView2 Runtime from Microsoft, and then start rmedia again. The
command-line tool does not need WebView2.

> **Unverified:** WebView2 download behaviour comes from the README and Tauri's installer defaults.
> Whether the `.msi` installs WebView2 was not checked. No rmedia error message for this case
> exists in the source.

### The AppImage shows rendering problems or does not start

> **Unverified:** The project research notes (`research/04-app-stack-and-packaging.md`) list known
> AppImage issues with GPU, Wayland and EGL rendering. No workaround is defined in the rmedia source.
> On Debian/Ubuntu, try the `.deb` package instead.

---

## Still Stuck?

When you report a problem, include:

* the output of `rmedia doctor`;
* the full command and its output, run with `-v` (for example `rmedia -v clip …`);
* for the desktop app, the exact message text shown.

## Related Information

* [Getting Started](getting-started.md)
* [Desktop App Guide](desktop-guide.md)
* [Command-Line Guide](cli-guide.md)
* [CLI Reference](../api/cli-reference.md)
