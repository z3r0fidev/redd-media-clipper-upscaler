# Smart Cut Owner Checks: Linux Results (0.4.0)

The Linux part of the [results sheet](smart-cut-owner-checks.md#results-sheet), filled in from
the owner's terminal run on 2026-11-18. The system details come from the same machine (the Dell
Latitude 5440). The other platforms get their own results files.

```text
rmedia 0.4.0 smart cut owner checks — date: 2026-11-18  build (release.yml run): 44075993817 (tag v0.4.0, 092171f)

LINUX  distro/version: Ubuntu 24.04.5 LTS  desktop X11/Wayland: X11 (Cinnamon)
       CPU/GPU: Intel Core i7-1355U (13th gen) / Intel UHD Graphics (Raptor Lake-P), Dell Latitude 5440
  3.1 digest matches ok (reported by the owner)
  3.2 version 0.4.0 ok / doctor bundled + smart available ok / 92% line ok
      open GOP fallback line ok / 435 frames all files ok / clean decodes ok
      strict: error line ok exit 3 ok nothing written ok SMART_UNAVAILABLE/open_gop ok
      cancel: elapsed 3.05 s (minus 3 s = 0.05 s to stop), nothing left ok
  3.3 D1 ticks ok D2 ok D3 bands + line ok D4 ok D5 seams ok D6 ok D7 ok
      D8 ticks in __ s (within 2 s; exact time not given) ok D9 cmp identical ok

Notes:
- Test files (step 2.1) made with the bundled rmedia-ffmpeg: src.mp4, other.mp4, open.mp4.
- Smart clips, 3.32–20.72 s: src_smart.mp4 and other_smart.mp4, other_smart.mkv, .mov and .ts,
  each "(smart, 92% copied)"; other_accurate.mp4 as the control. All 6 jobs succeeded.
- Sizes: other_accurate.mp4 8,819,145 B; other_smart.mp4 6,577,558 B, .mov 6,577,609 B,
  .mkv 6,571,439 B, .ts 6,834,176 B; src_smart.mp4 6,723,739 B.
- doctor: "ffmpeg: …/rmedia-ffmpeg (bundled)", "smart cut: available (libx264)", "status: ready".
- Dry run of src.mp4: "head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s
  (18 fr) re-encoded · 92% copied", exactly as expected.
- open.mp4 dry run: "smart → accurate: the video has open GOPs (frames that depend on the
  previous GOP)". The fallback dry run was run twice; the --no-smart-fallback line (expected:
  error, exit 3) was not run.
- Frame counts: 435 for all six files; other_smart.ts printed 435, an empty line and 435 again
  (the expected double count for .ts).
- Decodes: "clean" for all six files.
- Cancel: long.mp4 (10 min, 1080p), smart 3.3–590.7 s; Ctrl+C at 13% (126x, "4s left"). The
  job reported "cancelled" (0 succeeded, 1 cancelled); cancel/ was empty (no .part, no scratch
  folder) and pgrep found no rmedia or ffmpeg process. The time to return was not measured (timed below).
- Desktop app (section 3.3): the owner reported all steps D1–D9 passed.
- Strict mode and the timed cancel were run by Claude on the same machine, in ~/rmedia-004 with
  the same release binary, following the runbook steps added on 2026-11-18:
  strict: "error: command line: smart cut not possible: the video has open GOPs (frames that
  depend on the previous GOP)", exit 3, strict/ empty, JSON "code":"SMART_UNAVAILABLE",
  "detail":{"reason":"open_gop"}.
  timed cancel: timeout -s INT 3 (SIGINT, as Ctrl+C) → "cancelled", summary "1 cancelled",
  elapsed 3.05 s (stopped 0.05 s after the signal), cancel/ empty, no rmedia or ffmpeg process.

Linux: all checks passed.

The Windows artifact's digest also matched (reported by the owner); it goes in the Windows
results.
```
