# Smart Cut Owner Checks, All Platforms (Runbook)

This runbook covers the 0.4.0 (feature 004, smart cut) checks that CI cannot run. They need real
machines, phones, the desktop app's screen and a Reddit account:

| Part                  | Platform      | What it proves                                                                | Open item                                              |
| --------------------- | ------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------ |
| [3](#3-linux)         | Linux         | The packaged CLI and desktop app, end to end                                  | Packages from the tag build                            |
| [4](#4-windows)       | Windows 10/11 | The CLI archive, the installers, and the desktop app's Smart mode in WebView2 | Spike S1's WebView2 row; desktop smart mode on Windows |
| [5](#5-android-phone) | Android       | Smart mp4 files play cleanly across the seams                                 | S1's Android row                                       |
| [6](#6-iphone)        | iPhone        | The same, in the iOS players                                                  | S1's iPhone row                                        |
| [7](#7-reddit-upload) | Reddit        | A smart mp4 is accepted, processed and plays on Reddit                        | S1's Reddit row                                        |
| [8](#8-real-footage)  | Any           | Smart cut on your own camera and phone footage                                | Behaviour outside the test patterns                    |

Plan on about 2 hours in total; the phone and Reddit parts take 15–20 minutes each. Parts 3 and
4 can be done in either order, and parts 5–7 need only the files made in part 2. Record the
results in the [results sheet](#results-sheet) and send it back; the maintainer copies it into
`specs/004-smart-cut/checklists/validation.md` and research R5's S1 table, and ticks the roadmap.

Nothing in parts 1–6 uploads anything. Part 7 posts a test video to Reddit: use a test
subreddit or your own profile, and delete the post afterwards if you like.

## 0. What to Test

Use the artifacts of the `release.yml` run for the tag `v0.4.0`:

| Platform | Artifact | Contents |
|---|---|---|
| Linux | `rmedia-x86_64-unknown-linux-gnu` | `.deb`, `.AppImage`, `rmedia-cli-v0.4.0-x86_64-unknown-linux-gnu.tar.gz` |
| Windows | `rmedia-x86_64-pc-windows-msvc` | NSIS `…_x64-setup.exe`, `.msi`, `rmedia-cli-v0.4.0-x86_64-pc-windows-msvc.zip` |

Tag build: `release.yml` run
[44075993817](https://github.com/z3r0fidev/redd-media-clipper-upscaler/actions/runs/44075993817)
(tag `v0.4.0` on `092171f`). Check the digest after downloading (step 2):

| Artifact | Id | sha256 of the artifact zip |
|---|---|---|
| `rmedia-x86_64-unknown-linux-gnu` | 14326876150 | `c946dd4442552930e3d9ddc1fb3184d53f00c1deb16eed215efa952205ec5a85` |
| `rmedia-x86_64-pc-windows-msvc` | 14327605545 | `cb0e1975a75fdfbd1dc67c75fef41cee223e90f41b58d8939d376b3323519359` |

## 1. The Seam Checks in Brief

The test files use FFmpeg's `testsrc2` pattern, which draws the **source time and frame number**
in its top-left corner. A smart clip of 3.32 s to 20.72 s of a 25 fps source has two seams, where
the re-encoded ends meet the copied middle:

| Seam | Clip time | Clip frame | Source frame shown in the picture |
|---|---|---|---|
| Start of the clip | 0.000 s | 0 | 83 |
| Head → copy | 0.680 s | 17 | 100 (source 4.000 s) |
| Copy → tail | 16.680 s | 417 | 500 (source 20.000 s) |
| End of the clip | 17.360 s (last frame) | 434 | 517 |

At each seam, a good file shows: the counter stepping by exactly one frame, no frozen frame, no
jump back or forward, no grey or green blocks, no flicker in the moving pattern, and sound with no
click or gap (the test tone is a steady 440 Hz). The whole clip lasts **17.40 s**.

## 2. Make the Test Files

Do this once, on the Linux or Windows machine, with the packaged CLI. The files are small (about
10 MB each). Copy the `out/` folder to the phones afterwards.

### 2.1 Linux

```bash
mkdir -p ~/rmedia-004 && cd ~/rmedia-004
tar xzf ~/Downloads/rmedia-cli-v0.4.0-x86_64-unknown-linux-gnu.tar.gz
B=$PWD/rmedia-cli-v0.4.0-x86_64-unknown-linux-gnu   # the unpacked folder; adjust if named differently
FF=$B/rmedia-ffmpeg

# Source A: like most cameras (closed 2 s GOPs, 3 B-frames, AAC).
$FF -v error -f lavfi -i testsrc2=s=1280x720:r=25:d=30 -f lavfi -i sine=frequency=440:d=30 \
  -c:v libx264 -x264-params keyint=50:min-keyint=50:scenecut=0:bframes=3 -pix_fmt yuv420p \
  -c:a aac -shortest src.mp4
# Source B: other encoder settings (slow preset, 4 references, 5 B-frames). The re-encoded ends
# cannot reuse its parameter sets, so players must switch mid-file: the hard case.
$FF -v error -f lavfi -i testsrc2=s=1280x720:r=25:d=30 -f lavfi -i sine=frequency=440:d=30 \
  -c:v libx264 -preset slow -x264-params keyint=50:min-keyint=50:scenecut=0:bframes=5:ref=4 \
  -pix_fmt yuv420p -c:a aac -shortest other.mp4
# Source C: open GOPs (smart cut must fall back).
$FF -v error -f lavfi -i testsrc2=s=1280x720:r=25:d=10 -c:v libx264 \
  -x264-params keyint=50:bframes=3:open-gop=1 -pix_fmt yuv420p open.mp4

# The clips: smart in four containers, and an accurate control.
$B/rmedia clip src.mp4 other.mp4 -s 3.32 -e 20.72 --mode smart -o out --name '{source}_smart.{ext}'
$B/rmedia clip other.mp4 -s 3.32 -e 20.72 --mode smart -o out --container mkv --name '{source}_smart.{ext}'
$B/rmedia clip other.mp4 -s 3.32 -e 20.72 --mode smart -o out --container mov --name '{source}_smart.{ext}'
$B/rmedia clip other.mp4 -s 3.32 -e 20.72 --mode smart -o out --container ts  --name '{source}_smart.{ext}'
$B/rmedia clip other.mp4 -s 3.32 -e 20.72 --mode accurate -o out --name '{source}_accurate.{ext}'
ls -l out
```

Expected: five `✓ … (smart, 92% copied)` lines and one plain `✓` for the accurate control. Files
in `out/`: `src_smart.mp4`, `other_smart.mp4`, `other_smart.mkv`, `other_smart.mov`,
`other_smart.ts`, `other_accurate.mp4`.

### 2.2 Windows (PowerShell)

```powershell
$w = "$HOME\rmedia-004"; New-Item -ItemType Directory -Force $w | Out-Null; Set-Location $w
Expand-Archive "$HOME\Downloads\rmedia-cli-v0.4.0-x86_64-pc-windows-msvc.zip" -DestinationPath cli -Force
$B = (Get-ChildItem cli -Recurse -Filter rmedia.exe | Select-Object -First 1).DirectoryName
$FF = "$B\rmedia-ffmpeg.exe"
& $FF -v error -f lavfi -i testsrc2=s=1280x720:r=25:d=30 -f lavfi -i sine=frequency=440:d=30 -c:v libx264 -x264-params keyint=50:min-keyint=50:scenecut=0:bframes=3 -pix_fmt yuv420p -c:a aac -shortest src.mp4
& $FF -v error -f lavfi -i testsrc2=s=1280x720:r=25:d=30 -f lavfi -i sine=frequency=440:d=30 -c:v libx264 -preset slow -x264-params keyint=50:min-keyint=50:scenecut=0:bframes=5:ref=4 -pix_fmt yuv420p -c:a aac -shortest other.mp4
& $FF -v error -f lavfi -i testsrc2=s=1280x720:r=25:d=10 -c:v libx264 -x264-params keyint=50:bframes=3:open-gop=1 -pix_fmt yuv420p open.mp4
& "$B\rmedia.exe" clip src.mp4 other.mp4 -s 3.32 -e 20.72 --mode smart -o out --name '{source}_smart.{ext}'
foreach ($c in 'mkv','mov','ts') { & "$B\rmedia.exe" clip other.mp4 -s 3.32 -e 20.72 --mode smart -o out --container $c --name '{source}_smart.{ext}' }
& "$B\rmedia.exe" clip other.mp4 -s 3.32 -e 20.72 --mode accurate -o out --name '{source}_accurate.{ext}'
Get-ChildItem out
```

## 3. Linux

Record: distribution and version, desktop (X11 or Wayland), CPU, GPU.

### 3.1 Download and Verify

```bash
cd ~/Downloads && sha256sum rmedia-x86_64-unknown-linux-gnu.zip   # compare with the release notes
unzip -o rmedia-x86_64-unknown-linux-gnu.zip -d rmedia-0.4.0
```

### 3.2 CLI

With `$B` from step 2.1:

```bash
cd ~/rmedia-004
$B/rmedia --version                     # rmedia 0.4.0
$B/rmedia doctor | grep -E "ffmpeg:|smart cut|status"
#   ffmpeg: … (bundled)    smart cut: available (libx264)    status: ready
$B/rmedia clip src.mp4 -s 3.32 -e 20.72 --mode smart --dry-run | grep smart
#   smart: head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s (18 fr) re-encoded · 92% copied
$B/rmedia clip open.mp4 -s 1.3 -e 8.7 --mode smart --dry-run | grep smart
#   smart → accurate: the video has open GOPs (frames that depend on the previous GOP)
for f in out/*; do printf '%s: ' $f; $B/rmedia-ffprobe -v error -count_frames -select_streams v:0 \
  -show_entries stream=nb_read_frames -of csv=p=0 "file:$f"; done      # 435 for every file (twice for .ts)
for f in out/*; do $B/rmedia-ffmpeg -v error -i "file:$f" -f null - && echo "$f clean"; done
```

**Strict mode (`--no-smart-fallback`):** with this flag, a video that smart cut cannot handle
must fail instead of being cut in accurate mode. `open.mp4` has open GOPs, so:

```bash
mkdir -p strict
$B/rmedia clip open.mp4 -s 1.3 -e 8.7 --mode smart --no-smart-fallback -o strict; echo "exit $?"
#   error: command line: smart cut not possible: the video has open GOPs (frames that depend on the previous GOP)
#   exit 3
ls -a strict                                                          # only . and ..: nothing written
$B/rmedia clip open.mp4 -s 1.3 -e 8.7 --mode smart --no-smart-fallback --json -o strict | grep -o '"code":"[A-Z_]*","detail":{"reason":"[a-z_]*"}'
#   "code":"SMART_UNAVAILABLE","detail":{"reason":"open_gop"}
```

Pass: the error line names the open GOPs, the exit code is 3 (not 0), `strict` stays empty and
the JSON code is `SMART_UNAVAILABLE` with the reason `open_gop`. Without the flag the same clip
succeeds in accurate mode (the dry-run line above).

**Cancel (SC-008):** start a long smart clip and press Ctrl+C during it, then check nothing is
left. To time it, let `timeout` send the same signal as Ctrl+C (SIGINT) after exactly 3 s; the
time to stop is the elapsed time minus 3 s:

```bash
$B/rmedia-ffmpeg -v error -f lavfi -i testsrc2=s=1920x1080:r=25:d=600 -c:v libx264 -preset ultrafast \
  -x264-params keyint=50:min-keyint=50:scenecut=0 -pix_fmt yuv420p long.mp4
t0=$(date +%s.%N); timeout -s INT 3 $B/rmedia clip long.mp4 -s 3.3 -e 590.7 --mode smart -o cancel
echo "elapsed $(echo "$(date +%s.%N) - $t0" | bc) s"                 # e.g. 3.05 s: stopped 0.05 s after the signal
ls -a cancel; pgrep -a 'rmedia|ffmpeg'                                # no .part, no folders, no processes
```

The clip line says `cancelled` and the summary `1 cancelled`. Pass: elapsed under 5 s (stopped
within 2 s of the signal), `cancel` holds only `.` and `..`, and `pgrep` prints nothing. You can
also press Ctrl+C by hand during the same command without `timeout`; the result must be the same.

### 3.3 Desktop App

Install the `.deb` (`sudo apt install ./rmedia_0.4.0_amd64.deb`) or run the `.AppImage`. Open
`src.mp4`, then follow these steps and note anything that looks wrong.

| #   | Step                                                                                                                                            | Expected                                                                                                                                 |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | Select `src.mp4`                                                                                                                                | Small ticks along the top of the timeline, one every 2 s (15), within about 2 s                                                          |
| D2  | **Settings › New clips › Mode** → **Smart (frame-exact, copies most of the video)** → **Save**. Doctor line in Settings                         | `Smart cut: available (libx264)`                                                                                                         |
| D3  | Mark In at 3.32 s and Out at 20.72 s: **Home**, **Shift+→** three times, **→** eight times, **I**; **Shift+→** 17 times, **→** ten times, **O** | Green band 4–20 s, orange bands at each end; under the timeline `Smart: 92% copied · 0.68 s re-encoded at the start · 0.72 s at the end` |
| D4  | **Add clip** (Enter), then **Run 1 job**                                                                                                        | `Finished · 1 succeeded`; the row says `Smart · 92% copied`                                                                              |
| D5  | Select the result to play it in the app; pause on the seams (table in [section 1](#1-the-seam-checks-in-brief))                                 | Clean seams, 17.40 s                                                                                                                     |
| D6  | Select `open.mp4` (add it with **Add videos**), mark 1.32–6.72 s                                                                                | Orange text: `Smart cut not possible: the video has open GOPs …; cutting in accurate mode`, no bands                                     |
| D7  | In the queue, open the video options of `open.mp4`, tick **Only smart (no fallback)**, add the clip and run                                     | The clip fails with `smart cut not possible: …`; nothing written                                                                         |
| D8  | Open a 10-minute source (`long.mp4` from 3.2)                                                                                                   | All the ticks appear within 2 s; the view never blocks while they load                                                                   |
| D9  | **Export…** the queue as `.json`, then run `rmedia batch` on it with `--on-exists overwrite`                                                    | Same file as the app's (`cmp` reports nothing)                                                                                           |

## 4. Windows

Use the [Windows runbook](windows-owner-checks.md) for the machine set-up (section 1), the
download check (section 2) and the installers (section 4.1/4.3). Then:

### 4.1 CLI

In PowerShell, with `$B` from step 2.2:

```powershell
Set-Location "$HOME\rmedia-004"
& "$B\rmedia.exe" --version
& "$B\rmedia.exe" doctor | Select-String 'ffmpeg:|smart cut|status'
& "$B\rmedia.exe" clip src.mp4 -s 3.32 -e 20.72 --mode smart --dry-run | Select-String smart
Get-ChildItem out | ForEach-Object { "$($_.Name): " + (& "$B\rmedia-ffprobe.exe" -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of csv=p=0 "file:$($_.FullName)") }
Get-ChildItem out | ForEach-Object { & "$B\rmedia-ffmpeg.exe" -v error -i "file:$($_.FullName)" -f null -; if ($LASTEXITCODE -eq 0) { "$($_.Name) clean" } }
```

Expected as in 3.2: `0.4.0`, `bundled`, `smart cut: available (libx264)`, the 92% line, 435
frames for every file, and `<name> clean` for all six files with no error lines between them (a
decode error prints a line from FFmpeg, and a failed decode leaves out that file's `clean`).

**Strict mode (`--no-smart-fallback`):** as in 3.2, `open.mp4` must fail instead of falling back:

```powershell
New-Item -ItemType Directory -Force strict | Out-Null
& "$B\rmedia.exe" clip open.mp4 -s 1.3 -e 8.7 --mode smart --no-smart-fallback -o strict; "exit $LASTEXITCODE"
#   error: command line: smart cut not possible: the video has open GOPs (frames that depend on the previous GOP)
#   exit 3
Get-ChildItem -Force strict                                          # nothing listed
& "$B\rmedia.exe" clip open.mp4 -s 1.3 -e 8.7 --mode smart --no-smart-fallback --json -o strict | Select-String -Pattern '"code":"[A-Z_]*","detail":\{"reason":"[a-z_]*"\}' -AllMatches | ForEach-Object { $_.Matches.Value }
#   "code":"SMART_UNAVAILABLE","detail":{"reason":"open_gop"}
```

Pass: the error line names the open GOPs, exit 3, `strict` empty, `SMART_UNAVAILABLE` with
`open_gop`.

**Cancel (SC-008):** these follow section 3.5 (a) and (c) of the
[Windows runbook](windows-owner-checks.md#35-cancel-and-cleanup-001-sc-007-003-sc-008) with a
smart job instead of a GIF. Close the desktop app first: (c) ends every process named `rmedia`.
First make `long.mp4` (10 minutes of 1080p; this takes a minute or two) and a helper that lists
what is left:

```powershell
& $FF -v error -f lavfi -i testsrc2=s=1920x1080:r=25:d=600 -c:v libx264 -preset ultrafast -x264-params keyint=50:min-keyint=50:scenecut=0 -pix_fmt yuv420p long.mp4
Get-Item long.mp4                      # about 10 minutes of video; the clip below needs it
function Show-Left { Get-Process | Where-Object Name -like 'rmedia*' | Select-Object Name, Id
                     Get-ChildItem -Force -Recurse cancel, cancel3 -ErrorAction SilentlyContinue }
```

The smart job is fast (only the ends are re-encoded; the Linux run was 13% done after about
2 s), so be ready to stop it as soon as its progress bar appears.

```powershell
# a) Ctrl+C: press it as soon as the progress bar shows; count how long it takes to return
#    (by eye or with a stopwatch). Ctrl+C also stops the rest of the line, so run Show-Left after.
& "$B\rmedia.exe" clip long.mp4 -s 3.3 -e 590.7 --mode smart -o cancel
Show-Left      # expect nothing

# c) Kill the parent. One line: it starts the job, ends only rmedia.exe 1 s later, then checks.
#    Press nothing until Show-Left prints.
Start-Process "$B\rmedia.exe" -ArgumentList 'clip', 'long.mp4', '-s', '3.3', '-e', '590.7', '--mode', 'smart', '-o', 'cancel3'; Start-Sleep 1; Stop-Process -Name rmedia -Force; Start-Sleep 2; Show-Left

# c, cleanup) A killed process cannot delete its own temp files; the next rmedia run does.
#    Run any short job, then look again:
& "$B\rmedia.exe" clip src.mp4 -s 1 -e 2 --mode fast -o next | Out-Null; Show-Left
```

Pass for (a): the clip line says `cancelled` and the summary `1 cancelled` within 2 s of
Ctrl+C, and `Show-Left` prints nothing: no `rmedia-ffmpeg` process, no `.part` and no hidden
`.…rmedia-…d` folder.

Pass for (c): right after the kill, `Show-Left` lists no process (Job Objects ended
`rmedia-ffmpeg` with its parent). A hidden `cancel3\.long_…rmedia-<run>-1.d` folder with
`head.ts` (and maybe `copy.ts`) may still be there: the killed process could not delete it. After
the cleanup line, `Show-Left` must print nothing: the next run deletes the temp files of runs that
died (the in-flight journal). `Stop-Process` must not say `Cannot find a process with the name
"rmedia"`: that means the job had already ended and nothing was killed. Delete `cancel3` and run
the line again with a shorter sleep (`Start-Sleep -Milliseconds 500`). An empty `cancel3` folder
is fine.

### 4.2 Desktop App and WebView2

Install with the NSIS installer, start rmedia from the Start menu, and run steps **D1–D9** from
[3.3](#33-desktop-app). D5 plays the smart clip in **WebView2**, which is spike S1's open row;
check it closely:

| # | Check | Expected |
|---|---|---|
| W1 | D5 with `src_smart.mp4` and `other_smart.mp4` (open them with **Add videos**) | Plays to the end; both seams clean; the time shows 17.4 s at the end |
| W2 | Seek to 0.6 s, 0.68 s and 16.7 s with the frame keys (**←**/**→**) | The picture always shows the counter for the frame you are on (source frame 98–102, 498–502) |
| W3 | Open `out\other_smart.mp4` in Microsoft Edge (drag it into a tab) | Plays and seeks cleanly (Edge uses the same engine as WebView2) |
| W4 | The **Windows Media Player** / **Films & TV** app, `other_smart.mp4` | Plays cleanly (optional; record what you see) |

## 5. Android Phone

Record: phone model, Android version.

1. Copy `out/src_smart.mp4`, `out/other_smart.mp4` and `out/other_accurate.mp4` to the phone (USB
   file transfer, or a cloud drive set to keep the original file).
2. Play each in:
   - **Google Photos** (or the phone maker's gallery app);
   - **Files by Google** (its built-in player);
   - **Chrome**: open the file from the Files app with Chrome, or from a local web page if
     easier;
   - **VLC for Android**, if installed (optional).
3. For each player, check the seams ([section 1](#1-the-seam-checks-in-brief)): let it play
   through once, then scrub to about 0.5 s and 16.5 s and step or play slowly past the seams.
   Note the duration the player shows.
4. `other_accurate.mp4` is the control: if it also misbehaves in a player, the problem is the
   player, not smart cut.

`.mkv`, `.mov` and `.ts` are optional on phones; most Android players handle mkv, few handle ts.

## 6. iPhone

Record: iPhone model, iOS version.

1. Send the three mp4 files from section 5 to the iPhone (AirDrop, or save them to **Files**
   from iCloud Drive).
2. Play each in:
   - **Files** (Quick Look preview);
   - **Photos**: in Files, share → **Save Video**, then play it in Photos;
   - **Safari**: open the file from iCloud Drive in Safari, or from a local web page.
3. Check the seams and the duration as in section 5. Also check that Photos shows a thumbnail
   and the right length (17 s).

`.mkv` and `.ts` do not play on iOS by design; only mp4 (and mov) matter here.

## 7. Reddit Upload

This posts a test video. Use your own profile or a test subreddit you control.

1. On the Reddit website, create a post with **Images & Video** and upload `other_smart.mp4`
   (the hard case: other encoder settings).
2. Wait for Reddit to finish processing. Record whether the upload was accepted, how long the
   processing took, and any error.
3. Play the post in a desktop browser and in the Reddit app on a phone. Check the seams and the
   duration as in section 1. Reddit re-encodes uploads, so a problem here is a decode problem
   at upload, not in the player.
4. Optional: repeat with `src_smart.mp4`, and post `other_accurate.mp4` as the control if
   anything looks wrong.
5. Delete the post if you don't want to keep it.

## 8. Real Footage

On any platform, cut two of your own videos with the CLI or the app:

- a camera or screen recording in H.264 mp4 (often smart-cut);
- a phone video (often variable frame rate, which falls back with `vfr`).

```bash
rmedia clip <video> -s <a time a few seconds in> -d 60 --mode smart --dry-run
rmedia clip <video> -s <same> -d 60 --mode smart -o real
```

Record for each: the dry-run line (the copied share, or the fallback reason), the result line,
whether the clip plays cleanly at its two seams (about 0–2 s in and 0–2 s before the end; the
dry run gives the exact head and tail lengths), and how long it took compared with
`--mode accurate` on the same range.

## Results Sheet

Copy this, fill it in, and send it back. Write `ok`, `fails: <what you saw>` or `skipped`.

```text
rmedia 0.4.0 smart cut owner checks — date: ____  build (release.yml run): ____

LINUX  distro/version: ____  desktop X11/Wayland: ____  CPU/GPU: ____
  3.1 digest matches ____
  3.2 version 0.4.0 / doctor bundled + smart available ____ / 92% line ____
      open GOP fallback line ____ / 435 frames all files ____ / clean decodes ____
      strict: error line ____ exit 3 ____ nothing written ____ SMART_UNAVAILABLE/open_gop ____
      cancel: elapsed __ s (minus 3 s = __ s to stop), nothing left ____
  3.3 D1 ticks ____ D2 ____ D3 bands + line ____ D4 ____ D5 seams ____ D6 ____ D7 ____
      D8 ticks in __ s ____ D9 cmp identical ____

WINDOWS  version: ____  CPU/GPU: ____  WebView2 version: ____
  4.1 version/doctor ____ / 92% line ____ / 435 frames ____ / clean decodes ____
      strict: error line ____ exit 3 ____ nothing written ____ SMART_UNAVAILABLE/open_gop ____
      cancel (a) Ctrl+C __ s ____ (c) kill parent: no process ____ cleaned by the next run ____
  4.2 D1–D9: ____________________________________________
      W1 src_smart ____ other_smart ____  W2 seeks ____  W3 Edge ____  W4 ____

ANDROID  model: ____  Android version: ____
                    src_smart.mp4   other_smart.mp4   other_accurate.mp4   duration shown
  Google Photos     ____            ____              ____                 ____
  Files by Google   ____            ____              ____                 ____
  Chrome            ____            ____              ____                 ____
  VLC (optional)    ____            ____              ____                 ____

IPHONE  model: ____  iOS version: ____
                    src_smart.mp4   other_smart.mp4   other_accurate.mp4   duration shown
  Files             ____            ____              ____                 ____
  Photos            ____            ____              ____                 ____
  Safari            ____            ____              ____                 ____

REDDIT
  upload accepted ____  processing time ____  error ____
  plays in browser ____  in the app ____  seams ____  duration ____

REAL FOOTAGE
  video 1 (camera/screen): dry run ____________ result ____ seams ____ smart __ s / accurate __ s
  video 2 (phone):         dry run ____________ result ____ seams ____ smart __ s / accurate __ s

Notes:
```

## Related

[Windows owner checks](windows-owner-checks.md) ·
[Smart cut quickstart](../../specs/004-smart-cut/quickstart.md) ·
[Release notes 0.4.0](../releases/0.4.0.md) · [Troubleshooting › Smart cut](../user/troubleshooting.md#smart-cut)
