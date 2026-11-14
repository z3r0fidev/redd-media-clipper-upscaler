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
| `rotation` | a display rotation that the final mux cannot carry (R5) | probe side data |
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

## R3. The copied middle: split exactly on keyframes

**Decision**: copy the middle with the segment muxer. The command is `-ss <Kprev> -i file:<src>
-map 0:v:0 -c copy -f segment -segment_format mpegts -segment_times <K1−Kprev>,<K2−Kprev>`, and
the middle segment is kept. `Kprev` is a keyframe at or before K1, so the input seek lands on a
keyframe even in containers without an index. The segment boundaries cut on keyframes, so the
middle is exactly the GOPs [K1, K2).

**Measured**:
- A copy cut with `-ss K1 -t (K2−K1)` gave 402 frames instead of 400 (the B-frame reorder delay
  lets two extra frames through).
- The segment muxer gave exactly 100, 400 and 250 frames for splits at 4 s and 20 s of a 30 s
  source.
- The 400 copied frames were identical to the source when decoded (framemd5, 400 of 400).

**Rationale**: GOP-exact boundaries make the frame count predictable, which the verification
needs (R7).

**Alternatives considered**: `-ss`/`-t` copy cuts are off by the reorder delay. A packet-level
copy would need our own muxer.

## R4. The re-encoded head and tail: matching the source

**Decision**:
- Encode the head [A, K1) and the tail [K2, Z) with libx264 from an exact seek (`-ss` before
  `-i`, as in accurate mode). Stop at an exact frame count (`-frames:v N`, N from the index)
  rather than a duration.
- Match the source's parameters:
  - `-profile:v` and `-level` from the source;
  - `-pix_fmt yuv420p` (or `yuvj420p` with full range);
  - `-x264-params ref=<refs>:bframes=<0 if the source has none, else 3>:open-gop=0:stitchable=1`;
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
- For mp4, m4v and mov, write the `avc3` sample entry (`-tag:v avc3`), which allows parameter sets
  to be carried in-band and to change between parts.
- mkv and ts carry in-band parameter sets natively.

**Measured**:
- Joining mp4 parts failed: "non monotonically increasing dts" at a seam.
- Joining TS parts into mp4, mov, mkv or ts decoded with 0 errors.
- The joined mp4 holds 12 in-band SPS/PPS against the source's 1.
- `-tag:v avc3` with `+faststart` was accepted and decoded with 0 errors.

**Player compatibility [verify, Phase 0 spike S1]**: FFmpeg decodes both the `avc1` form (in-band
changes) and the `avc3` form. Other players must be checked:
- the desktop app's webviews (WebKitGTK on Linux, WebView2 on Windows);
- Chrome, Firefox and VLC;
- an Android phone and an iPhone;
- a Reddit upload (an owner check).

If `avc3` turns out to be less compatible than `avc1` with in-band parameter sets, the spike
switches the default. The choice is one constant plus a snapshot.

**Display rotation [verify, Phase 0 spike S2]**: the TS parts carry no display matrix. The final
mux must set the source's rotation again (the `-display_rotation` input option on the joined
input). If that does not work for every output container, rotated sources fall back with the
`rotation` reason.

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

**Decision**: three checks run on the joined `.part` file, all through `exec::process`:
1. **Packet scan** (no decoding): the video frame count equals head + copy + tail; the pts
   values, once sorted, increase by one frame duration (±1 tick) across both seams; the first
   pts is 0 and the duration is within one frame of Z − A.
2. **Seam decode**: decode the GOP before and the GOP after each seam with `-v error`. There
   must be no error lines, the decoded frame count of each window must be as planned, and every
   frame must have the stream's size and pixel format (`params_changed` otherwise).
3. **Copy identity**: the framemd5 of the first and last copied GOP equals the source's for the
   same frames. These are the GOPs where a broken reference would show.

A failure discards the `.part` file. The job then runs again in accurate mode with the reason
(`params_changed` or `seam_check`), or fails under strict mode.

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
