# Smart Cut Owner Checks: Windows Results (0.4.0)

The Windows part of the [results sheet](smart-cut-owner-checks.md#results-sheet), filled in from
the owner's PowerShell run on 2026-11-18 (working folder `%USERPROFILE%\rmedia-004`, the CLI
archive `rmedia-cli-v0.4.0-x86_64-pc-windows-msvc.zip` from the tag build's Windows artifact).
The machine's details come from the owner's second run the same day. The [Linux results](smart-cut-owner-checks-results-linux.md)
are in their own file.

```text
rmedia 0.4.0 smart cut owner checks — date: 2026-11-18  build (release.yml run): 44075993817 (tag v0.4.0, 092171f)

WINDOWS  version: Windows 11 Pro, 10.0.26300, 64-bit
         CPU/GPU: Intel Core i7-1185G7 @ 3.00 GHz (11th gen), 32 GB / Intel Iris Xe Graphics (driver 32.0.101.7092)
         WebView2 version: 157.0.4341.60
  digest matches ok (reported by the owner)
  4.1 version/doctor ok / 92% line ok (text garbled by the console, see notes) / 435 frames ok / clean decodes ok
      strict: error line ok exit 3 ok nothing written ok SMART_UNAVAILABLE/open_gop ok
      cancel (a) Ctrl+C __ s (not timed) ok (c) kill parent: no process ok cleaned by the next run ok
  4.2 D1 ticks ok D2 ok (owner, no screenshot) D3 bands + line ok (other range, see notes) D4 ok
      D5 seams ok D6 ok D7 ok (refused before the run) D8 ticks in __ s (within 2 s; exact time not given) ok (owner, no screenshot)
      D9 cmp identical ok
      W1 src_smart ok other_smart ok  W2 seeks ok  W3 Edge ok  W4 ok   (owner)

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
- Clean decodes: the Windows CLI block (4.1) had no decode loop at first; it was added on
  2026-11-18 and run the same day: "clean" for all six files (other_accurate.mp4,
  other_smart.mkv, .mov, .mp4, .ts, src_smart.mp4), no error lines.
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
  copy.ts, no engine process was left, and the next short clip deleted the folder.
- Cancel (c) cleanup on Windows: before, Show-Left still listed the scratch folder with its empty
  head.ts; the cleanup line (a fast clip of src.mp4, 1–2 s, into next\) ran; after it,
  Show-Left printed nothing: no process, and the hidden folder in cancel3\ was gone. The next run
  deletes a killed run's temp files on Windows too.
- Machine: Get-ComputerInfo gave "Microsoft Windows 11 Pro", OsVersion 10.0.26300, 64-bit,
  "11th Gen Intel(R) Core(TM) i7-1185G7 @ 3.00GHz", 33,706,872,832 B of memory (32 GB).
  Win32_VideoController listed "Intel(R) Iris(R) Xe Graphics" 32.0.101.7092 and also
  "Microsoft Remote Display Adapter" 10.0.26100.9818, which Windows adds for Remote Desktop
  sessions; if the desktop checks (4.2) run over Remote Desktop, note that with their results.
  WebView2 Runtime (machine-wide, EdgeUpdate pv): 157.0.4341.60.

- Desktop app (4.2), from the owner's screenshots and notes:
  - D1: src.mp4 open, 00:00:00.000 / 00:00:30.000; white ticks along the top of the timeline,
    one every 2 s. The time they took to appear is not in the screenshot.
  - D2: passed (owner); no screenshot was captured (the image sent for D2 was a copy of D1's).
  - D3: done with a different range from the runbook's: In 00:00:24.103, Out 00:00:30.000 (the end
    of the video) instead of 3.32–20.72 s. The line read "Smart: 34% copied · 1.90 s re-encoded
    at the start · 2.00 s at the end", with orange bands at both ends and a green one between.
    That is right for this range: the CLI dry run of src.mp4 -s 24.103 -e 30 gives head 1.897 s
    (47 fr), copy 2.000 s (50 fr), tail 2.000 s (50 fr), 34% copied. The runbook's 92% case was
    not shown on Windows. The In times (24.103, and 1.314 in D6) are not on 25 fps frame
    boundaries, so they were probably set by clicking the timeline rather than with the
    runbook's keys.
  - D4: "Finished 1/1 jobs", "1 succeeded", the row "Done src_001_00-00-24.103_00-00-30.000.mp4
    Smart · 34% copied". After the run the editor's In had moved to 00:00:30.000 and showed
    "start (00:00:30.000) is at or beyond the end of the video (00:00:30.000)": the check for a
    new clip starting at the very end, not a problem with the finished job.
  - D5: seams clean (owner).
  - D6: open.mp4, In 00:00:01.314, Out 00:00:06.762; orange text "Smart cut not possible: the
    video has open GOPs (frames that depend on the previous GOP); cutting in accurate mode"; no
    bands on the timeline. As expected.
  - D7: open.mp4's queue options with "Only smart (no fallback)" ticked (the chip reads "Smart
    only"); the queue shows "Fix these before running: smart cut not possible: the video has open
    GOPs (frames that depend on the previous GOP)". The app refuses the job before it runs, as
    the CLI does (exit 3 at planning), so nothing can be written. The runbook's expected text
    ("The clip fails with …") describes the same result.
  - D8: passed (owner); no screenshot was captured (the image sent for D8 was a copy of D7's).
  - D9: pass (owner).
- Observation (not a failure): with Out at the very end of the video, the last GOP (28–30 s) is
  re-encoded as the tail instead of copied, in the desktop app and the CLI alike. Copying a
  final GOP that runs to the end of the file would raise the copied share for such ranges; a
  possible later improvement.

- WebView2 checks W1–W4: the owner reported all passed: W1 src_smart.mp4 and other_smart.mp4
  play to the end in the app with clean seams; W2 frame-key seeks around the seams; W3
  other_smart.mp4 in Microsoft Edge; W4 Windows Media Player / Films & TV. This closes spike S1's
  WebView2 row.

Windows CLI (4.1): all checks passed. Desktop (4.2): D1–D9 and W1–W4 passed (D2 and D8 by
the owner's report, without screenshots). Windows: all checks passed.
```
