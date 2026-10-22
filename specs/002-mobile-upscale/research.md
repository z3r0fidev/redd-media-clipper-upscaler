# Research & Decisions: Mobile Upscaling & Smart Reframe

Sources: `research/02` (engines, plus "Measured"), `06` (device targets), `07` (upscalers), `08`
(smart reframe, plus "Measured"), `09` (open items settled).

Tags: [V] = verified on the bundled engine or read from a primary source, [D] = design decision.

## R1. Presets as data

- **Decision:** presets are JSON files that follow
  [contracts/preset.schema.json](./contracts/preset.schema.json).
  - Built-in presets are compiled in with `include_str!`.
  - Users can add or override presets by `id` in `<config>/presets/*.json`.
  - Each preset carries:
    - `schema` and `version`;
    - size, `max_fps`, codec and profile/level, CRF;
    - audio settings;
    - `hdr_capable`;
    - the device classes it targets;
    - optional `limits` (`max_bytes`, `max_duration`);
    - an `est_bpp` used for the size estimate.
- **Shipped set** (research/06 §2, research/09 §5–6):

  | id | W×H | aspect H/W | video | notes |
  |---|---|---|---|---|
  | `phone-fill` (default) | 1080×2340 | 2.167 | HEVC Main, `hvc1`, CRF 22 | iPhone 15–18, Galaxy S22–S26 exact; Pixel bars ≈ 1.3% |
  | `phone-fill-hd` | 1440×3120 | 2.167 | HEVC Main, CRF 22 | high-detail sources only |
  | `pixel-tall` | 1080×2400 | 2.222 | HEVC Main, CRF 22 | Pixel / tall Android |
  | `reddit-916` | 1080×1920 | 1.778 | H.264 High, CRF 18 | `max_fps` 60; 1 GB / 15 min limits |
  | `reddit-feed-45` | 1080×1350 | 1.25 | H.264 High, CRF 18 | feed crop size (research/09 §5) |
  | `fold-inner` | 1080×1200 | 1.111 | HEVC Main, CRF 22 | near-square class (Fold inner 1.04–1.16) |
  | `duo` | 1080×1566 | 1.45 | HEVC Main, CRF 22 | Duo outer 1.455; inner 1.42 is close |

- **Rationale:**
  - The constitution requires presets to be data.
  - One preset per aspect class covers the unverified device rows (research/09 §6).
  - `fold-inner` moved off 1080×1350 so it no longer duplicates the Reddit 4:5 preset.
- **Alternatives:**
  - An `ios-pro` 1206×2622 preset was dropped: a 1080-wide output is not under-resolved on Pro
    screens (research/06 §1), and users can add it as their own preset.
  - TOML/YAML was rejected because 001 already standardised on JSON.

## R2. Encode and colour

- **HEVC (phone presets):**
  ```
  libx265 -preset fast -crf <preset> -pix_fmt yuv420p -profile:v main -tag:v hvc1
    -x265-params colorprim=bt709:transfer=bt709:colormatrix=bt709:range=limited:log-level=error
  ```
  - `hvc1` is required by Apple Photos and QuickTime (research/06 §3).
  - At 1080×1920, `-preset fast` ran about 1.7× realtime on the reference laptop (research/02
    "Measured"). That fits SC-003 with margin.
- **H.264 (Reddit presets):**
  - `libx264 -preset medium -crf 18 -profile:v high -pix_fmt yuv420p`, with `+faststart`.
  - Level: 4.2 for 1080×1920 at 60 fps or below. Taller sizes are computed from the macroblock rate
    (research/06 §3): 5.0 at 30 fps, 5.1 at 60 fps.
- **Colour:**
  - zscale reads the input matrix from the probe. Untagged sources are treated as BT.601 when the
    height is 576 or less, otherwise BT.709.
  - Output is always BT.709, limited range, with explicit
    `-colorspace/-color_primaries/-color_trc/-color_range` (FR-004).
- **Audio:**
  - Copy when the codec is AAC, otherwise AAC at 192k.
  - Text subtitles become `mov_text`; image subtitles are dropped with a warning.
  - Chapters and metadata are kept (FR-020). This reuses 001's `streams::select_streams` with the
    mp4 container.
- **Platform limits:** when `estimate.bytes > limits.max_bytes`, the plan warns, and the encode gets
  `-maxrate/-bufsize` sized to fit (FR-002). When the duration is over `max_duration`, the plan
  warns and still renders.

## R3. Pre-processing order and the no-enlarge rule

- **Chain,** applied at source resolution:
  1. `bwdif` when `field_order` is interlaced.
  2. `scale=iw*sar:ih,setsar=1` when the SAR is not 1:1.
  3. Tone-map when the source is HDR and `hdr=sdr` (R4).
  4. `hqdn3d=1.5:1.5:6:6`.
  The CPU scaler is `zscale` Lanczos with explicit input and output colour properties, then
  `cas=0.7` when enlarging (see the measurements below).
  5. `fps=<max_fps>` only when the source is faster than that. Otherwise
     `-fps_mode passthrough`, which keeps VFR timing.
- **Rotation:** FFmpeg autorotates by default. The probe reads the display-matrix rotation so that
  geometry and analysis use the displayed size.
- **No enlargement (FR-005):**
  - When the crop window is already at or above the target size, the scaler is a plain downscale and
    `cas` is dropped.
  - The plan sets `no_upscale_needed: true` and tells the user.
- **Tiny sources (240p or below):** warn, and suggest AI if it is available.
- **Scaler and sharpening (measured 2026-10-14, VMAF against the clean 1080x1920 original, x264
  CRF 18 output, `DegradedPortrait` 480p fixture):**

  | Chain | VMAF | vs bicubic (54.85) |
  |---|---|---|
  | `hqdn3d` + spline36 + `cas=0.5` (first plan) | 56.54 | +1.7 |
  | `hqdn3d` + lanczos + `cas=0.7` (**chosen**) | 57.63 | +2.8 (+2.9 at 360p) |
  | `hqdn3d` + spline36 + `cas=0.8` | 57.90 | +3.1 |
  | `hqdn3d` + spline36 + `cas=1.0` | 64.31 | +9.5, with visible ringing on flat areas |

  VMAF rewards over-sharpening, so the chain stays inside the 0.3–0.7 `cas` range and SC-002's
  automated CPU bar is +1.5 on the synthetic fixtures (owner decision). `hqdn3d` stays: it costs
  little VMAF on clean synthetic content and is the artefact reduction FR-006 asks for on real
  footage.
- **Alternatives:** `nlmeans` was rejected as a default because it ran at 5.6 fps (research/02
  "Measured"). It may become an opt-in later.

## R4. HDR

- **Default `hdr=sdr`.** HDR sources (transfer `smpte2084` or `arib-std-b67`) are tone-mapped:
  - CPU: `zscale=t=linear:npl=100,format=gbrpf32le,tonemap=hable:desat=0,zscale=t=709:m=709:p=709:r=tv`.
  - GPU tier: libplacebo `tonemapping=bt.2390`.
- **`hdr=keep`** is allowed only for presets with `hdr_capable: true`: `phone-fill` and
  `phone-fill-hd`.
  - The output is HEVC Main10 `yuv420p10le` with the source primaries, transfer and matrix tags.
  - The Dolby Vision RPU is dropped, and mastering metadata is not carried in v1.
  - Using `keep` with a non-capable preset is a validation error (exit 3).
- **SDR sources are never converted to HDR.**

## R5. Filter graph: one pass, mixed reframe [V]

- The graph is built by `upscale::graph` as a pure function and covered by insta snapshots.
- Smart reframe with fallback ranges:
  ```
  [0:v]<trim?>,<pre>,split[a][b];
  [a]sendcmd=f=reframe.cmd,crop@trk=w=CW:h=CH:x=X0:y=Y0,<SCALER→W:H>,cas=0.7[crop];
  [b]split[bg0][fg0];
  [bg0]scale=W/10:H/10:force_original_aspect_ratio=increase,crop=W/10:H/10,gblur=sigma=3,
       eq=brightness=-0.08,scale=W:H[bg];
  [fg0]<SCALER fit W:-2 or -2:H>[fg];
  [bg][fg]overlay=(W-w)/2:(H-h)/2[pad];
  [crop][pad]overlay@fb=0:0:enable=0,setsar=1,format=yuv420p[v]
  ```
- **Verified on n9.0.2 (2026-10-07):** `reframe.cmd` lines of the form `<t> [enter] overlay@fb enable 1;`
  toggle the overlay at the commanded times. With a red main input, a blue pad and enable from 2 s
  to 4 s, frames at 1 s, 3 s and 5 s sampled red, blue and red. `crop@trk x <px>` commands were
  already verified (research/08 "Measured").
- The fallback branch is left out when there are no fallback ranges.
  - **Center crop** is the same graph with a fixed `x` and no `sendcmd`.
  - **Blur** for the whole video uses only `[pad]`.
- **Command file:** ffmpeg is spawned with the working directory set to the job scratch directory,
  and `f=reframe.cmd` is a bare relative name. This avoids escaping Windows drive and UNC paths
  inside a filtergraph (Principle IV).
- **Timing:**
  - When a clip has a trim, the trim is the first filter (`trim,setpts=PTS-STARTPTS`), so command
    times are relative to the clip.
  - Commands are keyed to frame time, so VFR sources need no special handling. A Phase 0 test with a
    VFR fixture confirms this.
- **Alternatives:**
  - `streamselect` with `map` commands (kept as a fallback should `enable` regress).
  - Segment + concat (rejected: extra encodes and audio-sync risk).
  - `crop` with a per-frame expression using `if(between(t,..))` (rejected: thousands of terms).

## R6. Smart reframe analysis

- **Decode:** one ffmpeg process per job.
  - `filter_complex` does `[0:v]<pre>,scale=576:-2 (320 tall for portrait),split[s][d]`.
  - `[s]scdet=t=10,metadata=print:file=cuts.txt` writes the cut scores.
  - `[d]fps=3,format=bgr24` goes to `-f rawvideo pipe:1`.
  - Both scdet and the rawvideo pipe were verified in research/08 "Measured".
- **Cuts:**
  - A cut is a frame with `lavfi.scd.score ≥ 10`.
  - A Rust histogram-difference check on the 3 fps frames catches cuts that land between samples.
  - Shots shorter than 1 s are kept as shots (spec edge case).
- **Detection:**
  - `Detector` trait. The production implementation is YuNet 2026may under `tract-onnx`, with the
    input built at runtime to the analysis size. Measured at 68 ms per frame (research/09 §4).
  - Score threshold 0.6, NMS IoU 0.3.
  - Frames are spread across a rayon-free pool of `std::thread::scope` workers, `min(cpus, 4)`.
  - Results are reassembled in frame order.
- **Subject selection, per sample:**
  - Take the largest face. Faces under 50% of its height are background and are ignored.
  - Every other face is a subject. The sample is "no fit" when any subject is more than 1.5 crop
    widths from the largest face, or when the subjects' union is wider than 0.9 × crop width.
    Otherwise the crop follows the centre of the union.
  - (Implementation decision, 2026-10-16: with "add faces within 1.5 crop widths", two equal faces
    far apart would track one of them and never fall back, contradicting FR-014.)
- **Fallback ranges (FR-014):**
  - Any run of "no face" or "no fit" samples lasting at least 1.0 s becomes a fallback range,
    clipped to shot bounds.
  - A shot with no face anywhere is a fallback for its whole length.
  - Gaps shorter than 1 s hold the last position.
- **Path, per shot:**
  1. Median filter over 5 samples on the subject centre x.
  2. Deadzone: the target moves only when the centre leaves ±6% of the crop width around the
     current target.
  3. Interpolate to the output frame grid (source fps capped at 60).
  4. Clamp velocity and acceleration with the motion profile:

     | profile | max speed (crop widths/s) | max accel (crop widths/s²) |
     |---|---|---|
     | `calm` | 0.35 | 0.7 |
     | `normal` (default) | 0.7 | 1.5 |
     | `fast` | 1.4 | 3.0 |

  5. Clamp to the frame, snap to even pixels, and reset at each cut.

  Following uses the fastest speed that can still stop on the target
  (`v = −a/2 + √(a²/4 + 2a|d|)`), then the acceleration limit, so the crop never overshoots.
- **Analysis decode:** long side 576 px (padded to multiples of 32 in Rust), 3 fps, no denoise or
  fps cap (they do not move anything). The scdet scores go to `cuts.txt` in a private working
  directory under `<cache>/reframe/tmp/`; a 48-bin histogram jump (> 0.5) between samples adds cuts
  scdet missed. On MovingMarker the hard cut scores 12.5, just above the threshold of 10.
- **sendcmd file (R5):** commands at frame midpoints `(k − 0.5)/fps`, crop lines only where x
  changes, toggles `[enter] overlay@fb enable 1|0` at the first frame of each fallback range and
  after it. Verified on n9.0.2 through the full graph: the background shows on exactly the
  commanded frames (2026-10-16). When fallback covers the whole clip, the render uses the plain
  blur graph.
- **SC-004 check:** the configured limit is the bound. Tests assert that no frame-to-frame step
  exceeds it except at cuts.
- **Determinism:** fixed thresholds and in-order reassembly give the same path for the same input on
  the same machine.
- **Missing model:** smart reframe becomes blur-pad, with the warning "face model not found;
  using blurred background".
- **Alternatives:**
  - Saliency, person detection, vertical motion and zoom are deferred (research/08, spec
    Assumptions).
  - `ort` was rejected (native library).
  - A Kalman filter was rejected: median + deadzone + clamp meets SC-004, and its parameters are
    easier to explain.

## R7. Testing smart reframe without face images

- `MarkerDetector` (test-only) implements `Detector` by finding pure-magenta pixels.
- **testkit fixtures:**
  - `moving_marker`: a 1280×720 gray `color` source with an 80×80 magenta `color` overlay whose `x`
    is an expression of `t`, plus a hard cut made with `concat` to a second shot.
  - `far_markers`: two markers 1000 px apart, which must produce a fallback.
- **End-to-end check:** render the output, run `MarkerDetector` on the output frames, and assert
  SC-004 (≥ 95% inside, never outside for more than 0.5 s, steps ≤ limit except at cuts).
- **Golden files:** golden `reframe.cmd` text for path snapshots, and table tests for the median,
  deadzone and clamp steps.
- **Real YuNet:** one `#[ignore]` test runs real YuNet on a public-domain NASA portrait fetched by
  `xtask fetch-models`, turned into a 3 s panning video with lavfi `zoompan`.

## R8. Capability detection [V]

- **Listing:**
  ```
  rmedia-ffmpeg -v verbose -init_hw_device vulkan=vk:N -f lavfi -i color=s=64x64:d=0.1
    -vf hwupload,libplacebo=w=128:h=128:format=yuv420p,hwdownload,format=yuv420p -f null -
  ```
  The "GPU listing" block was verified on this machine:
  ```
  0: Intel(R) Graphics (RPL-U) (integrated) (0xa721)
  1: llvmpipe (LLVM 20.1.2, 256 bits) (software) (0x0)
  ```
  - Parse each line as `index: name (kind) (0xdeviceid)`, where kind is integrated, discrete,
    virtual, cpu or software.
  - Reject `software`/`cpu` devices and names matching llvmpipe, lavapipe or SwiftShader.
  - Prefer discrete, then integrated.
  - A device is usable only when the libplacebo round trip on that index exits 0.
  - Mesa `FINISHME` lines are ignored.
- **Fingerprint:** SHA-256 over:
  - the app version plus a check revision (bumped when the self-tests or tier graphs change),
    the ffmpeg version string, the listing lines and the AI model SHAs (YuNet is left out, so
    fetching the face model does not re-test);
  - the `VK_DRIVER_FILES`, `VK_ICD_FILENAMES` and `VK_ADD_DRIVER_FILES` values;
  - the `rmedia-sr` version (US6);
  - the Vulkan driver identity, because the listing has no driver version. On Linux, the
    `icd.d/*.json` files under `/usr/share/vulkan` and `/etc/vulkan`, plus the size and mtime of
    each `library_path`. On Windows, the value names under `HKLM\SOFTWARE\Khronos\Vulkan\Drivers`
    plus the size and mtime of each file.
- **Record:** `<state>/capability.json`
  ([contracts/capability-record.schema.json](./contracts/capability-record.schema.json)).
  - Written to a `create_new` temp file, then renamed, while holding std `File::lock()` on a
    `capability.lock` sidecar (as `exec/journal.rs` does; `fs4` is used only for free space).
  - Loaded with a size cap and validated (schema 1, hex fingerprint, bounded lists and strings);
    anything else counts as no record.
  - Contents: devices, tiers (`cpu`, `gpu`, `ai`, each with `available`, `reason` and measured
    throughput), and `demoted[]`.
- **When it is checked:**
  - The cheap listing runs once per process (about 0.3 s) to compute the fingerprint.
  - The self-test runs only when the fingerprint changes, or on `doctor --recheck` / the Settings
    "Check again" button (FR-009).
  - This reads US3 scenario 5's "do not repeat the check" as the self-test, not the listing.
- **Self-test, both run at the default preset size:** 60 frames of lavfi `testsrc2` at 480×854
  through the tier's graph and the preset's x265 encode to `-f null` (about 1.5–2 s each), so the
  CPU and GPU figures compare whole jobs. The measured fps feeds the time estimate (FR-021),
  scaled by output pixel count. Every probe has a 60 s timeout; a check never fails, problems
  become `reason` strings.
- **Measured on the reference laptop (2026-10-19):** CPU 33–36 fps, Intel RPL-U iGPU 21–29 fps;
  first check 4.2 s, cached 0.13 s. The x265 encode dominates both tiers, and on an iGPU the
  upload and download cost more than the scaling saves.

## R9. Tier selection and fallback

- **`auto`:**
  - Choose `gpu` when the record says it is available, it has not been demoted, and its self-test
    is at least as fast as the CPU's (SC-003). A slower GPU gets `cpu` with the reason
    `… measured slower than the CPU here (21 vs 35 fps)`; it can still be forced.
  - Otherwise choose `cpu`, with a `tier_reason`.
  - `auto` never selects `ai` on its own. When AI is available and the source is under 720p, the
    plan sets `suggest: "ai"` and the CLI and desktop show it. This is the reading of FR-011 and
    US6 scenario 1: AI is offered, then used if chosen.
- **Forced tiers:**
  - A forced tier that is not available is a validation error before anything runs (exit 3, code
    `TIER_UNAVAILABLE`).
  - A forced tier never falls back (FR-008).
- **One fallback:** an automatic `gpu` job that fails with `ENGINE_FAILED` is retried once on `cpu`.
  - Not on disk full, cancel or verify errors.
  - The job emits a `job_warning` (with ffmpeg's message), and its outcome records
    `tier_fallback`. Progress continues from where the bar is (never backwards).
  - Only when the CPU retry succeeds (so the source is not to blame) does the record get
    `demoted: {tier: gpu, error, at}`, with a fixed `error` text (ffmpeg's text can name the
    user's files). Later `auto` runs pick CPU until the fingerprint changes or a recheck (SC-008).
- **Checks are bounded and never stored half-done (review 2026-10-19):** probes run through
  `exec::process` (process-tree kill), each within 60 s and the whole check within 180 s, and
  honour Ctrl-C. A check cut short is used for that run only, never stored. `upscale --tier cpu`
  skips the check. The desktop loads a matching stored record at start (listing only) and runs
  the self-tests only on **Check again**.
- **Known limit:** on Windows the driver fingerprint reads only
  `HKLM\SOFTWARE\Khronos\Vulkan\Drivers`; drivers registered under the display-adapter key are
  missed, so after a Windows driver update run `rmedia doctor --recheck`.
- **Disappearing GPU:** a GPU that vanished since the check (dock, driver) shows up as a failure of
  the job's `-init_hw_device`. It is handled by the same fallback (auto) or a clear error (forced).
- **GPU scaler (measured 2026-10-19, 480p fixture to 1080×1920, VMAF):**
  `libplacebo=…:upscaler=ewa_lanczossharp:sigmoid=0`, then `hwdownload` and the CPU tier's
  `cas=0.7` when enlarging. Results: 57.66, against CPU 57.63 and bicubic 54.85. The libplacebo
  defaults (sigmoid upscaling, no sharpening) scored 53.86, under bicubic; with `cas=0.7` and
  sigmoid on, 56.0. The ignored test `us3_gpu::gpu_is_not_slower_and_not_worse_than_cpu` keeps
  GPU ≥ CPU − 0.5. HDR→SDR uses `tonemapping=bt.2390:peak_detect=0` (deterministic across cuts);
  the blurred background stays on the CPU.
- **GPU device pinning:** the GPU tier always uses `-init_hw_device vulkan=vk:<index>` +
  `hwupload` + libplacebo + `hwdownload`, so libplacebo cannot silently pick llvmpipe
  (research/07).
- **Scheduling:** GPU and AI jobs also take a `Resource::Gpu` permit (1 slot).

## R10. Where analysis runs; caching

- Planning stays fast and never runs detection.
- **Dry run:** `--dry-run` on smart jobs runs the analysis by default so the crop path can be shown
  (FR-016). `--no-analyze` skips it.
- **Execution:** if no cached analysis exists, the job runs it as stage 1, with weighted progress
  (analysis 0.25, render 0.7, verify 0.05).
- **Measured (release build, 2026-10-16):** analysing 60 s of 1080p takes 10.7 s against a 79 s
  centre-crop render (13.5%; SC-005 allows 50%). Clips that mix tracked and fallback sections
  render both the crop and the background branch, which costs more than centre or blur alone.
- **Cache:** the key is the SHA-256 of:
  - canonical path, size and mtime;
  - clip span;
  - pre-process parameters;
  - analysis size;
  - motion profile;
  - algorithm version;
  - model SHA (in the detector id);
  - crop size, output fps and the ffmpeg version (added in implementation: the path depends on
    them).
- Cache entries are JSON in `<cache>/reframe/`, with LRU eviction by count (500).
- The cache stores analysis only and never media. The desktop `analyze_reframe` command uses the
  same cache, so the overlay and the render agree.

## R11. Determinism

- Same input, options, tier and machine → the same output bytes. FFmpeg and x265 are deterministic
  with fixed threads, so the CPU tier pins `-threads` and `-x265-params pools=` to the plan's
  value.
- GPU and AI output may differ between devices. This is documented in `cli.md`.
- Reframe analysis is deterministic (R6).

## R12. Estimates and disk space (FR-021)

- **Time estimate:** `(media_secs × fps_src) / measured_fps(tier, preset)`. For smart reframe, add
  the analysis time, `samples × 68 ms / workers`. The CPU and GPU fps come from the capability
  record.
- **Size estimate:** `est_bpp × W × H × fps_out × duration / 8`, plus the audio bitrate × duration.
- **Free space:** checked per output directory with `fs4` against the sum of estimates × 1.2. When
  it is short, the plan reports a warning (not an error), because the estimates are rough.

## R13. AI tier worker (US6)

- **Worker:** `rmedia-sr`, a Rust binary.
  - Builds against the ncnn 20260526 prebuilt static libraries (Linux ubuntu-2204, Windows
    vs2022, Vulkan on), fetched and SHA-pinned by `xtask fetch-ncnn`.
  - Uses an FFI over `c_api.h`.
- **Protocol:** [contracts/rmedia-sr-protocol.md](./contracts/rmedia-sr-protocol.md).
  - Input on stdin: `rgb24` frames of W×H.
  - Output on stdout: frames of W·s × H·s.
  - stderr: JSON lines with progress, device and errors.
  - `--gpu <index>` is required. The worker rejects software devices itself.
  - `--selftest` times N frames and prints the result.
- **Pipeline:**
  1. ffmpeg #1 decodes, trims and pre-processes, applies the reframe crop at source scale (with
     blur-pad composed at source scale; a documented softness trade-off), and writes rawvideo.
  2. `rmedia-sr` runs ×s.
  3. ffmpeg #2 reads rawvideo with the source frame timing, runs `scale` to W×H, a light
     anti-flicker step, encodes, and muxes the audio from the source by a second input. The
     anti-flicker method is chosen in US6 by measurement (static-region frame difference); the
     leading candidate is a motion-masked temporal blend (`tmix` where a `tblend` difference is low).
  - All three processes share one process group / Job Object, so cancel kills them all
    (Principle IV).
- **Gate:**
  - The self-test predicts the job time. AI is `available` only when the prediction for a 60 s 480p
    reference is 10 minutes or less (SC-009 bound).
  - When AI is forced but the prediction is more than 10× the clip length, the plan warns with the
    estimate: "too slow: estimated 45 min for this clip".
- **Models:** `realesr-animevideov3-x2/x3/x4` (BSD-3), plus optional `x4plus`.
  - SHA-pinned and fetched by `xtask fetch-models`.
  - Licences go in `licenses/models/`.
- **Alternatives:** the same as in plan.md Complexity Tracking.

### R13 measurements (US6, 2026-10-20, Intel RPL-U iGPU)

- **ncnn options:** Winograd convolution off is ≈ 2× faster on this iGPU (1.9 → 0.95 s per
  480x270 frame ×2); fp16 off or fp32 arithmetic is 2.5–3× slower. The worker turns Winograd off
  on integrated GPUs only; discrete GPUs keep ncnn's defaults (to confirm on owner hardware).
  The reference tool measured ≈ 0.64 s per frame at steady state for the same size.
- **Model cost:** the x2/x3 graphs run the x4 network and downscale, so cost per input
  megapixel does not depend on the scale; the gate uses seconds per input megapixel.
- **Gate:** 4 frames of 480x270 after a warm-up; available when 1800 × 0.41 Mpx × s/Mpx ≤ 600 s.
  This iGPU: 8.6 s/Mpx (release) → ≈ 106 min → unavailable, with that estimate as the reason.
- **Quality (SC-002, degraded 360p → 1080x1920):** AI 51.84 VMAF, GPU tier 43.73 (+8.1).
- **Flicker (static noisy source, mean frame difference of luma):** source 2.474, GPU tier 1.900,
  AI 1.105. Candidates on the AI output: `hqdn3d=0:0:3:3` 0.715 (−35 %) at 51.77 VMAF on moving
  content (−0.07); 3-frame `tmix` 0.609 at 34.59 VMAF (ghosting); motion-masked `tmix` 0.830 but
  `tblend` drops the first frame. Chosen: temporal-only `hqdn3d=0:0:3:3`.
- **Pipeline layout:** the blurred background (blur reframe, smart fallback ranges) is made from
  the source in ffmpeg #2, so the worker only upscales the picture; inputs larger than half the
  output are shrunk first so the model does ×2 of what is needed. Fallback ranges are therefore
  classically scaled on the AI tier.
- **Models pinned:** only the six files with the lock file's SHA-256s are used; the worker checks
  the bytes it loads (ncnn's memory reader has no length bound, so only known-good bytes reach it).

### R13 measurements on a discrete GPU (2026-10-22, Quadro M4000, owner run)

- **Speed:** self-test 0.999 s/Mpx. 60 s of portrait 480x854 with the default preset took 600 s,
  predicted 605 s (0.8 % off; SC-009 needs ±30 %).
- **Quality and flicker:** SC-002 AI 50.40 vs GPU tier 42.23 VMAF (+8.17); flicker AI 0.746, GPU
  tier 1.906, source 2.506. Both match the iGPU numbers.
- **Gate reference (decision):** the gate keeps pricing the full 854x480 frame (0.41 Mpx), the
  worst case over the framing modes (blur fit keeps the whole frame), although the default preset
  crops a portrait 480p source to 394x854 (0.34 Mpx) and a landscape one further. The M4000 is
  therefore not offered AI (737 s predicted), although this job met SC-009. Owner decision
  2026-10-22: keep the worst case.

## R14. Model files and lookup

- Model files are never committed.
- `xtask fetch-models` downloads them from Git LFS media URLs and verifies SHA-256:
  - YuNet 2026may;
  - a public-domain NASA test portrait (test-only, not listed in the notices);
  - Real-ESRGAN (US6).
- Installers ship them under `models/` next to the executables.
- Lookup order: `settings.models_dir` → the bundled `models/` → `<cache>/models`.
- `doctor` lists each model and its status.
