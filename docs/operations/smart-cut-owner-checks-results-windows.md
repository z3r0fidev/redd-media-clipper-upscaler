# Smart Cut Owner Checks: Windows Results (0.4.0)

The Windows part of the [results sheet](smart-cut-owner-checks.md#results-sheet), filled in from
the owner's PowerShell run on 2026-11-18 (working folder `%USERPROFILE%\rmedia-004`, the CLI
archive `rmedia-cli-v0.4.0-x86_64-pc-windows-msvc.zip` from the tag build's Windows artifact).
The machine's details were not in the output. The [Linux results](smart-cut-owner-checks-results-linux.md)
are in their own file.

```text
rmedia 0.4.0 smart cut owner checks — date: 2026-11-18  build (release.yml run): 44075993817 (tag v0.4.0, 092171f)

WINDOWS  version: ____  CPU/GPU: ____  WebView2 version: ____
  digest matches ok (reported by the owner)
  4.1 version/doctor ok / 92% line ok (text garbled by the console, see notes) / 435 frames ok / clean decodes ____ (not run)
      strict: error line ok exit 3 ok nothing written ok SMART_UNAVAILABLE/open_gop ok
      cancel (a) Ctrl+C __ s (not timed) ok (c) kill parent: no process ok cleaned by the next run ____ (not run yet)
  4.2 D1–D9: not run
      W1 src_smart ____ other_smart ____  W2 seeks ____  W3 Edge ____  W4 ____   (not run)

Notes:
- Test files (step 2.2) made with the bundled rmedia-ffmpeg.exe: src.mp4, other.mp4, open.mp4.
- Smart clips, 3.32–20.72 s: src_smart.mp4 and other_smart.mp4, other_smart.mkv, .mov and .ts,
  each "(smart, 92% copied)"; other_accurate.mp4 as the control. All 6 jobs succeeded.
- Sizes: other_accurate.mp4 8,636,089 B; other_smart.mp4 6,558,496 B, .mov 6,558,547 B,
  .mkv 6,552,429 B, .ts 6,814,812 B; src_smart.mp4 6,745,782 B (within 1% of the Linux run;
  the sources are encoded on each machine, so the bytes differ).
- version: "rmedia 0.4.0". doctor: "ffmpeg: …\rmedia-ffmpeg.exe (bundled)", "smart cut: available
  (libx264)", "status: ready".
- Dry run of src.mp4: "head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s
  (18 fr) re-encoded · 92% copied", the expected numbers. Through Select-String the console showed
  "→" as "ΓåÆ" and "·" as "┬╖": rmedia writes UTF-8 and Windows PowerShell decoded it with the
  console's OEM code page. Display only; setting
  [Console]::OutputEncoding = [Text.Encoding]::UTF8 before the command should show it correctly.
- Frame counts: 435 for all six files; other_smart.ts printed "435  435" (the expected double
  count for .ts).
- Clean decodes: the Windows CLI block (4.1) had no decode loop, so none were run; it was added
  on 2026-11-18.
- Strict mode: "error: command line: smart cut not possible: the video has open GOPs (frames that
  depend on the previous GOP)", "exit 3", strict\ empty; JSON "code":"SMART_UNAVAILABLE",
  "detail":{"reason":"open_gop"} (the error line is also printed on stderr).
- Cancel: the first try failed with "error: command line: source not found: …\long.mp4"
  because long.mp4 had not been made. The runbook's Windows cancel step only referred to the
  Linux command for long.mp4; on 2026-11-18 it got the PowerShell commands, and the second try
  used them.
- long.mp4: 1,031,901,679 B (10 min of 1080p).
- Strict mode, JSON line run again: same "SMART_UNAVAILABLE" / "open_gop" result.
- Cancel (a), Ctrl+C: pressed at 84% (375x, "0s left"); "cancelling… (press Ctrl-C again to
  quit immediately)", then "✗ …\cancel\long_001_00-00-03.300_00-09-50.700.mp4 cancelled",
  "0 succeeded, 0 skipped, 0 failed, 1 cancelled". Show-Left printed nothing (no rmedia process,
  nothing in cancel\). The time to return was not given.
- Cancel (c), kill the parent: Stop-Process printed no error, so rmedia.exe was killed while
  running. Show-Left listed no process: no rmedia-ffmpeg survived its parent. It listed the
  scratch folder cancel3\.long_001_00-00-03.300_00-09-50.700.rmedia-f614b6e0211ff7e0-1.d with an
  empty head.ts (0 B). A killed process cannot delete its own temp files; the next rmedia run
  deletes those of dead runs (the in-flight journal, FR-021). The runbook's pass rule for (c)
  wrongly failed any hidden folder; it was corrected on 2026-11-18 and got a cleanup line. On
  Linux (same release, kill -9 of the parent at 1.5 s and 2.5 s) the folder held head.ts and
  copy.ts, no engine process was left, and the next short clip deleted the folder. The Windows
  cleanup line has not been run yet.

Still to do on Windows: machine details (Windows version, CPU/GPU, WebView2 version); the (c)
cleanup line in 4.1 (expect Show-Left to print nothing, cancel3's hidden folder gone); the
clean-decode loop in 4.1; the desktop app D1–D9 and W1–W4.
```
