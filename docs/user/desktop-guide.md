# Desktop App Guide

## Overview

The rmedia desktop app lets you choose videos, find the moments you want in a built-in player, mark
clips with the keyboard, and cut them all in one run. It can also make a video ready for a phone
held upright: see [Upscale for Phones](#upscale-for-phones). Use it when you want to watch the footage
while you choose clips. Use the [command-line tool](cli-guide.md) when you already know the times
or want to automate the work.

The window has three panes and a run panel:

| Area | Position | What it does |
|---|---|---|
| **Sources** | Left | The videos you have opened. |
| **Player** | Centre | Plays the selected source. The **Clip** and **Upscale for phone** buttons at its top switch between marking clips and upscaling. |
| **Queue** | Right | The clips waiting to be cut, with per-video options. |
| **Run panel** | Bottom (appears during a run) | Progress, results and cancel buttons. |

**Settings** is in the top-right corner of the window.

## Before You Begin

* rmedia is installed. See [Getting Started](getting-started.md).
* The video engine is working. If the top bar shows **● Video engine missing — fix in Settings**,
  fix that first. See [Video engine missing or too old](troubleshooting.md#video-engine-missing-or-too-old).

## Add Source Videos

For security, the app works only with files and folders that **you** have chosen. A source can be
chosen in any of the following ways.

### Add files

1. Select **Add videos** in the **Sources** pane, or in the empty player.
2. Choose one or more video files in the file dialog.

Each file appears in the list. It shows **reading…** until rmedia has read its length and size, and
then shows the duration and resolution, for example `00:01:00.000 · 1280×720`. If a file cannot be
read, the reason appears in its place.

### Add a folder

1. Select **Add folder**.
2. Choose a folder.

rmedia searches the folder and all of its sub-folders. Each candidate file is checked with the video
engine, and only real videos are added. While it searches, the pane shows a progress message, for
example **Checked 12 of 40 candidate files**, and a **Cancel** button. When it finishes, a message
such as **Found 8 video(s) in Matches, 2 skipped (not playable video)** appears.

The search skips:

* files that do not have a video file extension;
* files the video engine cannot read, files with no video stream (for example audio-only files),
  and still images;
* hidden files and folders;
* partial downloads and temporary files (such as `.part`, `.crdownload`, `.tmp`);
* system, trash and thumbnail folders (for example `$RECYCLE.BIN`, `.Trashes`, `@eaDir`);
* symbolic links;
* on Windows, online-only cloud files (such as OneDrive files that are not downloaded), because
  reading them would download them.

### Drag and drop

Drag video files or folders from your file manager onto any part of the rmedia window. Files are
added as sources, and folders are searched as described above.

### Open videos from the command line

Pass files or folders when you start the app:

```bash
rmedia-desktop video.mp4 ~/Videos/matches
```

Paths that do not exist are ignored.

> **Unverified:** `rmedia-desktop` is the program name in the project build files and README. The
> name of the installed launcher on each platform has not been verified.

### Open a batch manifest

Select **Open manifest…** in the **Sources** pane header and choose a `.json` or `.csv` manifest
(the same files that `rmedia batch` runs; see the [Command-Line Guide](cli-guide.md#run-a-batch-manifest)).
The videos in the manifest are added as sources, and their clips and options are added to the
queue. A message such as **Loaded 3 video(s) from jobs.json** confirms it.

### Remove a source

Point at a source and select **×**. This removes it from the list only. The file is not deleted.

## Use the Player

Select a source in the list to open it in the player.

### Playback modes

Your system's built-in web player cannot play every video format. rmedia picks the best option
automatically, and a label next to the file name shows which one is in use:

| Label | What you see | When rmedia uses it |
|---|---|---|
| **Original** | The source file itself plays. | MP4, M4V or MOV files with H.264 video and AAC audio, and WebM files with VP8, VP9 or AV1 video and Opus or Vorbis audio. |
| **Preview copy** | A lightweight copy of the video plays. | The format cannot be played directly (for example MKV, TS, or HEVC video), or direct playback failed. rmedia first makes an H.264 copy, and if that cannot play either, a VP8/WebM copy. |
| **Still frames** | Exact single frames are shown instead of playback. You can scrub, but not play. | Neither preview copy can be played or made. |

Important points:

* **Clips are always cut from the original file**, whichever label is shown. The preview copy is
  only for watching.
* While a preview copy is being made, the player shows **Preparing a preview copy for this format…**
  with a percentage. To skip the wait, select **Use still frames instead**.
* Preview copies are kept in a cache, so a video opens faster the next time. You can set the cache
  size in [Settings](#change-settings).
* In **Still frames** mode the player shows: **Playback isn't available for this file here; showing
  exact still frames.**

### Move around the video

* Drag along the timeline (the strip of thumbnails) to scrub. Point at the timeline to see the time
  under the pointer.
* Use the play button and the previous/next-frame buttons below the video.
* Use the keyboard shortcuts below.

### Keyboard shortcuts

Shortcuts work when the player has a video open and you are not typing in a text box or list.

| Key | Action |
|---|---|
| `Space` | Play or pause |
| `I` | Set the **In** point (clip start) at the current time |
| `O` | Set the **Out** point (clip end) at the current time |
| `Enter` or `A` | Add the marked clip to the queue |
| `←` or `,` | Previous frame |
| `→` or `.` | Next frame |
| `Shift` + `←` / `→` | Back or forward 1 second |
| `Home` | Go to the start |
| `End` | Go to the end |

## Mark Clips

1. Move to the first frame of the clip and press `I`, or select the **In** button.
2. Move to the end of the clip and press `O`, or select the **Out** button.
   * If you do not set an Out point, the **Out** button shows **end**, and the clip runs to the end
     of the video.
   * The Out point must be after the In point. If you set a new In point after the current Out
     point, the Out point is cleared.
3. Optional: type a name in **Clip name (optional)**. The name is used in the output file name.
4. Press `Enter`, or select **Add clip**.

A message such as **Added 00:00:05.000 → 00:00:10.000 to the queue** confirms it. The player then
gets ready for the next clip: the new In point is the previous Out point, and the Out point and name
are cleared. This lets you cut a video into back-to-back clips quickly.

**Add clip** is unavailable until an In point is set.

## Work With the Queue

The queue lists each video that has clips, with one row per clip. The header shows the total
number of clips.

If you have more than one source, **Add all to queue** (at the bottom of the **Sources** pane) adds
every readable source to the queue so that you can import a clip list for all of them.

### Edit start and end times

Each clip row has a **Start** box and an **End** box. Select a box, type a new value, and then
press `Tab` or select somewhere else to apply it.

* **Start** accepts a time such as `90`, `1:30`, `00:01:30.500` or `1m30s`.
* **End** accepts any of these:
  * a time, for example `2:00`: the clip ends at that point;
  * `+` and a duration, for example `+15`: the clip lasts that long;
  * `end`: the clip runs to the end of the video. An empty box also means `end`.

If the text is not valid, the box is highlighted, and pointing at it shows the reason. The queue
keeps the previous value until you enter a valid one.

The length of each clip is shown after the End box. You can also type or change a clip **name**,
and select **×** on a row to remove that clip.

### Choose options for one video

Each video in the queue has a summary line, for example **Default mode · all audio · subtitles**.
Select it to show three lists:

| Option | Choices | Notes |
|---|---|---|
| Mode | **Default mode**, **Fast (no re-encode)**, **Frame-accurate** | **Default mode** uses the mode set in [Settings](#change-settings). See [Fast mode and accurate mode](getting-started.md#fast-mode-and-accurate-mode). |
| Audio | **All audio**, **No audio**, or one track, for example **Track 2 (eng) · aac** | Lets you keep only one audio track, such as a commentary track. To keep several specific tracks, use the command line (`--audio 1,2`). |
| Subtitles | **Keep subtitles**, **No subtitles** | Some output formats cannot store some subtitle types. rmedia warns you when subtitles are dropped. |

These options apply to every clip of that video. Select **×** next to the video name to remove the
video and all its clips from the queue.

### Import a clip list

A clip list is a text file with one clip per line. For the format, see
[Use a clip list](cli-guide.md#cut-many-clips-from-a-clip-list).

1. Select the source the list belongs to.
2. Select **Import list…** in the **Queue** header.
3. Choose a `.txt` file.

The clips are added to the selected source. If no source is selected, the clips are added to every
video in the queue. If the list has errors, a message shows the file, the line number and the
problem, and nothing is imported.

### Export the queue

Select **Export…** to save the queue as a batch manifest (default name `rmedia-queue.json`). Choose
a `.json` or `.csv` file name. The confirmation message tells you how to run it, for example:

```text
Saved rmedia-queue.json — run it with: rmedia batch "rmedia-queue.json"
```

Use this to repeat the same job later, run it on another computer, or automate it. **Export…** is
unavailable while the queue is empty.

> **Note:** A `.json` export also includes the defaults from **Settings** › **New clips** (mode,
> output folder, file names, and what to do if a file exists) in the manifest's `defaults`. Values
> set in the queue win over the Settings defaults. `rmedia batch` therefore cuts the same clips the
> app would. A `.csv` export stores only each clip's mode and output folder; file names, what to
> do if a file exists, and audio and subtitle choices are lost, so use `.json` to keep them.
> **Clips at once** is not exported; pass `-j` to `rmedia batch` if you need it.

## Run the Queue

1. Select **Run N clips** at the bottom of the queue.
2. rmedia checks every clip before it cuts anything. If there are problems, a box titled
   **Fix these before running:** lists them, and nothing runs. Correct the clips and select
   **Run** again.
3. Follow the progress in the run panel. While the clips are being cut, it shows:
   * the overall progress, as **N / M clips**, with a progress bar and time remaining;
   * a list of clips (select the arrow button to show or hide it), each with a status: **Waiting**,
     **Cutting**, **Done**, **Skipped**, **Failed** or **Cancelled**.

The clip list opens automatically if a clip fails, and shows the reason next to it.

### Cancel

* To stop one clip, select **Cancel** on its row while it is being cut.
* To stop everything, select **Cancel all**.
* Closing the window also cancels any running work.

A cancelled clip leaves no temporary file behind. Clips that were already finished are kept.

### Read the results

When the run ends, the panel shows **Finished** and a summary such as **3 succeeded · 1 skipped**.
Next to each clip it shows:

* **Starts on the keyframe at … s** for a fast-mode clip that starts earlier than the time you chose;
* the reason for a skipped clip, for example `output already exists`;
* the error for a failed clip;
* any warnings, for example a dropped subtitle stream.

Select **Close** to hide the panel.

## Find Your Clips

### Where clips are saved

* By default, in a folder called **`clips` next to each source video**. The queue footer shows the
  current setting, for example **Output: “clips” folder next to each video**.
* If you choose an output folder in [Settings](#change-settings), all clips go there. The folder
  stays usable after you restart the app. A new folder must be picked with **Choose…**; otherwise
  **Save** shows `choose the output folder with Choose…`.

rmedia creates the folder if needed. It can write only next to the source video or inside a folder
you chose. See ["… is outside the folders you chose"](troubleshooting.md#is-outside-the-folders-you-chose-pick-the-output-folder-in-settings).

### File names

The default name is `{source}_{index:03}_{name-or-range}.{ext}`:

| Part | Meaning | Example |
|---|---|---|
| `{source}` | Source file name without its extension | `match12` |
| `{index:03}` | Number of the clip within its source, with 3 digits | `001` |
| `{name-or-range}` | The clip name, or the start and end times if the clip has no name | `opening goal` or `00-01-30.000_00-02-00.000` |
| `{ext}` | File extension: the same as the source, or `mp4` when accurate mode cannot write the source format | `mkv` |

For example, `match12_001_opening goal.mkv` or `match12_002_00-03-15.250_00-03-35.250.mkv`.

Characters that are not allowed in file names on Windows or Linux are replaced with `_`. To change
the pattern, see the **File names** setting and the
[naming placeholders](cli-guide.md#name-output-files).

### What else rmedia writes

* **No changes to your source files.** rmedia never modifies, moves or deletes them.
* **Temporary files while cutting.** Each clip is written first to a hidden file named like
  `.match12_001_goal.rmedia-<id>-<n>.part` in the output folder. When the clip has been checked,
  this file is given its final name. If a run is interrupted (for example, the computer loses
  power), the next run removes any temporary files that were left behind.
* **Preview copies and thumbnails** in a per-user cache folder, limited by the **Preview cache**
  setting. The oldest unused files are removed first.
* **Settings** in a file called `settings.json` in your per-user configuration folder.

The folders depend on the operating system's standard per-user folders, for example
`~/.config/rmedia/` and `~/.cache/rmedia/preview/` on Linux. See
[Configuration › File locations](../administration/configuration.md#file-locations).

> **Unverified:** The Windows folders have not been checked on a Windows machine.

## Upscale for Phones

Select a video, then select **Upscale for phone** at the top of the player. rmedia makes a copy that
fills a phone screen held upright: it frames the picture for the screen's shape, scales it up, and
saves it as an `.mp4` file in an `upscaled` folder next to the video. The original is never changed.

The quickest way: select the video, select **Upscale for phone**, then select **Upscale now**.

### What you see

* **The player** shows the part of the picture that will be kept as a white rectangle. With
  **Follow faces**, the rectangle moves with the person while the video plays.
* **Hatched parts of the timeline** are where no single face fits the screen. There the phone shows
  the whole picture over a blurred copy of it.
* **The phone-shaped picture** beside the player compares the result at the playhead. Drag across
  it (or use the arrow keys after selecting it) to see **Before** (plain enlarging) and **After**
  (rmedia's upscale). It updates when you pause, seek or change a setting. **Centred until
  analysed** means rmedia is still finding faces; the picture updates when it is done.

### Choices

| Setting | Choices | Default |
|---|---|---|
| **Phone screen** | The device presets (for example **Phone fill**, 1080×2340, for most current iPhones, Pixels and Galaxys; **Reddit 9:16**). The phones each preset is for are listed under it. | Phone fill |
| **Framing** | **Follow faces** (pans with the person; blurred background where nobody fits), **Centre** (crops the middle), **Show all** (the whole picture over a blurred copy) | Follow faces |
| **Camera movement** | **Calm**, **Normal**, **Fast**: how quickly the frame may pan (Follow faces only) | Normal |
| **Quality** | **Automatic** (the fastest option that works on this computer), **Standard** (processor), **Graphics card**, **AI** | Automatic |

Choices that are not available on your computer are marked **unavailable**; the line under the
picker says why. Your choices are remembered for next time.

**AI** recovers the most detail from blurry, low-resolution videos. It is offered only when the
graphics card passed the AI self-test (Settings › **Check again** runs it); on most built-in
graphics it is too slow and the line under the picker says how long a minute of video would take.
**Automatic** never picks AI by itself. With AI chosen, the before/after picture is made with the
standard scaler and is marked **Preview: standard scaler**; the finished video is sharper.

To upscale only part of a video, mark **In** and **Out** in the player first. The line next to the
buttons shows **Whole video** or the part that will be used.

### Upscale now or later

* **Upscale now** adds the job to the queue and runs only that job. Other queued clips keep waiting.
* **Add to queue** keeps it for later. Queued upscales show a badge such as `phone-fill · smart`.
  Select **Run** in the queue to process everything.

The run panel shows each upscale's stage (**Finding faces**, **Upscaling**, **Checking**) and, when
it is done, the quality tier used and any parts that used the blurred background. An exported
queue (**Export…**) runs the same jobs with `rmedia batch` and gives identical files.

### Face model

**Follow faces** needs the face model file. The installers include it. In a source checkout, download it with
`cargo xtask fetch-models`. If it is missing,
the framing line says so and rmedia uses the blurred background instead. To use a model in another folder, open
**Settings**, and under **Upscaling quality tiers** select **Models folder › Choose…**.

## Change Settings

Select **Settings** in the top-right corner. Select **Save** to keep your changes, or **Cancel**
(or press `Escape`) to discard them.

### New clips

These are the defaults for every clip in the queue. The options for one video in the queue
override them.

| Setting | Choices | Default |
|---|---|---|
| **Mode** | **Fast (no re-encode)**, **Frame-accurate** | Fast |
| **Output folder** | Select **Choose…** to pick a folder. Select **Reset** to go back to the default. | **“clips” next to each video** |
| **If the file exists** | **Skip it**, **Add a number** (save as `name_1`, `name_2`, …), **Replace it** | Skip it |
| **File names** | A naming pattern. Leave it empty for the default. | `{source}_{index:03}_{name-or-range}.{ext}` |

### Performance

| Setting | Meaning | Default |
|---|---|---|
| **Clips at once** | How many clips are cut at the same time (1–64). | Empty (automatic): half the number of processor cores, up to 4 |
| **Preview cache (MB)** | The most disk space used for preview copies and thumbnails. | 5120 |

### Video engine

This section shows the FFmpeg program in use, where it came from (`bundled`, `config` or `path`),
its version, and which formats accurate mode can write. Any problems are listed as text messages,
shown in red.

* **Choose ffmpeg…** lets you choose a different FFmpeg program. The matching `ffprobe` program must
  be in the same folder.
* **Use automatic** appears after you have chosen a program. It goes back to the bundled engine
  (or, if there is none, the FFmpeg on your `PATH`).

## Troubleshooting

| Symptom | Resolution |
|---|---|
| **● Video engine missing — fix in Settings** | See [Video engine missing or too old](troubleshooting.md#video-engine-missing-or-too-old). |
| `… was not opened in rmedia (use Add files/folder or drag it in)` | See [File was not opened in rmedia](troubleshooting.md#was-not-opened-in-rmedia-use-add-filesfolder-or-drag-it-in). |
| `… is outside the folders you chose; pick the output folder in Settings` | See [Output is outside the folders you chose](troubleshooting.md#is-outside-the-folders-you-chose-pick-the-output-folder-in-settings). |
| The label shows **Preview copy** or **Still frames** | See [Playback modes](troubleshooting.md#the-player-shows-preview-copy-or-still-frames). |
| A clip is **Skipped** with `output already exists` | See [Output already exists](troubleshooting.md#skipped-output-already-exists). |
| **Select a source first** when importing a list | Add a video and select it, then import again. |

## Related Information

* [Getting Started](getting-started.md)
* [Command-Line Guide](cli-guide.md)
* [Troubleshooting](troubleshooting.md)
* [File Formats Reference](../api/file-formats.md): clip lists and manifests
