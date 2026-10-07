# 08 — Smart reframe (feature 002): landscape -> portrait with content tracking

Date: 2026-10-06. Tags: [V] verified with URL / local test, [S] source-stated by a secondary page, [K] known from
background knowledge, NOT re-verified, [unverified] open item.
Local ffmpeg used for tests was 6.1.1 (not the bundled FFmpeg 9); re-run the tests on the bundled build.

## 1. Prior art
- AutoFlip (MediaPipe): [V] https://github.com/google-ai-edge/mediapipe/blob/master/docs/solutions/autoflip.md
  - Status: legacy; "support ended March 1, 2023". Treat as frozen reference, not a dependency.
  - Licence: doc page doesn't state it; MediaPipe is Apache-2.0 [K]. Verify LICENSE before copying any code.
  - Pipeline [K/V]: shot-boundary detection -> per-frame "signals" (face detection, object detection, optional
    saliency) -> per-shot "key frame" regions with scores/required-vs-optional flags -> a camera path solver ->
    crop/pad render. Motion modes: stationary (one fixed window per shot, picked to cover the union of required
    regions), tracking (follows content, smoothed), and a stabilize/"snap to center" threshold
    (`motion_stabilization_threshold_percent`) [V]. Fallback when content can't fit: padding with blurred/solid
    background (a first-class AutoFlip option) [K].
  - Not usable without MediaPipe (C++/Bazel graph); the algorithm is easy to re-implement (~500 LOC) and that is the plan.
- Adobe Premiere "Auto Reframe" (Sensei): per-shot subject tracking with motion presets (slower/default/faster)
  [K]. Useful UX idea: a 3-level "motion" knob that maps to smoothing strength / max pan speed.
- YouTube/Grundmann et al. "Auto-directed video stabilization with robust L1 optimal camera paths" (CVPR 2011):
  camera path = piecewise constant / linear / parabolic segments minimising L1 of 1st/2nd/3rd derivative; applied
  to cropping it gives the "static, then smooth pan, then static" look of a human operator [K]. (Needs an LP
  solver; overkill for v1 — an approximation is in section 3.)
- Other OSS (smaller, lower confidence): OpenCV/YOLO/face-based "auto reframe" scripts, `autocrop`-style tools,
  Vidgear/MoviePy hacks [K, names unverified]. Academic: "Deep Learning for Automatic Video Reframing / Video
  retargeting", saliency-based seam-carving — heavy and unneeded [K].
- Cheap and good [S/K]: face + person detector at 2-5 fps, union of boxes per shot, heavy temporal smoothing,
  dead-zone (don't move unless target leaves a central band), max pan speed. That is ~90% of AutoFlip's perceived quality.

## 2. Detectors and runtimes

| Model | Task | Licence | Size / note |
|---|---|---|---|
| YuNet | face | repo BSD-3-Clause [V https://github.com/ShiqiYu/libfacedetection.train]; OpenCV Zoo copy under Apache-2.0 repo [V https://github.com/opencv/opencv_zoo] | ~230 KB int8 / ~340 KB fp32 [K]; verify per-model LICENSE |
| BlazeFace / MediaPipe face | face | Apache-2.0 [K] | tiny, but ONNX conversions are third-party |
| SCRFD (InsightFace) | face | code MIT, **pretrained models non-commercial** [K] | avoid |
| Ultra-Light-Fast (RFB-320) | face | MIT [K] | 1 MB; lower accuracy than YuNet |
| YOLOX (Megvii) | person/object | Apache-2.0 [K]; in OpenCV Zoo [V] | nano ~0.9M params, tiny ~5M |
| NanoDet(-Plus) | object | Apache-2.0 [K]; in OpenCV Zoo [V] | ~1-4 MB |
| PP-PicoDet | object | Apache-2.0 [K]; referenced in Zoo [V-ish] | ~1-4 MB |
| MediaPipe PersonDet | person | Apache-2.0 [K]; in Zoo [V] | |
| RT-DETR | object | Apache-2.0 [K] | too heavy for CPU sampling |
| MobileNet-SSD | object | Apache-2.0 [K] | older, weaker |
| U2-Netp | saliency | Apache-2.0 [K] | ~4.7 MB; slower (~100s ms CPU @320) [K] |
| Ultralytics YOLOv5/8/11 | | **AGPL-3.0** [K] — exclude (GPL FFmpeg bundle does not make AGPL OK; also taints distribution) |

Check the licence of the *weights* (not just the code) for each model before bundling [unverified for all rows].
- Runtimes:
  - `ort` 2.0.0-rc.13, MIT OR Apache-2.0, wraps ONNX Runtime 1.28 [V https://crates.io/api/v1/crates/ort]. Still
    release-candidate after years. Fast (SIMD), but needs onnxruntime shared lib (~10-20 MB CPU-only, per OS) via
    download-binaries or load-dynamic; extra packaging and DLL handling on Windows [K].
  - `tract-onnx` 0.23.8, MIT OR Apache-2.0, pure Rust, no native lib [V https://crates.io/api/v1/crates/tract-onnx].
    Handles small CNNs (YuNet, YOLOX-nano, NanoDet) usually; operator coverage gaps possible (must test each model)
    [K]. 1.5-3x slower than ORT on CPU [K].
  - `opencv` crate: needs system/bundled OpenCV (huge, C++ build) — reject for "lean".
- CPU budget [K, unverified here]: YuNet @320x180-ish ~2-5 ms; YOLOX-nano @416 ~15-40 ms on desktop cores. A 10-min
  clip at 3 fps = 1800 frames -> well under a minute even with tract.
- Recommendation: `tract-onnx` first (zero native deps), behind a `Detector` trait in rmedia-core so `ort` can be
  swapped in. Gate with a spike: load YuNet + YOLOX-nano in tract and compare outputs against OpenCV/ORT [unverified].
  Models downloaded on first use or bundled (<10 MB total), SHA-256 pinned, licence notice in NOTICE file.

## 3. Pipeline

1. Analysis decode (ffmpeg subprocess via `exec::process`):
   `rmedia-ffmpeg -i file:IN -vf "fps=3,scale=320:-2:flags=bilinear,format=rgb24" -f rawvideo -` (or gray for the histogram pass).
   Keep per-frame pts; fixed frame size => simple read loop. One decode pass can feed both cut detection and detectors.
2. Shot cuts: own luma-histogram (e.g. 32-bin) chi-square/L1 diff on the sampled frames, adaptive threshold
   (median + k*MAD) — easy in Rust, deterministic, tunable, and sampling at 3 fps is enough for hard cuts but
   timestamps are coarse (+-0.33 s). Option: run `scdet` (verified present locally [V], `scdet=threshold=10`, emits
   `lavfi.scd.time` metadata; parse via `-vf scdet,metadata=print:file=-`) at full fps for frame-exact cuts, then
   run detectors only on the sampled frames. Recommended: scdet at full rate (cheap on low-res: `scale=320:-2,scdet`),
   own histogram as fallback/test oracle.
3. Per-shot target selection: for each sample frame collect boxes: faces (score >= 0.6), persons; weight
   (face 1.0, person 0.6, other object 0.3, scaled by area). Subject set S = high-weight boxes. Per-frame desired
   center x = weighted mean of S (or the single largest/most persistent track). Vertical crop is full-height for
   16:9 -> 9:16, so only x is animated (y fixed). For 4:3 -> 9:16 same; for already-narrower sources animate y.
4. Motion mode per shot (AutoFlip-style): if the span of desired x over the shot fits inside the crop window with margin
   -> stationary at the median/centered window; else tracking.
5. Smoothing (x path), in order of complexity:
   a. v1: median filter (window ~1 s) -> dead-zone/hysteresis (move only if target leaves the central ~30% of the
      window, then ease toward it) -> critically-damped / exponential low-pass with max velocity clamp
      (e.g. <= 25% of crop width per second, tunable) -> piecewise-linear resample at 10 Hz. Reset state at shot cuts
      (cut = jump allowed, no pan across cuts).
   b. later: one-euro filter (min-cutoff 0.5-1 Hz, beta tuned) or constant-velocity Kalman; L1-optimal path via LP.
   Constraint: clamp 0 <= x <= iw - cw always; and a "keep required face inside crop" hard clamp after smoothing.
6. Render (the key decision) — three options, verified locally with ffmpeg 6.1.1:
   - A. `crop` with expression `x='min(max(0,<piecewise>),iw-ow)'` [V works]. Expressions of t get long
     (hundreds of segments) — limited by AVOption string/parse cost; fragile on very long clips.
   - B. `sendcmd` file with `crop x <value>;` entries [V: `sendcmd=f=c.cmd,crop=w=406:h=720:x=0:y=0,...` ran OK;
     `crop` x/y/w/h options flagged `T` (runtime-settable) in `ffmpeg -h filter=crop` [V]]. Command file format:
         0.000 crop x 312;
         0.100 crop x 318;
     Pass the file via `-filter_script`-free `sendcmd=f=<path>` (path must be escaped in filtergraph; write it to a
     temp file in the job dir; on Windows escape `:` and `\`). Granularity: 1 update per frame time at source fps
     (e.g. every 1/30 s) gives smooth motion; with 10 Hz updates motion visibly steps (crop is integer-pixel and holds
     the value until the next command) — so emit a command PER FRAME using the smoothed path interpolated at each
     frame's pts, rounded to even pixels. The crop `x` is evaluated per frame, so a per-frame value is exact.
     Caveat: sendcmd matches on frame pts (stream time base); with VFR input, `fps=` first or interpolate on actual pts
     [unverified]. A per-frame command file for a 1-h 30 fps video = 108k lines ~2.5 MB — fine.
   - C. Pipe frames through Rust and re-encode: avoids filter fragility but duplicates decode/encode; reject.
   **Recommendation: B**, emitted by core from the same smoothed path struct; deterministic and unit-testable
   (assert file contents). Use `zmq` only for live preview — not needed (needs ffmpeg build with libzmq [unverified on bundle]).
   Final chain (x animated, fixed crop size; compute cw = round(ih*9/16/2)*2):
     `sendcmd=f=path.cmd,crop=w=<cw>:h=ih:x=0:y=0,scale=1080:1920:flags=lanczos,setsar=1`
   Scale last (one resample); never upscale crops beyond source scale — if cw*sx would upscale more than ~1.5x, warn
   (hand off to feature 002's upscaler stage).

## 4. Fallbacks and decision rule
Computed per shot from the subject set and the crop window width cw:
- No subject found (< ~20% of sampled frames have any box) -> **center crop** if source is only mildly wider
  (e.g. 4:3 / 1:1: loses <= ~40% width) else **blurred-pad fit** (below). Salience-free default for screen recordings,
  landscapes, slides.
- Union span of required subjects <= 0.9*cw -> stationary or tracking crop.
- Span > cw (e.g. two people far apart) -> **blur-pad fit** (whole frame visible, centered), or **split** (v2).
- Large text/graphics/letterboxed source -> run `cropdetect` first to strip black bars, then decide [V cropdetect present].
- Thresholds are tunables; store the chosen mode + reason per shot in the analysis JSON (debuggable).

Exact blur-pad filtergraph (tested OK on local ffmpeg [V]); W/H = 1080x1920:
```
[0:v]split=2[a][b];
[a]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,gblur=sigma=30,eq=brightness=-0.1[bg];
[b]scale=1080:-2[fg];
[bg][fg]overlay=(W-w)/2:(H-h)/2,setsar=1
```
(Cheaper blur: `scale=108:192` -> `gblur` -> `scale=1080:1920` before overlay; ~10x faster, looks the same. [K])
Split-screen (defer): `[0:v]crop=cw:ih/2... ` crop two subject windows, `vstack` — needs 2 tracked paths; `vstack` present [V].
Shots are rendered as segments (trim + per-shot filter) then concatenated, or one graph with per-shot `enable`/`overlay`
switching; simplest: one pass with `sendcmd` also toggling modes is hard — v1 uses **per-shot segments** encoded with
identical params then concat (re-encode; no `-c copy`). Alternative: choose one mode per whole video in v1 [recommended].

## 5. Evaluation with synthetic fixtures (rmedia-testkit)
- Fixtures [lavfi, no binaries committed]:
  - `testsrc2` plus moving subject: `color=c=black:s=1280x720,drawbox` can't move, so use
    `overlay` of `smptebars/color` square with `x='t*100'`: `-f lavfi -i color=c=gray:s=1280x720:r=30 ... overlay=x='(W-w)*abs(sin(t))':y=300` [unverified exact expression].
  - Static face-like object: no real faces in lavfi — for the detector-integration tests use a tiny checked-in-by-hash
    downloaded sample? Better: unit-test the planner with injected `Detector` (fake boxes) and keep the real detector
    as an `#[ignore]`d smoke test using a CC0 image downloaded in CI [S].
  - Two-shot clip: concat of two lavfi sources with subject on opposite sides (tests cut handling).
  - Two far-apart subjects (tests fallback to blur-pad).
- Ground truth: the subject's known path (x(t)) from the generator expression; compare with the final crop window path
  produced by the planner (path-level test, no encode needed) and optionally by re-detecting a marker colour in the output.
- Metrics / pass thresholds (starting values; tune):
  - Subject-in-crop >= 95% of frames (>= 99% for the stationary case), subject center within crop at margin >= 10% cw.
  - Max |velocity| <= vmax (config, default 0.25*cw/s); max |acceleration| and |jerk| bounded: p99 |d3x/dt3| <= J (set
    from the golden run, then guard against regressions).
  - No motion in dead zone: subject static -> crop path variance 0 (std < 0.5 px).
  - Cuts: cross-cut path discontinuity is allowed ONLY at detected cut times; cut detection recall = 100% on synthetic
    cuts (tolerance +-1 sample interval), precision >= 1 false cut / 10 min.
  - Fallback test: two subjects span > cw -> mode == blur-pad; no subject -> center crop or blur-pad per rule.
  - Output geometry: exactly 1080x1920 (or target), SAR 1, even dims, duration within 1 frame of source, audio copied/untouched.
- Render test: parse the generated `.cmd` (values within [0, iw-cw], monotone timestamps, one per frame) and ffprobe the output.
- Golden files: store planner JSON for fixtures (text, small) not video.

## 6. Recommendation for v1 / deferred
V1 (genuinely smart, small):
1. One decode pass at 320 px / 3 fps -> luma-histogram cuts (+ optional full-rate scdet) -> YuNet faces (+ YOLOX-nano
   persons) via `tract` behind a `Detector` trait; models SHA-pinned, <10 MB.
2. Per-shot stationary-vs-track decision; x-only path: median -> dead-zone -> low-pass with velocity clamp; reset at cuts.
3. Render with `sendcmd` per-frame `crop x` + scale. Fallback: shot-level blur-pad when no subject / span > cw;
   whole-video center crop when detectors disabled (`--no-detect`, the zero-model mode).
4. Single "motion" knob (calm/normal/fast) -> smoothing + vmax. Emit analysis JSON (shots, boxes, modes, path) for
   debugging and a future GUI preview/edit (draggable crop keyframes).
Defer: saliency model (U2-Net) for no-face content; split-screen; L1-optimal/Kalman paths; per-shot mixed modes in one
render; y/zoom animation (variable crop size); speaker/active-speaker detection; GPU execution providers; zmq live preview.

## Open items [unverified]
- Re-run sendcmd/crop/blur-pad tests on bundled FFmpeg 9; VFR behaviour of sendcmd.
- tract operator support + speed for YuNet / YOLOX-nano / NanoDet; ort rc stability.
- Per-model weight licences (YuNet opencv_zoo LICENSE, YOLOX weights, MP person det).
- scdet metadata parsing format; thresholds for histogram cuts on real footage.
- Exact synthetic-moving-subject lavfi expression in testkit.

## Measured on the bundled engine (FFmpeg n9.0.2, 2026-10-06, Intel Raptor Lake iGPU, 12 threads)
- [V] `sendcmd=f=cmds.txt,crop=w=608:h=1080:x=0:y=0,scale=1080:1920:flags=lanczos,setsar=1` with
  `<t> crop x <px>;` lines moves the crop at the commanded times (checked on extracted frames).
- [V] Blur-pad graph (`split → scale increase + crop + gblur=sigma=30 + eq → overlay` of the fitted
  foreground) renders 1080x1920 correctly.
- [V] `scdet=t=10,metadata=print:file=-` prints `lavfi.scd.score` / `lavfi.scd.mafd` per frame
  (parseable from stdout).
- Still open: `sendcmd` with variable-frame-rate sources (commands are time-keyed, so VFR should be
  fine; confirm with a VFR fixture).
