# Windows Owner Checks over SSH

This is the [Windows owner checks](windows-owner-checks.md) runbook for an SSH session into
the Windows machine. It has the same commands, plus the steps that SSH changes: copying files
over, starting PowerShell and setting its console to UTF-8, timing the AI tier, and checking
MP4 audio without a player. The runbook still says what each result should be, and its
[results sheet](windows-owner-checks.md#results-sheet) is where you record them.

A program started over SSH has no visible window. The installers' prompts, the desktop app,
closing a window mid-job and playing the results need the screen or RDP ([step 9](#9-needs-the-screen-or-rdp)).

Build under test: `release.yml` run **40740230580** (commit `5e01399`, gifski off). Windows
artifact `rmedia-x86_64-pc-windows-msvc`, id 12840729892, 412,635,658 B, digest
`sha256:b0c6860b0814add19a2b1aa7c2bfda4107ec7f3315a0cdac40d11b4124aa3fbe`.

## 0. On the Linux Machine: Fetch the Artifact

Downloading the zip through the API keeps it zipped, so its hash can be checked against the
digest:

```bash
mkdir -p ~/rmedia-qa && cd ~/rmedia-qa
gh api repos/z3r0fidev/redd-media-clipper-upscaler/actions/artifacts/12840729892/zip > rmedia-x86_64-pc-windows-msvc.zip
sha256sum rmedia-x86_64-pc-windows-msvc.zip
# expect b0c6860b0814add19a2b1aa7c2bfda4107ec7f3315a0cdac40d11b4124aa3fbe
```

After step 1 has created the folders on Windows, copy the zip, a test video of a minute or more
(so that the cancel tests in step 7 have time to stop it), and the SC-009 script:

```bash
scp rmedia-x86_64-pc-windows-msvc.zip <user>@<host>:C:/rmedia-qa/artifact/
scp <your video> <user>@<host>:C:/rmedia-qa/footage/a.mp4
scp <repo>/docs/operations/windows-qa/Measure-Sc009.ps1 <user>@<host>:C:/rmedia-qa/
```

## 1. Start PowerShell and Set Up

OpenSSH on Windows usually opens `cmd`, so start PowerShell first. UTF-8 output makes the
Unicode folder names print correctly:

```powershell
powershell
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$qa = 'C:\rmedia-qa'
New-Item -ItemType Directory -Force "$qa\artifact", "$qa\cli", "$qa\footage", "$qa\out", "$qa\Clips ünï 日本" | Out-Null
```

Record the machine details:

```powershell
Get-ComputerInfo -Property OsName, OsVersion, OsArchitecture, CsProcessors, CsTotalPhysicalMemory
Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64' -ErrorAction SilentlyContinue |
  Select-Object Installed, Version          # empty = no VC++ runtime (the best case for step 6)
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue | Select-Object pv
```

Now run the `scp` commands from step 0.

## 2. Verify and Unpack

```powershell
(Get-FileHash $qa\artifact\rmedia-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash   # B0C6860B…AA3FBE
Expand-Archive $qa\artifact\rmedia-x86_64-pc-windows-msvc.zip $qa\artifact\x
Get-ChildItem -Recurse $qa\artifact\x | Select-Object FullName, Length

$zip = Get-ChildItem -Recurse $qa\artifact\x -Filter 'rmedia-cli-*.zip' | Select-Object -First 1
Expand-Archive $zip.FullName $qa\cli
$cli = (Get-ChildItem -Recurse $qa\cli -Filter rmedia.exe | Select-Object -First 1).FullName
```

If the SSH session drops, reconnect and run `powershell`, the `[Console]` line, the `$qa` line and
the `$cli` line again. Step 7's `Show-Left` must be defined again too.

## 3. Doctor (Runbook §3.1)

```powershell
Get-ChildItem (Split-Path $cli) -Recurse | Select-Object FullName, Length
& $cli --version
& $cli doctor
& $cli doctor --json > $qa\doctor.json
```

Expected:
- `ffmpeg` and `ffprobe` show `(bundled)`.
- All encoders `yes`.
- `gif: yes`, `reddit mp4: yes`.
- gifsicle 1.98 bundled, and gifski `no`.
- `model:` lines `ok`, and `status: ready`.
- `licenses\` has no GIFSKI files.

## 4. Clips and the Untouched Source (Runbook §3.2)

```powershell
Copy-Item $qa\footage\a.mp4 "$qa\Clips ünï 日本\vidéo à.mp4"
$before = Get-FileHash "$qa\Clips ünï 日本\vidéo à.mp4"
& $cli clip "$qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8
& $cli clip "$qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8 --mode accurate -o $qa\out
& $cli clip "$qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8          # again: must say skipped
(Get-FileHash "$qa\Clips ünï 日本\vidéo à.mp4").Hash -eq $before.Hash   # must be True

# A path over 260 characters
$long = 'C:\rmedia-qa\' + ('d' * 60 + '\') * 5
New-Item -ItemType Directory -Force $long | Out-Null
& $cli clip $qa\footage\a.mp4 -s 10 -d 4 -o $long
```

Check that no path rmedia prints starts with `\\?\`.

## 5. GIF and Reddit MP4 (Runbook §3.3)

```powershell
& $cli gif --list-targets
& $cli gif "$qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8 --dry-run
& $cli gif "$qa\Clips ünï 日本\vidéo à.mp4" -s 10 -d 8
& $cli gif $qa\footage\a.mp4 -s 10 -d 8 --target reddit-comment -o $qa\out
& $cli gif $qa\footage\a.mp4 -s 10 -d 8 --format mp4 -o $qa\out
& $cli gif $qa\footage\a.mp4 -s 10 -d 8 --target max-quality -o $qa\out    # expect ENGINE_FALLBACK (no gifski)
& $cli gif $qa\footage\a.mp4 -s 10 -d 6 --shape 9:16 --framing smart -o $qa\out
& $cli clip $qa\footage\a.mp4 -s 30 -d 6 --gif -o $qa\out
```

For each command, record:
- the summary line (size, width, fps, engine);
- the dry run's estimate against the real size.

Without a player, check that the MP4s have no audio track:

```powershell
$ff = (Get-ChildItem -Recurse $qa\cli -Filter rmedia-ffprobe.exe | Select-Object -First 1).FullName
Get-ChildItem $qa\out -Filter *.mp4 | ForEach-Object {
  $_.Name; & $ff -v error -show_entries stream=codec_type -of csv=p=0 "file:$($_.FullName)" }
```

Each MP4 should list only `video`. Playback and looping are checked in step 9.

## 6. Upscale and the AI Worker (Runbook §3.4)

This is the main check for this build: the Windows binaries link the C runtime statically, and
ncnn has no OpenMP.

```powershell
& $cli doctor --recheck
& $cli clip $qa\footage\a.mp4 -s 10 -d 6 --upscale -o $qa\out
Measure-Command { & $cli clip $qa\footage\a.mp4 -s 10 -d 6 --upscale --tier ai -o $qa\out\ai | Out-Host } |
  Select-Object TotalSeconds
& $cli upscale $qa\footage\a.mp4 --dry-run
```

Record:
- the `tier:` lines;
- whether the AI tier starts. On a machine without the VC++ runtime, there should be no missing
  `vcruntime140.dll`, `msvcp140.dll` or `vcomp140.dll`;
- the AI clip's `TotalSeconds`.

If you can, time the same clip with run 40452999345's CLI, which needs the VC++ runtime
installed. If this build is noticeably slower, the fallback is shipping the three DLLs next to
`rmedia-sr.exe`.

## 7. Cancel and Cleanup (Runbook §3.5)

```powershell
function Show-Left { Get-Process | Where-Object Name -like 'rmedia*' | Select-Object Name, Id
                     Get-ChildItem -Recurse $qa\out -Force -Filter *.part -ErrorAction SilentlyContinue }

# a) Ctrl+C during "encode" or "optimise"; it should return within 2 s
& $cli gif $qa\footage\a.mp4 --budget none -o $qa\out\cancel
Show-Left

# a, 002) The same with an upscale job
& $cli upscale $qa\footage\a.mp4 --tier cpu -o $qa\out\cancel4
Show-Left

# c) Kill only the parent
Start-Process $cli -ArgumentList 'gif', "$qa\footage\a.mp4", '--budget', 'none', '-o', "$qa\out\cancel3" -NoNewWindow
Start-Sleep 5; Stop-Process -Name rmedia -Force; Start-Sleep 2; Show-Left

# d) Ctrl+C while it is still scanning, before any job_started line; expect 130
& $cli gif C:\Users --json -o $qa\out\cancel5 ; $LASTEXITCODE
```

`Show-Left` should print nothing every time. Case (b), closing the window, needs the screen
(step 9).

Over SSH, Ctrl+C reaches the program through the SSH server, not a local console. A failure of
(a) or (d) here doesn't prove the same failure at a local console, so repeat a failing case at
the screen before reporting it.

## 8. SC-009 on Real Footage (Runbook §5)

Copy 8–12 real clips into `C:\rmedia-qa\footage` with `scp`. Each should be 6–10 s long, 720p
or larger, and the set should mix content (see runbook §5.1). List them, one row per clip:

```powershell
@'
file,start,duration
C:\rmedia-qa\footage\game.mp4,42,8
C:\rmedia-qa\footage\phone_party.mov,5,8
'@ | Set-Content -Encoding UTF8 $qa\footage\clips.csv
Get-Content $qa\Measure-Sc009.ps1          # read it before running it
powershell -ExecutionPolicy Bypass -File $qa\Measure-Sc009.ps1 -Cli $cli -Clips $qa\footage\clips.csv -Out $qa\sc009
```

It passes at 15% or more smaller over the whole set, with a worst VMAF loss of 1.0 or less.
Keep `C:\rmedia-qa\sc009\results.csv`.

## 9. Needs the Screen or RDP

- **Installers (runbook §4.1, §4.3):** whether they ask for admin, the SmartScreen warning, the
  Start menu entry, and the uninstall entry. Over SSH you can still check what the NSIS
  installer puts on disk with a silent install. It doesn't show the admin prompt or SmartScreen,
  so those still need the screen:

  ```powershell
  $setup = (Get-ChildItem -Recurse $qa\artifact\x -Filter '*-setup.exe').FullName
  Start-Process $setup -ArgumentList '/S' -Wait
  Get-ChildItem "$env:LOCALAPPDATA\rmedia" -Recurse | Select-Object FullName, Length
  ```

- **Desktop QA (runbook §4.2):** all twelve steps.
- **Cancel case (b) (runbook §3.5):** closing a terminal window while a job runs.
- **Playback:** the GIFs loop in Edge, the MP4s play, and the SC-009 pairs (`NN\plain` and
  `NN\gifsicle`) look right side by side.

## 10. Send the Results Back

Fill in the [results sheet](windows-owner-checks.md#results-sheet), then copy the two result
files back to the Linux machine:

```bash
scp <user>@<host>:C:/rmedia-qa/doctor.json <user>@<host>:C:/rmedia-qa/sc009/results.csv .
```

## Related

[Windows owner checks](windows-owner-checks.md) · [Build and Release](build-and-release.md) ·
[Roadmap](../roadmap.md)
