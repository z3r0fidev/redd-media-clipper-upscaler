# Windows Owner Checks (Runbook)

This runbook covers two owner checks that CI cannot run:

1. **Windows hardware QA:** the installers, the CLI archive and the desktop app on a real Windows
   machine. CI only builds and tests them on `windows-latest` runners
   ([roadmap](../roadmap.md#release-and-project-housekeeping)).
2. **SC-009 on real footage:** with gifsicle, the default `reddit-post` GIF is at least 15%
   smaller than without it, and loses at most 1 VMAF point
   ([spec](../../specs/003-gif-export/spec.md)). The automated synthetic set measured 6.4%
   (bar 5%).

Plan on 2–3 hours, plus the time to choose footage. Run every command in PowerShell on the
Windows machine (for example over RDP). Nothing here uploads anything; the footage is only read.
Record results in the [results sheet](#results-sheet) at the end and send it back. The maintainer
then copies it into `specs/003-gif-export/checklists/validation.md` and ticks the roadmap items.

## 0. What to Test

Use the Windows artifact `rmedia-x86_64-pc-windows-msvc` from a `release.yml` run:

| Run | Commit | Notes |
|---|---|---|
| 40345344064 | `723bbbe` | Existing run (gifski off), digest `sha256:f1fa7fdf7abcce0504dc5ed25d6b4cc0eedfe4c42870a72d59f820055e81bc36`. Predates the Settings › GIF export engine pickers (`813cdeb`) and the fixes in `475ccf4` and `a69089a` |
| **40452999345** | `789d7e9` | **Use this one.** Same code as `a69089a` plus the runbook docs (gifski off, dispatched 2026-10-27 with the owner's approval). Both packages built, and `release gate: ok` before staging, after staging and on the built bundles (`--scan`). Windows artifact 411,470,509 B, digest `sha256:68075b99d243932f7ce4ed35b47ec39095c5024e90b3945fa4fa117427cd7e27` |

The artifact contains the NSIS setup (`…_x64-setup.exe`), the MSI and the CLI archive
(`rmedia-cli-<ref>-x86_64-pc-windows-msvc.zip`). The installer file names still carry `0.2.0`,
because `tauri.conf.json` has not been bumped. With gifski off, gifski is not bundled:
`max-quality` falls back to the built-in engine with `ENGINE_FALLBACK`, as designed.

## 1. Machine and Folders

The machine needs:
- Windows 10 or 11, x64.
- At least 10 GB free.
- Ideally a **clean** machine without the Visual C++ Redistributable, so that the 002 runtime gap
  shows (step 3.4).
- A GPU, if available, for the AI tier check.

Record the machine details:

```powershell
Get-ComputerInfo -Property OsName, OsVersion, OsArchitecture, CsProcessors, CsTotalPhysicalMemory
Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64' -ErrorAction SilentlyContinue |
  Select-Object Installed, Version          # empty = no VC++ runtime installed
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue | Select-Object pv
```

Create the working folders. One of them has spaces and non-ASCII letters, to test path handling:

```powershell
$qa = 'C:\rmedia-qa'
New-Item -ItemType Directory -Force "$qa\artifact", "$qa\cli", "$qa\footage", "$qa\out", "$qa\Clips ünï 日本" | Out-Null
```

## 2. Download and Verify

1. Download the artifact zip from the run's page (Actions › the run › Artifacts ›
   `rmedia-x86_64-pc-windows-msvc`) into `C:\rmedia-qa\artifact`. Alternatively, with an
   authenticated `gh`, run
   `gh run download <run-id> -R z3r0fidev/redd-media-clipper-upscaler -n rmedia-x86_64-pc-windows-msvc -D C:\rmedia-qa\artifact`.
   `gh` unpacks the zip, so skip the hash check below.
2. Check the zip against the digest on the run page, then unpack it:

   ```powershell
   Get-FileHash C:\rmedia-qa\artifact\rmedia-x86_64-pc-windows-msvc.zip -Algorithm SHA256
   Expand-Archive C:\rmedia-qa\artifact\rmedia-x86_64-pc-windows-msvc.zip C:\rmedia-qa\artifact\x
   Get-ChildItem -Recurse C:\rmedia-qa\artifact\x | Select-Object FullName, Length
   ```

3. Record the file names and sizes. They fill the "Unverified" notes in
   [Installation](../administration/installation.md).

## 3. CLI Archive

### 3.1 Unpack and Doctor

```powershell
$zip = Get-ChildItem -Recurse C:\rmedia-qa\artifact\x -Filter 'rmedia-cli-*.zip' | Select-Object -First 1
Expand-Archive $zip.FullName C:\rmedia-qa\cli
$cli = (Get-ChildItem -Recurse C:\rmedia-qa\cli -Filter rmedia.exe | Select-Object -First 1).FullName
Get-ChildItem (Split-Path $cli) -Recurse | Select-Object FullName, Length
& $cli --version
& $cli doctor
& $cli doctor --json > C:\rmedia-qa\doctor.json
```

Expected:
- `ffmpeg:` and `ffprobe:` show `(bundled)`.
- All encoders `yes`.
- `gif: yes`, `reddit mp4: yes`.
- `gifsicle: 1.98 (bundled …\rmedia-gifsicle.exe)`.
- `gifski: no (…)`.
- `model:` lines `ok`.
- `status: ready`.
- `licenses\` contains the FFmpeg, ncnn, glslang and GIFSICLE notices, and no GIFSKI files.

SmartScreen may warn on first run because the binaries are unsigned. Record whether it does.

### 3.2 Clips (001) and Never Touching the Source

Copy a real video into both folders, and note its hash:

```powershell
Copy-Item <your video> "C:\rmedia-qa\footage\a.mp4"
Copy-Item <your video> "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4"
$before = Get-FileHash "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4"
& $cli clip "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8
& $cli clip "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8 --mode accurate -o C:\rmedia-qa\out
& $cli clip "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8     # again: skipped, never overwritten
(Get-FileHash "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4").Hash -eq $before.Hash   # must be True
```

Check that:
- the outputs land in `clips\` next to the source and in `C:\rmedia-qa\out`;
- they play in the Windows player;
- the paths that rmedia prints have no `\\?\` prefix.

Long paths: repeat one clip with `-o` set to a folder whose full path is over 260 characters:

```powershell
$long = 'C:\rmedia-qa\' + ('d' * 60 + '\') * 5
New-Item -ItemType Directory -Force $long | Out-Null
& $cli clip C:\rmedia-qa\footage\a.mp4 -s 10 -d 4 -o $long
```

If you have a second drive or a OneDrive folder, also run one clip from it, and one clip to it.

### 3.3 GIF and Reddit MP4 (003)

```powershell
& $cli gif --list-targets
& $cli gif "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8 --dry-run
& $cli gif "C:\rmedia-qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8
& $cli gif C:\rmedia-qa\footage\a.mp4 -s 10 -d 8 --target reddit-comment -o C:\rmedia-qa\out
& $cli gif C:\rmedia-qa\footage\a.mp4 -s 10 -d 8 --format mp4 -o C:\rmedia-qa\out
& $cli gif C:\rmedia-qa\footage\a.mp4 -s 10 -d 8 --target max-quality -o C:\rmedia-qa\out   # ENGINE_FALLBACK (no gifski)
& $cli gif C:\rmedia-qa\footage\a.mp4 -s 10 -d 6 --shape 9:16 --framing smart -o C:\rmedia-qa\out
& $cli clip C:\rmedia-qa\footage\a.mp4 -s 30 -d 6 --gif -o C:\rmedia-qa\out
```

For each command, record:
- the summary line (size, width, fps, engine);
- the dry run's estimate against the real size;
- that the GIF loops in a browser (drag it into Edge);
- that the MP4 plays and has no audio.

### 3.4 Upscale and the AI Worker (002)

```powershell
& $cli doctor --recheck
& $cli clip C:\rmedia-qa\footage\a.mp4 -s 10 -d 6 --upscale -o C:\rmedia-qa\out
& $cli clip C:\rmedia-qa\footage\a.mp4 -s 10 -d 6 --upscale --tier ai -o C:\rmedia-qa\out
& $cli upscale C:\rmedia-qa\footage\a.mp4 --dry-run          # whole video: plan only
```

Record the `tier:` lines.

On a machine **without** the VC++ runtime (step 1), the AI tier is expected to fail to start
`rmedia-sr.exe`, with a missing `vcruntime140.dll`, `msvcp140.dll` or `vcomp140.dll`. Record
the exact error. It decides the open "Visual C++ runtime" item. With the runtime installed,
record whether the AI tier is listed as usable and how fast it is.

### 3.5 Cancel and Cleanup (001 SC-007, 003 SC-008)

These tests check that Job Objects kill every child process and that no partial files are left.
Use a source of a minute or more as `a.mp4`, so that each job is still running when you stop it.
Close the desktop app first: case (c) ends every process named `rmedia`. Start a long GIF job,
then stop it in three ways:

```powershell
function Show-Left { Get-Process | Where-Object Name -like 'rmedia*' | Select-Object Name, Id
                     Get-ChildItem -Recurse C:\rmedia-qa\out -Force -Filter *.part -ErrorAction SilentlyContinue }

# a) Ctrl+C: press it during "encode" or "optimise"; time how long it takes to return
& $cli gif C:\rmedia-qa\footage\a.mp4 --budget none -o C:\rmedia-qa\out\cancel
Show-Left      # expect nothing

# b) Close the window: start the same job in a new PowerShell window, then close that window
Start-Process powershell -ArgumentList '-NoExit', '-Command', "& '$cli' gif C:\rmedia-qa\footage\a.mp4 --budget none -o C:\rmedia-qa\out\cancel2"
# ...close it with the X while it is encoding, then:
Show-Left      # expect nothing

# c) Kill the parent: start it again, then end only rmedia.exe
Start-Process $cli -ArgumentList 'gif', 'C:\rmedia-qa\footage\a.mp4', '--budget', 'none', '-o', 'C:\rmedia-qa\out\cancel3'
Start-Sleep 5; Stop-Process -Name rmedia -Force
Start-Sleep 2; Show-Left      # expect no rmedia-ffmpeg / rmedia-gifsicle left
```

d) Ctrl+C before the run (needs a build from `fae5d47` or later; run 40452999345 predates it):
   point `gif` at a large folder with `--json` and press Ctrl+C while it is still scanning,
   before any `job_started` line. Expected: exit 130 (`$LASTEXITCODE`), nothing written, and
   the last line a `summary` with `"exit_code":130`.

   ```powershell
   & $cli gif C:\Users --json -o C:\rmedia-qa\out\cancel5 ; $LASTEXITCODE
   ```

Repeat case (a) with an upscale job for 002: `& $cli upscale C:\rmedia-qa\footage\a.mp4 --tier cpu -o C:\rmedia-qa\out\cancel4`.

Pass: case (a) returns within 2 s. In every case, no `rmedia-ffmpeg`, `rmedia-ffprobe`,
`rmedia-gifsicle` or `rmedia-sr` process is left, and no `.part` file remains.

## 4. Installers and Desktop App

### 4.1 NSIS (per user)

Run `…_x64-setup.exe` and record:
- whether it asks for admin (it should not);
- the install folder (expected `%LOCALAPPDATA%\rmedia\`);
- the files installed;
- the Start menu entry;
- the SmartScreen warning.

```powershell
Get-ChildItem "$env:LOCALAPPDATA\rmedia" -Recurse | Select-Object FullName, Length
```

### 4.2 Desktop QA

Start rmedia from the Start menu.

Basics:
1. The window opens. **Settings** shows the bundled engine.
2. **Settings › GIF export** (needs a build from `813cdeb` or later) shows:
   - GIF yes and Reddit MP4 yes;
   - gifsicle 1.98 (bundled) and gifski not found.
3. **Choose gifsicle…**: pick `rmedia-gifsicle.exe` from the CLI folder. The source becomes the
   setting. **Use automatic** goes back to bundled.

Clip and queue:
4. Add `C:\rmedia-qa\Clips ünï 日本` as a source. The video lists and plays, and seeking works.
5. Mark In and Out and add a clip to the queue. **Run**. The clip is written, and the result
   plays in the app.

GIF view:
6. **GIF for Reddit**: the estimate shows before export, and changes when you change the target
   or format.
7. **Export now** (SC-007: count the clicks after opening the video; at most 3). The result card
   shows its size. The GIF plays in the app and is within the target's limit.
8. Export the same range with the CLI (`gif … -s <in> -e <out>`). The two files must be
   byte-identical:

   ```powershell
   Get-FileHash <app gif>, <cli gif>
   ```

9. Start a GIF export and **Cancel** it. In PowerShell, run `Show-Left`: nothing is left.
10. Close the app while an export runs. Again nothing is left.

Upscale and export:
11. Upscale view: **9:16 smart** on a clip with a face. The overlay follows the face, and the
    upscale runs.
12. **Export manifest** from the queue, then run it with
    `rmedia batch <manifest> --dry-run`. It plans the same jobs.

### 4.3 MSI

Uninstall the NSIS install first (**Settings › Apps › rmedia › Uninstall**; record the entry
name). Then install the `.msi` and record:
- whether it asks for admin;
- its install folder and scope (per user or per machine).

Start the app once, then uninstall it. Check that `%APPDATA%\OpenLogic\rmedia` and
`%LOCALAPPDATA%\OpenLogic\rmedia` are still there; they are not meant to be removed.

## 5. SC-009: gifsicle on Real Footage

### 5.1 Choose the Footage

Pick 8–12 real clips of the kind you would post, each 6–10 s long and 720p or larger. Mix the
content:
- gameplay;
- phone video of people;
- a screen recording or UI;
- animation;
- fast sport or motion;
- a low-light or noisy shot;
- a talking head.

Avoid clips whose scene changes completely at every cut. List them in
`C:\rmedia-qa\footage\clips.csv` (absolute paths; start and duration in seconds):

```text
file,start,duration
C:\rmedia-qa\footage\game.mp4,42,8
C:\rmedia-qa\footage\phone_party.mov,5,8
C:\rmedia-qa\footage\desktop_capture.mkv,0,10
```

### 5.2 Run the Measurement

Use the script [`windows-qa/Measure-Sc009.ps1`](windows-qa/Measure-Sc009.ps1). Fetch it from
the repo, or copy it over RDP. For each clip it:
- makes the default `reddit-post` GIF with `--budget none`, as the automated bench does: once
  with `--no-optimise` and once with gifsicle;
- scores both against the source with the bundled ffmpeg's libvmaf, using the same graph as the
  test helper `vmaf_gif_at`.

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/z3r0fidev/redd-media-clipper-upscaler/main/docs/operations/windows-qa/Measure-Sc009.ps1 -OutFile C:\rmedia-qa\Measure-Sc009.ps1
Get-Content C:\rmedia-qa\Measure-Sc009.ps1      # read it before running it
powershell -ExecutionPolicy Bypass -File C:\rmedia-qa\Measure-Sc009.ps1 -Cli $cli -Clips C:\rmedia-qa\footage\clips.csv -Out C:\rmedia-qa\sc009
```

The script ends with a line like:

```text
SC-009: 10 clips, 61,234,567 -> 50,123,456 bytes (18.1% smaller), worst VMAF loss 0.62: PASS
```

The **pass** condition: at least 15% smaller over the whole set, and a worst loss of 1.0 or
less. Keep `C:\rmedia-qa\sc009\results.csv`; its per-clip rows show which content gifsicle helps.
Open a few pairs (`NN\plain` against `NN\gifsicle`) side by side in Edge for a visual check.

If the set fails at the default lossy level (30), that is the result to report; do not tune it
here. Note the content types that saved the least.

## Results Sheet

Copy this, fill it in, and send it back:

```text
Machine: <OS + build>, <CPU>, <RAM>, <GPU + driver>, VC++ runtime: <none | version>, WebView2: <version>
Artifact: run <id>, commit <sha>, zip sha256 matches: <yes/no>
Files: setup.exe <name, size>, msi <name, size>, cli zip <name, size>

3.1 doctor: ffmpeg bundled <y/n>, gif/mp4 <y/n>, gifsicle <line>, gifski <line>, status <..>, SmartScreen <y/n>
3.2 clip: unicode/space path <ok/..>, accurate <ok/..>, no-clobber skip <ok/..>, source hash unchanged <True/False>, long path <ok/..>, \\?\ shown <y/n>, other drive <ok/n.a.>
3.3 gif: post <size, WxH@fps>, comment <..>, mp4 <..>, max-quality fallback <ok/..>, 9:16 smart <ok/..>, clip --gif <ok/..>, dry-run estimate vs real <..>
3.4 upscale: tiers <cpu/gpu/ai lines>, ai on this machine <works | error text>
3.5 cancel: Ctrl+C <s>, window close <clean/left: ..>, kill parent <clean/left: ..>, upscale cancel <..>
4.1 NSIS: admin <y/n>, folder <..>, files <..>, Start menu <..>
4.2 desktop: settings GIF <ok/..>, picker <ok/..>, playback unicode path <ok/..>, clip run <ok/..>, GIF estimate <ok/..>,
    export clicks <n>, app = CLI bytes <y/n>, cancel clean <y/n>, close-while-running clean <y/n>, smart 9:16 <ok/..>, manifest <ok/..>
4.3 MSI: admin <y/n>, folder/scope <..>, uninstall entry <name>, user data kept <y/n>
5 SC-009: <n> clips, <plain> -> <opt> bytes, <saved>% smaller, worst VMAF loss <..>: <PASS/FAIL>; weakest content <..>
Anything odd: <..>
```

## Related

[Installation](../administration/installation.md) · [Build and Release](build-and-release.md) ·
[Roadmap](../roadmap.md) · [003 validation](../../specs/003-gif-export/checklists/validation.md)
