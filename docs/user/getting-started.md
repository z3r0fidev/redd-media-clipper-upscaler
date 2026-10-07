# Getting Started with rmedia

## Overview

rmedia cuts clips from video files on Linux and Windows. You can use it in two ways:

* **The desktop app** — open a video, mark where a clip starts and ends, and add it to a queue.
* **The `rmedia` command-line tool** — cut one clip, many clips from a list, or a batch of videos
  from a manifest file.

Both use the same engine and the same rules, so a clip made in the app and the same clip made on
the command line come out the same.

rmedia never changes your source videos, and it never overwrites an existing file unless you ask it
to.

This guide covers installation, your first clip in each tool, and the difference between **fast**
and **accurate** mode.

## Before You Begin

* A 64-bit (x86-64) computer running Windows 10/11 or Linux. The package names below use `amd64`/`x64`.
* Permission to install software for your user account. On Debian/Ubuntu, installing a `.deb`
  needs `sudo`.
* A video file to practise on.

You do not need to install FFmpeg separately. Every rmedia package includes its own copy, named
`rmedia-ffmpeg` and `rmedia-ffprobe` so that it never conflicts with an FFmpeg already on your
system. rmedia calls this copy the **video engine**.

## Install rmedia

Choose the package for your platform. Replace `<version>` with the version you downloaded.

| Platform | Package | Install |
|---|---|---|
| Debian / Ubuntu | `rmedia_<version>_amd64.deb` | `sudo apt install ./rmedia_<version>_amd64.deb` |
| Other Linux | `rmedia_<version>_amd64.AppImage` | `chmod +x rmedia_<version>_amd64.AppImage` and then run the file |
| Windows 10/11 | `rmedia_<version>_x64-setup.exe` (installs for your user only) or the `.msi` | Run the installer |
| Command line only (any platform) | `rmedia-cli-<version>-<platform>` archive | Unpack it anywhere |

> **Unverified:** The package names and install commands come from the project `README.md` and
> the bundle settings in `apps/desktop/src-tauri/tauri.conf.json`. This guide was written from the
> source code, not tested against a published release.

### Where the command-line tool ends up

Every desktop package also contains the `rmedia` command-line tool:

* **Debian/Ubuntu (`.deb`):** `rmedia` is on your `PATH` (in `/usr/bin`). The package also
  installs the GStreamer plugins the app uses to play video (`gstreamer1.0-plugins-base`,
  `gstreamer1.0-plugins-good`, `gstreamer1.0-libav`).
* **Windows:** `rmedia` is in the install folder (`%LOCALAPPDATA%\rmedia\` for the per-user
  setup). Add that folder to your `PATH`, or use the command-line archive instead.
* **AppImage:** the command-line tool stays inside the AppImage and is not available on your
  `PATH`. Download the command-line archive if you want to use `rmedia` in a terminal.

The Windows setup downloads Microsoft WebView2 if your computer does not have it. WebView2 comes
with Windows 11 and current versions of Windows 10.

> **Unverified:** The README says the setup downloads WebView2. The bundle configuration does not
> set this option explicitly; the behaviour comes from the Tauri installer's default. The source
> does not say whether the `.msi` does the same.

### Check the installation

Open a terminal and run:

```bash
rmedia doctor
```

Example output. This was captured on a development machine where rmedia used the system FFmpeg, so
it says `(path)`. An installed package normally says `(bundled)` and shows the bundled paths and
version.

```text
ffmpeg:   /usr/bin/ffmpeg (path)
ffprobe:  /usr/bin/ffprobe (path)
version:  6.1.1-3ubuntu5
encoder:  aac         yes
encoder:  libopus     yes
encoder:  libvpx-vp9  yes
encoder:  libx264     yes
accurate mode can write: mp4, m4v, mov, mkv, ts, webm
status:   ready
```

`status: ready` means rmedia found a video engine it can use. The word in brackets says where the
engine came from:

* `bundled`: the copy installed with rmedia.
* `config`: a path you set yourself.
* `path`: an FFmpeg found on your system `PATH`.

If you see `status: NOT READY`, see
[Video engine missing or too old](troubleshooting.md#video-engine-missing-or-too-old).

In the desktop app, the same information is in **Settings**, in the **Video engine** section.

## Make Your First Clip in the Desktop App

1. Start rmedia.
2. Select **Add videos** and choose a video file. You can also drag a file anywhere onto the window.
3. The video opens in the player. Press `Space` to play it, or drag along the timeline to find the
   moment where your clip should start.
4. Press `I` to set the **In** point (the start of the clip).
5. Move to where the clip should end and press `O` to set the **Out** point. If you skip this step,
   the clip runs to the end of the video.
6. Optional: type a name in **Clip name (optional)**.
7. Press `Enter`, or select **Add clip**. The clip appears in the **Queue** on the right.
8. Select **Run 1 clip** at the bottom of the queue.

### Verify the result

A run panel appears at the bottom of the window and shows **Finished** when the work is done, plus
a summary such as **1 succeeded**. Unless you chose a different output folder in **Settings**, the
clip is saved in a folder called `clips` next to the source video. Its file name looks like this:

```text
match12_001_opening goal.mp4
match12_001_00-01-30.000_00-02-00.000.mp4     (when the clip has no name)
```

For the full desktop workflow, see the [Desktop App Guide](desktop-guide.md).

## Make Your First Clip on the Command Line

1. Open a terminal in the folder that contains your video.
2. Cut the part from 1 minute 30 seconds to 2 minutes:

   ```bash
   rmedia clip game.mkv -s 1:30 -e 2:00
   ```

3. Or cut 15 seconds starting at 90 seconds, in accurate mode, into a folder called `out`:

   ```bash
   rmedia clip game.mkv -s 90 -d 15 --mode accurate -o out/
   ```

You can write times in several ways: `90`, `90.5`, `1:30`, `00:01:30.500`, `1m30s`, or `500ms`.

### Verify the result

rmedia prints one line per clip and a summary. This is real output from a fast-mode clip of a test
video (the folder path is shortened here):

```text
✓ …/clips/game_001_00-00-10.500_00-00-20.000.mp4  (actual 00:00:10.000 → 00:00:20.080, requested 00:00:10.500 → 00:00:20.000)
1 succeeded, 0 skipped, 0 failed, 0 cancelled
```

The `actual … requested …` note appears when the clip's real start or end differs from what you
asked for. The next section explains why this happens.

Run the same command again and rmedia skips the clip instead of overwriting it:

```text
- …/clips/game_001_00-00-10.500_00-00-20.000.mp4 skipped: output already exists
0 succeeded, 1 skipped, 0 failed, 0 cancelled
```

For more command-line tasks, see the [Command-Line Guide](cli-guide.md).

## Fast Mode and Accurate Mode

Every clip uses one of two modes. **Fast** is the default.

### Why two modes?

Compressed video stores a complete picture only every so often. That picture is called a
**keyframe**. The frames in between store only what changed since the keyframe. A clip that is
copied without re-encoding must begin on a keyframe, because the frames after it cannot be shown
without it.

### Fast mode

* Copies the video and audio without re-encoding them.
* Is very quick, and the quality is identical to the source.
* Starts on the **keyframe at or before** your start time. The clip can therefore begin a little
  earlier than you asked, often by a second or two. The end can also differ slightly.
* Tells you what it did. The command line prints the real start and end. The desktop app shows
  **Starts on the keyframe at … s** next to the clip. A dry run (`--dry-run`) shows the keyframe
  in advance, for example:

  ```text
  fast mode starts at the keyframe at 00:00:04.000 (00:00:01.000 early)
  ```

### Accurate mode

* Re-encodes the clip so that it starts and ends on the exact frames you chose.
* Takes longer, because every frame is encoded again.
* Uses high-quality settings: H.264 video with AAC audio for MP4, M4V, MOV, MKV and TS files, and
  VP9 video with Opus audio for WebM files.
* Writes an MP4 file instead if the source format cannot be re-encoded in place, for example AVI.
  rmedia warns you when this happens.
* Converts 10-bit and other high-bit-depth video, including HDR, to standard 8-bit video. rmedia
  warns you when this happens. Use fast mode to keep HDR.

### Which mode should I use?

| If you want… | Use |
|---|---|
| The quickest result, with no quality loss | **Fast** |
| A clip that starts on an exact frame (for example, a reaction that must start on cue) | **Accurate** |
| To keep HDR or 10-bit video | **Fast** |
| To change the format of an AVI, WMV, FLV or similar file | **Accurate** (it writes MP4) |

To choose a mode:

* **Command line:** `--mode fast` or `--mode accurate`.
* **Desktop app:** pick the default under **Settings → New clips → Mode**, or choose a mode for one
  video in its queue options.

## Troubleshooting

| Symptom | Cause | Resolution |
|---|---|---|
| `status: NOT READY` from `rmedia doctor` | rmedia cannot find a usable FFmpeg. | See [Video engine missing or too old](troubleshooting.md#video-engine-missing-or-too-old). |
| The clip starts earlier than you asked | This is how fast mode works. | Use accurate mode. See [The clip starts before the time I chose](troubleshooting.md#the-clip-starts-before-the-time-i-chose-fast-mode). |
| `skipped: output already exists` | A file with the same name already exists. | See [Output already exists](troubleshooting.md#skipped-output-already-exists). |
| The app shows **Preview copy** or **Still frames** | The built-in player cannot play this format directly. | Nothing is wrong. Clips are still cut from the original. See [Playback modes](troubleshooting.md#the-player-shows-preview-copy-or-still-frames). |
| `rmedia: command not found` after installing the AppImage | The AppImage does not put `rmedia` on your `PATH`. | Download the command-line archive. |

## Related Information

* [Desktop App Guide](desktop-guide.md)
* [Command-Line Guide](cli-guide.md)
* [Troubleshooting](troubleshooting.md)
* [CLI Reference](../api/cli-reference.md)
