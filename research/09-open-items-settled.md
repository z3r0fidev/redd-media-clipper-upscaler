# 09 - Feature 002 open items: how each was settled

Date: 2026-10-06. Machine: Intel Raptor Lake-P laptop (i7-1355U class, 12 threads), Intel UHD iGPU
(Mesa ANV Vulkan), no discrete GPU. Engine: bundled FFmpeg n9.0.2.

Tags: [V] verified this session (measured here, or read from a primary source), [D] settled by a design
decision, [O] still needs the owner or real hardware (each [O] becomes a validation task in 002).

## Summary

| # | Open item | Outcome | How |
|---|---|---|---|
| 1 | Is a bundled AI worker feasible? | Yes: ncnn + Real-ESRGAN family | [V] ncnn is actively maintained, with a release on 2026-05-26 and prebuilt Linux/Windows libraries (Vulkan builds included). The reference tool runs here on the iGPU |
| 2 | AI speed on integrated graphics | Too slow; the self-test must gate it | [V] animevideov3 ×2: **1.46 s/frame** (0.7 fps) for 480×854 input. x4plus: ≈ 45 s/frame. A 60 s clip would take ≈ 44 min |
| 3 | Licences of what we would bundle | All permissive | [V] see §3 |
| 4 | Face detector usable from Rust without native deps | Yes: YuNet 2026may under `tract` | [V] 68 ms/frame at 576×320, single-threaded. 3 fps sampling of 60 s = 180 frames ≈ 12 s |
| 5 | Objective quality metric for SC-002 | VMAF (libvmaf), plus SSIM/XPSNR as cross-checks | [V] `libvmaf`, `ssim`, `psnr` and `xpsnr` are in the bundled build |
| 6 | Reddit upload limits | Conservative preset values kept as data, plus one manual upload test | [D]/[O] see §5 |
| 7 | Gallery app fit vs fill per phone | Phone-fill presets match screen aspect, so fit and fill look the same | [D]/[O] see §6 |
| 8 | Unverified device panels | Presets cover aspect classes, not individual models | [D] see §6 |
| 9 | FFmpeg 9 behaviour of the 08 pipeline | Works | [V] `sendcmd`+`crop`, blur-pad and `scdet` measured on n9.0.2 (research/08 "Measured") |

## 1-2. AI tier: engine and measured speed

- **Engine choice [V]:** use our own small worker built on **ncnn** (BSD-3) with the Vulkan backend. It
  is cross-vendor on Linux and Windows, and the same Vulkan stack is already used by the GPU tier.
- **Release status [V]:** ncnn tag `20260526` ships prebuilt packages:
  `ncnn-20260526-ubuntu-2204/2404(.zip|-shared.zip)` and `windows-vs20xx`.
- **Why not the reference tool [V]:** `realesrgan-ncnn-vulkan` is frozen at v0.2.0 (2022-04-24) and
  only reads image files or folders. It is used here as a reference only.
- **Worker shape:**
  - The worker reads rawvideo frames on stdin and writes upscaled frames on stdout, sitting between
    two FFmpeg processes.
  - It keeps tiles in GPU memory, uses fp16 where supported, and reports JSON progress on stderr.
- **Reference-tool quirk [V]:** on a machine with both a real GPU and llvmpipe, the tool exits with
  code 2 and writes nothing unless `-g 0` is given. Our worker must choose the GPU explicitly and
  reject software devices (llvmpipe, lavapipe, SwiftShader).
- **Measured on this iGPU [V]** (60 frames of 480×854 decoded to PNG):

  | Model (scale) | Time | Per frame |
  |---|---|---|
  | realesr-animevideov3 (×2) | 87.6 s | 1.46 s |
  | realesrgan-x4plus (×4) | 15 frames in ≈ 11 min (run stopped) | ≈ 45 s |

  Classical tiers on the same input run at 69 fps (GPU) and 100 fps (CPU). The AI tier is therefore
  for discrete GPUs only, and the self-test (FR-010) will reject this iGPU for anything but very
  short clips.
- **Still open [O]:** throughput on a discrete GPU (SC-009) needs owner hardware or a GPU CI runner.
- **Implication for the plan:**
  - The self-test must time a few representative frames and refuse when the predicted duration is
    more than N× the clip length (N set in planning).
  - The model set is animevideov3 ×2/×3/×4 (small and fast) plus an optional x4plus "max quality"
    mode.
  - `realesr-general-x4v3` has no official ncnn conversion. Converting it is optional work for the
    AI story, not a blocker.

## 3. Licences (read from the primary LICENSE files)

| Component | Licence | Source |
|---|---|---|
| ncnn (library and binaries) | BSD-3-Clause, plus third-party notices inside the source tree | `Tencent/ncnn/LICENSE.txt` |
| Real-ESRGAN code and released weights | BSD-3-Clause | `xinntao/Real-ESRGAN` (GitHub licence API) |
| Real-ESRGAN-ncnn-vulkan (reference tool, not shipped) | MIT | `xinntao/Real-ESRGAN-ncnn-vulkan/LICENSE` |
| YuNet face detector (all files in the model folder) | MIT | `opencv_zoo/models/face_detection_yunet/LICENSE` and its README |
| tract (Rust ONNX inference) | MIT OR Apache-2.0 | crates.io (research/08) |
| OpenCV Zoo repository | Apache-2.0; per-model licences apply | GitHub licence API |

- Models are fetched from **Git LFS media URLs** (the raw URL returns a pointer file). The fetch
  must verify the SHA-256, as `xtask fetch-tools` does for FFmpeg. YuNet 2023mar has SHA-256
  `8f2383e4…52fa4`.
- Excluded, as research/07 and 08 found: Upscayl and Video2X (AGPL), Ultralytics YOLO (AGPL), and
  non-commercial OpenModelDB models.

## 4. Face detection under tract [V]

| Model | Input | Result |
|---|---|---|
| `face_detection_yunet_2023mar.onnx` | fixed 1×3×640×640 | 165 ms/frame; cannot be resized (tract fails at Conv_0) |
| `face_detection_yunet_2026may.onnx` | dynamic H×W, run at 576×320 | **68 ms/frame**, load and optimise 79 ms |

- The model has 12 outputs: cls, obj, bbox and kps at strides 8, 16 and 32. Decoding follows
  `yunet.py` from the zoo.
- Timings are single-threaded. Analysing frames in parallel across cores scales close to linearly.
- Decision: use YuNet 2026may at an analysis width of 576 px (or 320 px tall for portrait sources)
  under `tract`. No ONNX Runtime or OpenCV dependency is needed for v1.

## 5. Reddit limits [D]/[O]

- **Primary source unavailable:** Reddit's help centre article ("Uploading and sharing videos on
  Reddit") returns 403 to automated clients, and its Help Center API returns 429.
- **Secondary sources agree on:**
  - 1 GB maximum file size;
  - 15 min maximum length;
  - MP4/MOV containers with H.264;
  - playback capped at 1080p.

  Ad specs add a 30 fps cap and list 1:1, 4:5, 4:3 and 16:9; 9:16 is listed only for premium
  "Takeover" ads. Sources:
  [capcut](https://capcut.com/resource/how-to-post-a-video-on-reddit),
  [dacast](https://www.dacast.com/blog/how-to-post-a-video-on-reddit/),
  [tryvizup](https://www.tryvizup.com/blog/reddit-ad-specs-2026-sizes-formats-and-creative-rules),
  [strikesocial](https://strikesocial.com/blog/reddit-ad-specs-complete-sizes-dimensions-and-safe-zones-guide/).
- **Decision [D]:** the Reddit preset is 1080×1920, H.264 High, 8-bit 4:2:0, AAC, MP4 with
  faststart, and at most 60 fps (the 30 fps cap applies to ads only).
- **Limits as preset data:** the preset carries `max_bytes = 1 GB` and `max_duration = 15 min`.
  The plan warns when an output is predicted to exceed them, and the bitrate is capped to fit when
  possible.
- **Feed preset:** add a **"Reddit feed 4:5"** preset (1080×1350). The organic feed may crop tall
  video to about 4:5; the full-screen player shows 9:16.
- **[O] Validation task:** the owner uploads one 9:16 and one 4:5 output to a test subreddit and
  records how the feed and the full-screen player present each. The presets are data, so the
  findings change defaults without code changes.

## 6. Phone fit/fill and device rows [D]/[O]

- Gallery apps default to aspect-fit (iOS Photos and Google Photos, research/06). When the output
  aspect equals the screen aspect, fit and fill look the same. The phone-fill preset (2.167) matches
  iPhones and the Galaxy S series exactly. On Pixel Pro XL (2.226) it leaves bars of about 1.3% at
  the top and bottom; an optional "Pixel tall" preset (1080×2400) covers that.
- Foldables and the iPhone Duo have their own aspect classes (inner around 1.0–1.42, Duo outer
  1.455). Each gets its own preset.
- Unverified panel rows don't block anything. A preset targets an aspect class, and every
  unverified device falls into one of the verified classes:
  - 2.16–2.17: Apple and Galaxy S;
  - 2.22–2.25: Pixel and tall Android;
  - ~1.42–1.46: Duo;
  - near-square: fold inner screens.
- **[O] Validation task:** a device test matrix on any two available phones (one iPhone, one
  Android). Play the default-preset output in the stock gallery app and check for no bars and
  correct colours.
