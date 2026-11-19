# Smart Cut Owner Checks: Android Results (0.4.0, emulator)

The Android part of the [results sheet](smart-cut-owner-checks.md#results-sheet), run by Claude on
2026-11-18 in the Android Studio emulator on the Linux machine (the Dell Latitude 5440), not on a
phone. The clips are the Linux run's (`~/rmedia-004/out`, from the v0.4.0 tag build, release.yml
run 44075993817). The [Linux](smart-cut-owner-checks-results-linux.md) and
[Windows](smart-cut-owner-checks-results-windows.md) results are in their own files.

**What an emulator does and does not show.** It runs Android's real media stack (MediaCodec,
Codec2, the apps' own players) with two H.264 decoders: `c2.goldfish.h264.decoder` (the emulator's
decoder, backed by the host) and `c2.android.avc.decoder` (Google's software decoder, which real
phones also carry). A phone normally decodes with its chip's hardware decoder (Qualcomm, Exynos,
Tensor, MediaTek), which the emulator does not have, and that is where a mid-file change of
parameter sets is most likely to trip. These results therefore cover Android's players and
software path; a check on a real phone is still needed for the hardware decoders.

```text
rmedia 0.4.0 smart cut owner checks — date: 2026-11-18  build (release.yml run): 44075993817 (tag v0.4.0, 092171f)

ANDROID  model: emulator, AVD Pixel_10 (sdk_gphone64_x86_64)  Android version: 16 (API 36), Google Play image
                    src_smart.mp4   other_smart.mp4   other_accurate.mp4   duration shown
  Google Photos     ok              ok                ok                   17.40 s (MediaStore 17400 ms)
  Files by Google   skipped (not installed on the emulator image)
  Chrome            ok              ok                ok                   17.4 s (video.duration)
  VLC (optional)    skipped (not installed)

  Real phone (hardware decoder): ok (owner, 2026-11-18; model and Android version not given)
```

## How It Was Checked

- **Setup.** `Pixel_10` booted headless (`-no-window -read-only -no-snapshot`, 2 GB, software
  GPU), so the AVD itself was not changed. The three clips were copied with `adb push` to
  `/sdcard/Download/rmedia-004/`; Android's media index read all three as 17,400 ms.
- **Chrome** (plays H.264 through Android's MediaCodec). The page was served from the computer
  through `adb reverse` and Chrome was driven over DevTools. For each clip: the length, media
  errors, `getVideoPlaybackQuality()`, and every frame presented in a full play-through
  (`requestVideoFrameCallback`); then slow playback (0.25x) through windows around both seams,
  drawing each presented frame to a canvas and saving it. Each saved frame was compared with
  FFmpeg's decode of the same clip.
- **Google Photos.** Each clip was opened with a VIEW intent and played while `screenrecord`
  recorded the screen and logcat was saved. Each recorded frame was identified by the counter
  that `testsrc2` burns into the picture (black-and-white digit masks matched against the clip's
  own counters, checked by eye on five frames), then compared as a whole with that exact clip
  frame.

## Results

**Chrome**, all three clips:
- `video.duration` 17.4 s; playback reached the end (`ended`); no media errors;
  `corruptedVideoFrames` 0; frame times never went backwards.
- Captured frames: `src_smart.mp4` 22 (both seams: frames 13, 15, 17, 18, 20 and 414, 415, 416,
  418, 419, 420); `other_smart.mp4` 371 (a slow pass ran through the whole clip; seam frames 13,
  15–18, 21 and 413, 415, 417–419, 421); `other_accurate.mp4` 342. Every capture is a correctly
  decoded frame: at full size the `src_smart.mp4` captures match FFmpeg's frame of the same number
  at about 46 dB against 15–17 dB for the neighbouring frames; across all 735 captures the best
  match is one frame, 8.7 dB or more above the next. In the long slow passes the canvas sometimes
  drew a frame one to three frames after the time Chrome reported, equally in the control, so
  that is the capture timing, not the files.
- Dropped frames (142–160 per play-through) are the emulator's software graphics at 720p: they are
  spread over the whole clip from the first second, and as many in the control.

**Google Photos**, all three clips:
- Each clip played from the start to the last frame and then looped (Photos repeats short
  videos); the only backward step in each recording is the loop (434 → 0/1).
- Decoders in logcat: `c2.goldfish.h264.decoder` for all three, and also
  `c2.android.avc.decoder` for `src_smart.mp4`. No decoder errors (only capability queries and
  thumbnail messages).
- Every recorded frame that shows the clip (253, 257 with a read counter and 301 for the three
  clips) matches its exact clip frame at 29.5–31.6 dB (screen recording and scaling set that
  level), with no low frames, at the seams as elsewhere.
- `other_smart.mp4` seam frames seen and looked at: counters 99, 100, 101 (clip frames 16, 17,
  18; 17 is the first copied frame) and 498, 500, 502 (415, 417, 419; 417 is the first
  re-encoded tail frame). Clean: no blocks, no grey or green patches, the moving pattern
  continuous.

Not checked here: sound (the emulator ran without audio), Files by Google and VLC (not on the
image), and a phone's hardware decoder.

Android (emulator): all checks run passed. Real phone: the owner reported on 2026-11-18 that the
checks passed on an Android phone, which covers its hardware decoder. Android: all checks passed.
