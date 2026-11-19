# Research: Smart Cut

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Date**: 2026-11-14

Background: `research/01-clipping-and-discovery.md` §1.4 (the algorithm and its pitfalls), §1.5
(audio sync, VFR) and §1.7 (smart as a third mode with automatic fallback).

Every finding marked **measured** was run on 2026-11-14 with the bundled engine (FFmpeg
n9.0.2-31, libx264). The test sources were made with lavfi: `testsrc2` at 1280x720 and 1920x1080,
25 fps, x264 with `keyint=50:min-keyint=50:scenecut=0:bframes=3` (one variant also `ref=4` with
preset slow, another `open-gop=1`), and sine AAC audio. Items marked **[verify]** are Phase 0
spikes in the plan.

## R1. The keyframe index

**Decision**: read the first video stream's packets with ffprobe (`-select_streams v:0
-show_entries packet=pts,dts,flags`, JSON, no decoding). A packet with the `K` flag is a
keyframe. For a plan, read a window around the range: from the keyframe before the start
(001's `keyframe_at_or_before` already finds it) to one GOP after the end, using
`-read_intervals`. For the desktop markers, read the whole file once and cache it per source.

**Rationale**:
- **Measured**: a whole-file packet scan of a 10-minute, 400 MB 1080p source takes 0.10 s for
  mp4 and mkv (index-backed) and 0.35 s for ts (a full read). That meets SC-007 (2 s) on a warm
  disk. For a cold disk, the desktop shows markers as they arrive.
- Packet flags need no decoding, and 001 already parses ffprobe packet JSON.
- The output is bounded: the window caps the packet count for plans. The desktop index is capped
  at 20,000 keyframes and 30 s, and is marked `truncated` when a cap is hit.

**Alternatives considered**:
- `-show_frames -skip_frame nokey` decodes keyframes: slower, and it gives no decode order,
  which the open-GOP rule (R2) needs.
- Container indexes (mp4 `stss`) cannot be read without our own demuxer, and there is none for
  ts.

## R2. Eligibility: which sources are smart-cut, and the fallback reasons

**Decision**: a pure function `smart::check(media, index, range, caps)` returns the plan or a
`FallbackReason`. It checks the rules below in this order and stops at the first failure. Each
reason has a stable `snake_case` value (data-model › FallbackReason).

| Reason | Rule | Data |
|---|---|---|
| `encoder_missing` | the engine has no `libx264` | `Caps.encoders` |
| `codec` | the video codec is not `h264` (HEVC and AV1 included) | probe `codec_name` |
| `container` | the source or output container is not mp4, mov, mkv or ts | probe `format_name`, output extension |
| `profile` | the H.264 profile is not Constrained Baseline, Baseline, Main or High | probe `profile` |
| `pixel_format` | the pixel format is not `yuv420p` or `yuvj420p` (10-bit, 4:2:2, 4:4:4) | probe `pix_fmt` |
| `interlaced` | the field order is `tt`, `bb`, `tb` or `bt` | `StreamInfo::is_interlaced` |
| `vfr` | `avg_frame_rate` differs from `r_frame_rate` (`is_vfr`), or packet spacing in the window is uneven by more than one timebase tick | probe and index |
| `rotation` | the source has a display rotation and the output container is ts, which cannot carry it (R5, spike S2) | probe side data, output extension |
| `no_full_gop` | there is no keyframe pair K1 < K2 inside the range | index |
| `open_gop` | a packet after K1 or K2 in decode order has a pts below that keyframe's pts | index |
| `params_changed` | the frames at a seam do not have the stream's size and pixel format (checked after encoding, R7) | seam check |
| `seam_check` | the joined result fails verification (R7) | seam check |

**The open-GOP rule (measured)**: in a closed-GOP x264 source, the packets that follow a
keyframe in decode order all have a later pts (keyframe at 2.000; next packets 2.080, 2.040,
2.160 …). In the `open-gop=1` source, the packet right after the keyframe has pts 1.960, a
leading picture that references the previous GOP. The mp4 muxer marks those recovery-point I
frames as keyframes, so the `K` flag alone is not enough. The rule needs only the index, and it
is checked at both copy boundaries.

**Rationale**: every rule runs before encoding except the last two, so the dry run can show most
fallbacks (FR-011, US2 scenario 5). Running the cheap probe rules first gives the most specific
reason.

**Alternatives considered**: parsing NAL unit types (IDR or not) with the `trace_headers`
bitstream filter. It is accurate but expensive, and the decode-order rule catches the case that
matters (leading pictures) with data we already have.

## R3. The copied middle: from K1, exactly the GOPs [K1, K2)

**Decision** (revised in US1, 2026-11-15): a stream copy in the way fast mode (001) copies,
ended by a packet count:
`-ss <K1 − 10 s> -i file:<src> -ss <K1 − lead> -map 0:<v> -c copy -frames:v <N> -f mpegts copy.ts`.
- The input seek is coarse (as accurate mode's on MPEG-TS) and may land anywhere before K1.
- The output seek is measured from the input seek point. A stream copy drops packets by decode
  time and then waits for a keyframe, so it cuts `lead` before K1: more than the B-frame delay
  (max(4, `has_b_frames` + 2) frames; K1's packet has a dts up to that much before its pts) and
  less than half the GOP before K1 (that keyframe must not be kept).
- `-frames:v N` with N = the frames in [K1, K2) ends the copy on the last packet before K2's
  keyframe: with closed GOPs, every frame of [K1, K2) comes before K2 in decode order. A copy
  that runs to the end of the video has no `-frames:v`.

**Measured** (bundled FFmpeg n9.0.2, `copytest` in the US1 session): 200 of 200 frames copied and
identical to the source (framemd5) for K1 = 2, 4 and 14 s on `SmartMkv`, `SmartMp4`, `SmartTs`,
`OffsetTs` and `OtherEncoder`.

**The segment muxer, first chosen here, was dropped**:
- `-segment_times` are measured from wherever the demuxer's seek landed. Matroska seeks land
  early: a seek to 2 s landed on the keyframe at 0 s, and one to 4.02 s on the keyframe at 2 s.
  The "middle" segment then began a GOP early. The seam check (R7) caught it: the copied
  frames were not the source's.
- The segment list reports times relative to the landing for Matroska, but absolute for MP4 and
  MPEG-TS sources.
- Spike S3's MPEG-TS result (250 of 250 frames) only held because those seeks landed exactly.
- A copy cut with `-ss K1 -t (K2−K1)` gives 402 frames instead of 400 (the B-frame reorder delay
  lets two extra frames through), and an output seek right at K1 drops K1 itself (its dts is
  before its pts); both are why the lead and the packet count are used.

**Rationale**: no step depends on where a demuxer seek lands; the frame count is exact, which
the verification needs (R7).

**Alternatives considered**: the segment muxer (above); no input seek at all (exact, but reads
the file from its start); a packet-level copy would need our own muxer.

## R4. The re-encoded head and tail: matching the source

**Decision**:
- Encode the head [A, K1) and the tail [K2, Z) with libx264 from an exact seek (`-ss` before
  `-i`, as in accurate mode). Stop at an exact frame count (`-frames:v N`, N from the index)
  rather than a duration.
- Match the source's parameters:
  - `-profile:v` and `-level` from the source;
  - `-pix_fmt yuv420p` (or `yuvj420p` with full range);
  - `-x264-params bframes=<0 if the source has none, else 3>:open-gop=0:stitchable=1`
    (references are not matched, see below);
  - the colour tags (primaries, transfer, matrix, range) copied from the probe;
  - the SAR set with `setsar`;
  - `-fps_mode passthrough`.
- Quality is accurate mode's: preset medium, CRF 18.
- Each part is written as MPEG-TS (Annex B, with parameter sets in-band).

**Measured**:
- Head and tail had exactly the planned 17 and 18 frames.
- The first frame matched the source frame at the requested start (PSNR 45.8 dB, against
  22.1 dB for the neighbouring frame), and so did the last frame at the requested end (45.4 dB).
- With matched `ref` and `bframes`, the head's SPS was identical to the source's. Its PPS differed
  in two fields (`num_ref_idx_l0_default_active_minus1` and `pic_init_qp_minus26`).

**Reference count (US1)**: ffprobe does not report it, and the SPS field
(`max_num_ref_frames`) does not map onto x264's `ref` (a veryfast source with B-pyramid shows 4).
It is not matched: the parts carry their own parameter sets in-band (R5), and spike S1 showed
decoders handle the change, so x264's default for preset medium is used.

**Rationale**:
- `stitchable=1` stops x264 from tailoring headers to the content.
- Matching profile, level, references and B-frames keeps the decoder's buffering the same across
  the seams.
- Identical parameter sets cannot be guaranteed in general (other encoders, other settings), so
  R5 makes the join safe for parameter-set changes instead.

**Alternatives considered**:
- x264 `--sps-id` with several parameter sets in the `avcC` box: FFmpeg's mp4 muxer writes the
  first part's extradata only.
- Copying the source's encoder settings exactly is not possible in general (research/01 §1.4).

## R5. Joining, and the output container

**Decision**:
- Join the TS parts with the concat demuxer, `-c copy`, then mux them with the audio (R6) into
  the output container in the same step.
- For mp4, m4v and mov, keep FFmpeg's default `avc1` sample entry. The `avcC` box holds the head's
  parameter sets, and every part also carries its own in-band, so they may change between parts
  (spike S1 chose `avc1` over `avc3`).
- mkv and ts carry in-band parameter sets natively.
- A rotated source: the head and tail are encoded with `-noautorotate`, and the join sets
  `-display_rotation` on the joined input (spike S2).

**Measured**:
- Joining mp4 parts failed: "non monotonically increasing dts" at a seam.
- Joining TS parts into mp4, mov, mkv or ts decoded with 0 errors.
- The joined mp4 holds 12 in-band SPS/PPS against the source's 1.
- `-tag:v avc3` with `+faststart` was accepted and decoded with 0 errors.

**Spike S1, player compatibility (2026-11-15)**: a smart cut of an `OtherEncoder`-like source
(1280×720 25 fps, x264 preset slow, `ref=4:bframes=5`, 2 s closed GOPs, AAC), range 3.32–14.72 s:
head 17 + copy 250 + tail 18 = 285 frames. Head and tail used `ref=4:bframes=3`, so their SPS/PPS
differ from the source's. Controls: an accurate-mode encode of the same range (`acc.mp4`), its
`avc3` remux and its ts remux.

| Player | mp4 `avc1` | mp4 `avc3` | mkv | ts | How it was checked |
|---|---|---|---|---|---|
| FFmpeg n9.0.2 | ok | ok | ok | ok | 285 frames, 0 decode errors; copied frames framemd5-identical |
| GStreamer 1.24.2 (WebKitGTK's backend, desktop app on Linux) | ok | ok | ok | ok | `uridecodebin`: 285 frames, no errors or warnings |
| VLC 3.0.24 (Flatpak) | ok | **fails** | ok | ok | decoded to raw frames and matched to FFmpeg's frames by content: every frame VLC output is identical and in order. VLC's transcode path drops frames 1–2 of any mp4 and the last frame of any ts, the controls included. `avc3` gives 0 frames ("buffer deadlock prevented") for the plain control too |
| Firefox 157 (Linux) | ok | ok | ok | not playable | 285 frames presented, 0 dropped at 1×; seam frames (16–18, 266–268) and frame 100 captured after seeks match FFmpeg's frames as closely as the control's do. ts is not a browser format |
| Chrome 157 (Linux) | ok | **fails** | ok | not playable | 285 frames, 0 dropped, 0 corrupted; full-size captures of frames 16, 17, 100, 266, 267 and 284 match FFmpeg's frame at 29.2–29.6 dB (the control: 29.3 dB; neighbouring frames ≈16–18 dB). `avc3` files (the plain control too) never reach `ended` |
| WebView2 157 (Windows 11), owner checks 2026-11-18 | ok | — | — | — | the desktop app's player and Edge: plays to the end, clean seams and seeks (W1–W4, [Windows results](../../docs/operations/smart-cut-owner-checks-results-windows.md)) |
| Android 16 emulator: Chrome, Google Photos (2026-11-18) | ok | — | — | — | Chrome: every captured seam frame matches FFmpeg's frame; Photos: recorded frames identified by their counter match their clip frame, seams clean; decoders `c2.goldfish.h264` and `c2.android.avc` ([Android results](../../docs/operations/smart-cut-owner-checks-results-android.md)) |
| Android phone, iPhone, Reddit upload (owner, 2026-11-18) | ok | — | — | — | reported as passed by the owner; no model or version details ([iPhone](../../docs/operations/smart-cut-owner-checks-results-iphone.md), [Reddit](../../docs/operations/smart-cut-owner-checks-results-reddit.md) results) |

Decision: **`avc1`** with in-band parameter sets is the default for mp4, m4v and mov; `avc3` is
not used (VLC 3 and Chrome fail on any `avc3` file). Giving the head and tail their own parameter
set ids (x264 `sps-id`) made no difference in Chrome and is not needed.

A false alarm, kept for the record: a 160×90 capture made in Chrome right after each seek
pointed at wrong frames for the smart files. Full-size captures showed the decode is right; the
small capture is not a reliable probe in Chrome.

**Spike S2, display rotation (2026-11-15)**: a 640×360 source with a 90° display matrix, range
1.32–9.72 s (17 + 150 + 18 frames).
- With FFmpeg's default autorotation, the head and tail are encoded upright (360×640) while the
  copied middle stays 640×360: the size changes mid-stream (R7 would report `params_changed`).
- With `-noautorotate` on the head and tail inputs, every part is 640×360, and
  `-display_rotation 90` on the joined input writes the matrix into mp4, mov and mkv (ffprobe:
  640×360, rotation 90; 185 frames; 0 decode errors).
- ts has no display matrix, so a rotated source cut to a ts output falls back with `rotation`.
  For every other output container, rotation is carried.

## R6. Audio and the other streams

**Decision**:
- Re-encode audio once, for the whole range [A, Z), with accurate mode's audio settings for the
  output container (`EncodeProfile`).
- Mux it with the joined video in the final step.
- Stream selection (`StreamPlan`), metadata and chapters (dropped) are as in accurate mode. Text
  subtitles are cut by time, as in accurate mode.

**Measured**: video and audio both started at 0.000 and lasted 17.400 s (435 video frames and 751
AAC frames for a 17.4 s range).

**Rationale**: audio is cheap to encode, and a single encode avoids AAC priming gaps at the seams
(research/01 §1.5). That matches the spec's assumption.

**Alternatives considered**: smart-cutting audio as well, by copying packets in the middle. That
needs priming and padding handling at each seam, for very little saving.

## R7. Verification before commit (FR-010)

**Decision** (as built in US1): two checks run on the joined `.part` file, all through
`exec::process`:
1. **Packet scan** (ffprobe, no decoding): the video packet count equals head + copy + tail,
   and the sorted pts step by one frame duration (± max(2 ms, 5 %)) everywhere.
2. **Seam windows** (ffmpeg `-v error -noautoscale -f framemd5`): around each seam, decode up to
   one GOP before and one after (an exact input seek; MPEG-TS outputs seek on the output side).
   - Any error line, or a window with too few frames: `seam_check`.
   - Error lines count for the joined clip only. Decoding the source after a coarse seek
     (MPEG-TS) can report "co located POCs unavailable" for B-frames before the window; CI's
     x264 (more threads, other frame-type choices) made such sources, and every smart cut of
     them fell back until the source decode's messages were ignored. Its frames are compared
     by MD5.
   - A frame whose decoded size is not the stream's (`-noautoscale` keeps a size change
     visible): `params_changed`.
   - The window's copied frames must have the same MD5s as the same frames decoded from the
     source (the first copied GOP at the first seam, the last one at the second): otherwise
     `seam_check`. This is where a broken reference or wrong parameter set shows; it is the
     check that caught the segment-muxer error (R3).

A failure discards the `.part` file. The job then runs again in accurate mode with the reason
(`params_changed` or `seam_check`) and a `SMART_FALLBACK` warning, or fails with
`SMART_UNAVAILABLE` under strict mode.

**Measured**: a full decode of a 60 s 1080p result takes 1.2 s; the two seam windows take 0.4 s.
Seam windows keep verification cheap on long clips.

## R8. Speed

**Measured** on a 12-core machine, for a 60.4 s range of a 75 s 1080p source:
- accurate mode took 11.3 s;
- smart took 4.3 s, which included a full decode check (38%), or about 3.1 s (27%) with
  seam-window checks only.

testsrc2 is the easiest content for accurate mode's encoder, so real footage favours smart even
more. SC-005 is therefore set at one third of accurate mode's time, measured on the SC-005 bench
fixture. Copy and seam checks are I/O-bound and do not grow with resolution the way encoding
does.

## R9. Progress, cancellation and scratch

**Decision**:
- A smart job has the stages `head`, `copy`, `tail`, `audio`, `join` and `verify`. Each runs one
  ffmpeg or ffprobe process through `exec::process`.
- Progress is weighted by estimated cost: encoded frames 1.0 each, copied frames 0.02, audio
  0.05 per second, join 0.02 per frame, verify 0.3 per decoded frame. ETA is computed the same way
  as in 001.
- Parts go in the job's scratch folder (002's `create_scratch`, journaled). The folder is removed
  on success, failure and cancel, so the 2 s cancel bound holds (one process runs at a time).

## R10. Determinism

**Decision**: smart output is deterministic per machine, as accurate mode is: x264 runs with its
default deterministic setting, and the copy and join steps are byte-stable. The byte-identity test
runs a smart cut twice on one machine (Principle II).

## R11. Desktop: markers and the live split

**Decision**:
- A new core function, `smart::keyframes`, builds the index (R1). The desktop calls it through a
  `keyframes` command, cached per source in `AppState`, and draws markers on the Player timeline.
- A `smart_plan` command calls the same core planning as `rmedia clip --mode smart --dry-run` for
  the selected range. It returns the head, copy and tail spans, the copied share, and any fallback
  reason, so the app never computes these itself (Principle I).
- Both commands are cancellable. A new request cancels the previous one, as with `gif_estimate`.
