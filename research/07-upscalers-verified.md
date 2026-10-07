# 07 - Upscalers, verified (feature 002 input)

Date: 2026-10-06. Supersedes the [K] items of `02-upscaling-engines.md`.
Tags: [V] verified (URL or local test), [S] searched but weakly sourced / single source, [K] background knowledge, unverified.
Local tests ran on the dev box: FFmpeg 6.1.1 (Ubuntu), i7-1355U (12 threads), Intel RPL-U iGPU via Mesa ANV + llvmpipe. FFmpeg 8.x/9.x was NOT available locally, so all option names are 6.1.1 and must be re-checked against the pinned BtbN n9.0.2 (`ffmpeg -h filter=libplacebo`).

## 0. Headline findings
1. libplacebo accepts SOFTWARE frames directly and creates its own Vulkan device. No `-init_hw_device`, `hwupload` or `hwdownload` is needed [V local, 6.1.1; upstream patch https://ffmpeg.org/pipermail/ffmpeg-devel/2023-May/309589.html]. This is the simplest, most robust Tier 1 graph. It works with a SW encoder and with VAAPI (tested).
2. Vulkan works on this box: ANV (Intel iGPU) and llvmpipe are both enumerated; libplacebo `ewa_lanczossharp` 360p->1080x1920 ran at about 75 fps on the iGPU (hwupload/hwdownload graph) [V local].
3. realesrgan-ncnn-vulkan has no stdin/stdout streaming. It is file/dir based only. Its last release is v0.2.0, 2022-04-24, so it is effectively unmaintained [V]. Do not use the binary; use the ncnn library in own worker, or ORT.
4. `ort` crate is still 2.0.0-rc.13 (2026-07-28), wrapping ONNX Runtime 1.28; no stable 2.0 [V]. DirectML is in sustained engineering (Windows ML is the successor) [V].
5. AnimeJaNai, many OpenModelDB models: CC-BY-NC-SA-4.0. Never bundle [V].

## 1. FFmpeg libplacebo

### 1.1 Options (6.1.1 `-h filter=libplacebo`) [V local]; official docs: https://ffmpeg.org/ffmpeg-filters.html (section "libplacebo")
- Geometry/format: `w`, `h` (expressions allowed, default iw/ih), `format` (output pix_fmt), `force_original_aspect_ratio` (0..2), `normalize_sar`, `pad_crop_ratio`, `fillcolor`.
- Colour: `colorspace`, `color_primaries`, `color_trc`, `range` (output tags), plus `tonemapping` (auto, clip, st2094-40/10, bt.2390, bt.2446a, hable ...), `gamut_mode`, `peak_detect`, `hybrid_mix`, `brightness/contrast/saturation/hue/gamma`.
- Scaling: `upscaler` (DEFAULT `spline36`), `downscaler` (default `mitchell`), `frame_mixer`, `lut_entries`, `antiringing` (non-EWA only), `sigmoid` (default true).
- Debanding: `deband` (default false), `deband_iterations` (1), `deband_threshold` (4), `deband_radius` (16), `deband_grain` (6).
- Misc: `apply_filmgrain`, `apply_dolbyvision`, `custom_shader_path` (mpv .hook shaders, GLSL), `extra_opts` (raw libplacebo `key=val:` pairs).
- Names: `ewa_lanczossharp`, `ewa_lanczos`, `lanczos`, `spline36`, `mitchell`, `bicubic`, `nearest`, etc. come from libplacebo's filter-preset list; I could not list them from the FFmpeg doc (page truncated) [K for the full list; `ewa_lanczossharp` and `spline36` verified by running].
- Option names in 6.1.1 are `upscaler`/`downscaler`/`w`/`h`; the older `scaler`/`out_w` names I flagged in doc 02 are from the pre-6.0 design. Re-verify on n9.0.2 [verify].
- Filter is also timeline-capable for most options (`T` flag).

### 1.2 Vulkan device init
- Not required for software-in/software-out use (see section 0). libplacebo creates an internal device [V local].
- To pin a GPU (e.g. avoid llvmpipe, choose dGPU), use explicit init: `-init_hw_device vulkan=vk:0` (index from the "GPU listing" printed at `-v verbose`; here 0 = Intel, 1 = llvmpipe (software)) [V local]. Then `hwupload` uses it (`-filter_hw_device vk` if there are several devices).
- Docs example: `-init_hw_device vulkan -vf hwupload,libplacebo=format=yuv420p,hwdownload,format=yuv420p` [S: dev.to/ffmpeg ML posts]; works locally with my own variant.
- The first SW-only variant picks the device by libplacebo's internal choice; on hybrid laptops that could be llvmpipe or iGPU. For determinism use the explicit `-init_hw_device vulkan=vk:N` + `hwupload` form [K, reasoning from the GPU listing].
- Vendors: ANV (Intel Mesa) [V local]. RADV (AMD Mesa) and NVIDIA proprietary on Windows are [K]: libplacebo is regularly used with all three (Jellyfin ships it for tone mapping on all of them), but I did not test them. Windows Intel/AMD proprietary: [K].
- Version coupling: newer libplacebo requires newer Vulkan than lavu's hwcontext; FFmpeg added an import-failure fallback to an internal device [V https://ffmpeg.org/pipermail/ffmpeg-devel/2023-May/309476.html]. Mixed FFmpeg/libplacebo versions are a known breakage source; the BtbN build statically pins one consistent pair, which is good.

### 1.3 Working command lines (all tested locally on 6.1.1 unless marked)
Software graph, SW encoder (simplest, tested ~33-39 fps incl. libx264 medium CRF 20 at 1080x1920; the CPU encoder is the bottleneck):
```
ffmpeg -i in.mp4 -vf "libplacebo=w=1080:h=1920:upscaler=ewa_lanczossharp:deband=1:format=yuv420p" \
  -c:v libx264 -preset slow -crf 18 -pix_fmt yuv420p -colorspace bt709 -color_primaries bt709 -color_trc bt709 -c:a copy out.mp4
```
Explicit device (pin the GPU), SW encoder (tested ~41 fps with x264 medium CRF18; 75 fps with null sink):
```
ffmpeg -init_hw_device vulkan=vk:0 -i in.mp4 -vf "format=yuv420p,hwupload,libplacebo=w=1080:h=1920:upscaler=ewa_lanczossharp:format=yuv420p,hwdownload,format=yuv420p" -c:v libx264 -crf 18 out.mp4
```
VAAPI encode (Linux Intel/AMD), single graph, tested ~64 fps:
```
ffmpeg -vaapi_device /dev/dri/renderD128 -i in.mp4 -vf "libplacebo=w=1080:h=1920:upscaler=ewa_lanczossharp:format=nv12,hwupload" -c:v h264_vaapi -qp 20 out.mp4
```
Combining BOTH `-init_hw_device vulkan` and a vaapi device in one graph failed in my tries (`hwupload` picks the single `-filter_hw_device`; "Impossible to convert between formats" error) [V local failure]. Use the SW-frames form above, or two processes with a rawvideo pipe (tested working: `ffmpeg ... libplacebo=format=nv12,hwdownload,format=nv12 -f rawvideo - | ffmpeg -f rawvideo -pix_fmt nv12 -s WxH -r R -i - -vaapi_device ... -vf format=nv12,hwupload -c:v h264_vaapi`).
NVENC / QSV / AMF [NOT TESTED, no hardware]: because libplacebo outputs SW frames, just append the encoder with plain system-memory input: `-c:v hevc_nvenc -preset p5 -tune hq -rc vbr -cq 21 -b:v 0`, `-c:v hevc_qsv -global_quality 22`, `-c:v hevc_amf -quality quality -rc cqp -qp_i 22 -qp_p 24`. NVENC/AMF/QSV accept yuv420p/nv12 sw frames and do the upload internally [K]. Zero-copy Vulkan->NVENC interop is not worth it.
Software HEVC/AV1: `-c:v libx265 -crf 22 -preset medium -tag:v hvc1`, `-c:v libsvtav1 -crf 30 -preset 6` (encoders listed present in BtbN build per task statement; local box has nvenc/qsv/vaapi listed [V]).

### 1.4 Speed measurements [V local, i7-1355U, 360p->1080x1920, null sink, 300 frames]
| Chain | fps |
|---|---|
| libplacebo ewa_lanczossharp, Intel iGPU (hwupload form) | 75 |
| libplacebo spline36, iGPU | 127 |
| zscale spline36 (CPU) | ~very high, limited by test (speed 23.9x of 30 fps = ~700 fps) |
| swscale lanczos + deblock + hqdn3d + cas (CPU) | ~116 (300 frames/2.59 s) |
| nlmeans s=2 p=5 r=11 at 360p + lanczos (CPU) | 13 |
| libplacebo EWA on llvmpipe | did not finish a usable result within the test (treat as unusable) |
Source was a synthetic testsrc2 clip, so denoise filters were not under real load; numbers show relative cost only.

### 1.5 Failure modes / robust detection
- No Vulkan loader or ICD at all: filter init fails ("Failed to create Vulkan instance/device"), exit non-zero [K].
- Software Vulkan (llvmpipe/lavapipe) exists on virtually every Linux desktop and WILL be selected if it is device 0 or the only one. Must reject by name ("llvmpipe", "lavapipe", "SwiftShader", "software" in GPU listing) [V local: listing literally shows `llvmpipe ... (software)`].
- Probe recipe (cheap, ~0.3 s): `ffmpeg -v verbose -init_hw_device vulkan=vk:N -f lavfi -i color=s=64x64:d=0.1 -vf "format=yuv420p,hwupload,libplacebo=format=yuv420p,hwdownload,format=yuv420p" -f null -`; parse the "GPU listing:" lines (index, name, `(integrated)/(discrete)/(software)`), and exit code. Iterate N=0.. until failure. Both steps were verified to work locally [V]; the parser format is stable across 6.x-7.x [S] but check on n9.0.2.
- Then run a real benchmark (30 frames at target size) and cache per (GPU name, driver version, ffmpeg build id).
- Noise on stderr: `MESA-INTEL: warning ... FINISHME` is harmless [V local]; do not treat stderr as failure.
- Mid-stream device loss / OOM [K]: ffmpeg aborts; the app should retry the same job on Tier 0.
- Old iGPUs (pre-Gen9 ANV) lack features libplacebo needs [K].
- HDR/10-bit input: set `format`/`colorspace` explicitly and tone-map before AI stage [K].

## 2. Best CPU chain for low-quality sources (360p-720p)

Filters present locally [V]: `deblock`, `pp`, `pp7`, `spp`, `fspp`, `uspp`, `hqdn3d`, `nlmeans`, `bm3d`, `smartblur`, `cas`, `zscale`, `noise`.

Recommended order (all at source resolution except sharpen/grain), cheap -> costly:
1. Deblock: `deblock=filter=weak:block=8:alpha=0.12:beta=0.06` (native FFmpeg, in-loop style, cheap) [V exists; parameter defaults [K]]. For heavy 360p macroblocking prefer `spp=4:6` or `pp7=qp=5:mode=medium` (libpostproc family, GPL, in BtbN GPL build; `uspp` is slower and better; `fspp` fast). The postproc filters are older and soften; test visually. All are single/low-thread cost, ~10-25 ms/frame at 360p [K].
2. Denoise: `hqdn3d=2:1.5:3:3` (temporal+spatial, very cheap, helps stability). `nlmeans` is ~10x more expensive: measured 13 fps total at 360p on 12 threads, and it scales with pixels; at 720p expect ~3-4 fps [V local at 360p, extrapolation]. Use nlmeans only in "Max quality / slow" mode. `nlmeans_vulkan` exists in the filter list (6.1.1 [V from doc 02]); I did not test it; it needs the hwupload/Vulkan graph and costs a CPU<->GPU round trip.
3. Scaler: single step. `zscale=w:h:filter=spline36` (16-bit precision, correct colour handling) or `scale=...:flags=lanczos+accurate_rnd+full_chroma_int`. Both exceed 100 fps at 360p->1080p here. Spline36 gives less ringing than lanczos on blocky sources [K].
4. Sharpen: `cas=strength=0.4` (V filter exists; measured within the 116 fps chain). Prefer cas over unsharp for upscaled content (less halo) [K]. If banding appears add libplacebo-style deband or `deband` filter before scaling.
5. Grain: `noise=alls=2:allf=t` after sharpen, and encode in 10-bit if possible [K].
Realistic speeds on a mid CPU (this i7-1355U, 12 threads): deblock+hqdn3d+lanczos+cas = ~116 fps at 360p->1080x1920 [V local]; the x264 slow encode at 1080x1920 will be the bottleneck (typically 20-40 fps) [K].

## 3. AI tier status (2026-10)

### 3.1 Engines
| Item | Status | Licence |
|---|---|---|
| xinntao/Real-ESRGAN-ncnn-vulkan | Latest release v0.2.0 (2022-04-24); earlier v0.1.3.2; repo ~38 commits; README lists 4 models: `realesr-animevideov3` (default), `realesrgan-x4plus`, `realesrgan-x4plus-anime`, `realesrnet-x4plus`. Options `-s 2/3/4`, `-t`, `-j`, `-g`, `-f jpg/png/webp`. No stdin/stdout; README TODO lists "some PCs output black images". Win/Linux/mac zips. [V https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan , https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan/releases] | MIT code; Real-ESRGAN weights BSD-3-Clause [V https://github.com/xinntao/Real-ESRGAN/blob/master/LICENSE] |
| realesr-general-x4v3 | Exists in PyTorch model zoo as "tiny small model", also usable at x1/x2/x3 via interpolation; "not too strong deblur and denoise" [V model_zoo.md]. NOT in the ncnn repo's model list [V]; an ncnn conversion (pnnx/onnx) is DIY [K]. | BSD-3 |
| realesr-animevideov3 | XS model; PyTorch speed (model time only, input res): V100 640x480 65.9 fps, 1280x720 22.6 fps, 1920x1080 10.0 fps [V https://github.com/xinntao/Real-ESRGAN/blob/master/docs/anime_comparisons.md]. | BSD-3 |
| nihui/realcugan-ncnn-vulkan | MIT; scales 1/2/3/4x, denoise levels -1..3, model dirs models-se/-pro/-nose; README says "early development stage" [V https://github.com/nihui/realcugan-ncnn-vulkan]. Anime-only. File/dir based like Real-ESRGAN. | Code MIT [V]; weights: bilibili/ailab repo says LICENSE file present but the text was not retrievable [unverified; PyPI and nihui pages say MIT]. |
| Upscayl `upscayl-ncnn` (upscayl-bin) | Fork of Real-ESRGAN-ncnn-vulkan, 214 commits, **AGPL-3.0** [V https://github.com/upscayl/upscayl-ncnn]; Upscayl app v2.15 also AGPL; extra models in upscayl/custom-models with mixed per-model licences [V https://github.com/upscayl/upscayl]. Image-oriented, no stdin streaming seen [S]. Do not bundle. | AGPL-3.0 (blocker) |
| Video2X 6.x | AGPL-3.0, build fragility (see doc 02 [V]). Avoid. | AGPL |
| ncnn library | BSD-3 (Tencent) [K] | BSD-3 |

### 3.2 ONNX Runtime from Rust
- `ort` crate (pykeio): 2.0.0-rc.13, 2026-07-28, wraps ORT 1.28, dual MIT/Apache-2.0, 20M+ downloads [V https://crates.io/api/v1/crates/ort , https://github.com/pykeio/ort].
- EPs documented: CUDA (docs say CUDA >= 13.2, cuDNN >= 9.23 for this release!), TensorRT, DirectML (needs helper DLLs), OpenVINO, ROCm, MIGraphX, CoreML, WebGPU ("experimental, may produce incorrect results/crashes") [V https://ort.pyke.io/perf/execution-providers]. EPs are Cargo features and registered in preference order; unsupported ops fall back to the next EP and finally CPU; `.error_on_failure()` forces strictness [V].
- DirectML: sustained engineering, no new features; Windows ML (Win11 24H2+) is the successor on the same ORT APIs [V search: Microsoft docs summary; I did not open the primary page].
- Cross-vendor gap [K/S]: Windows = DirectML (any DX12 GPU); Linux NVIDIA = CUDA/TensorRT; Linux AMD = ROCm/MIGraphX (limited GPU support, large install); Linux Intel = OpenVINO; no single Linux EP for all vendors. WebGPU EP (Dawn -> Vulkan) is the only possible common path and is flagged experimental by `ort` itself. Therefore ORT on Linux means shipping several large provider libs.
- Size [K]: ORT CPU ~10-20 MB; CUDA/cuDNN stack hundreds of MB to GB (cannot be redistributed trivially: NVIDIA redistribution terms apply [unverified]).

### 3.3 Small video SR models
- SPAN (Swift Parameter-free Attention): Apache-2.0 code; ONNX/TensorRT 2x reached 95.6 fps on RTX A6000 (resolution not stated in the snippet) [S arXiv 2311.12770 + search]. Community-trained SPAN weights on OpenModelDB carry per-model licences.
- Compact/SRVGGNet (Real-ESRGAN family): tiny; AnimeJaNai HD V3 Compact is CC-BY-NC-SA-4.0 [V https://openmodeldb.info/models/2x-AnimeJaNai-HD-V3-Compact]; AnimeJaNai guidance: RTX 3080 minimum for realtime UltraCompact at 1080p, RTX 4090 for Compact [S mpv-AnimeJaNai]. NC: DO NOT SHIP.
- RealPLKSR, other OpenModelDB models: licence varies per model (CC-BY-4.0, CC-BY-NC, etc.) [K; not individually verified]. If a model is wanted, train/fine-tune own on permissive data, or use Real-ESRGAN's BSD-3 weights.
- ArtCNN, waifu2x, RealESRGAN v2/v3, Real-CUGAN, RIFE, SCUNet are what vs-mlrt wraps; vs-mlrt is GPL-3.0 and shows the multi-backend pattern (TRT, ORT-CUDA, OpenVINO, MIGraphX, NCNN-Vulkan "smaller footprint, device-independent, some speed cost") [V https://github.com/AmusementClub/vs-mlrt].

### 3.4 Throughput numbers (540p->1080p)
I could NOT find verified numbers for mid-range consumer GPUs or Intel iGPU. Only [V] data: V100 PyTorch model-time for animevideov3 (above): 640x480 input = 66 fps, 1280x720 = 22.6 fps. Scale to 540p (960x540 = 0.52 MP vs 0.31 MP at 640x480 and 0.92 MP at 720p): roughly 30-35 fps on V100 [derived]. ncnn-Vulkan on a mid GPU (RTX 3060 / RX 6600 class) is typically 0.3-0.7x of V100 PyTorch fp16 [K, unverified], so ~10-20 fps for Compact-class models at 540p->2x; iGPU (Iris Xe/780M) another 3-8x slower, i.e. ~2-5 fps [K, unverified]. Heavy RRDB (x4plus) is 5-15x slower than the small models [K]. The app MUST benchmark at runtime (10 frames) and not rely on tables. I could not test AI inference locally (no ncnn/ORT binaries; the iGPU is available, so a local ncnn benchmark is a good first task).

### 3.5 Temporal flicker
Covered thoroughly in doc 02 section 3 [K]. Added notes: (a) per-frame SR has no [V]-citable quantified flicker data in my sources; (b) mitigations in cost order: temporal denoise before (hqdn3d/atadenoise), blend 20-40% Lanczos reference after, fixed overlapped tile grid, light grain; (c) true VSR (BasicVSR++/RTX VSR) are NVIDIA/PyTorch-heavy and out of scope.

## 4. Feeding frames: architecture recommendation

Options:
- A) Existing image-dir tools (realesrgan-ncnn-vulkan, upscayl-bin, realcugan-ncnn-vulkan): file-based only [V no stdin in README]. Disk cost: PNG of 540p ~0.3-0.6 MB/frame in and ~1-2 MB out (2x) i.e. ~1.5-2.5 MB/frame total; a 60 s 30 fps clip = 1800 frames = ~3-4.5 GB temp; 5 min = ~20 GB [derived estimate]. Acceptable only as a debug fallback.
- B) Own ncnn-based worker process, rawvideo in/out over pipes.
- C) In-process `ort` inference in Rust, ffmpeg rawvideo pipes in/out.

Recommendation for Tier 2: **B, an own `rmedia-upscale-worker` using the ncnn C API (statically linked, Vulkan), with Real-ESRGAN-family weights converted to ncnn, fed by two ffmpeg subprocesses over rawvideo pipes (`-f rawvideo -pix_fmt rgb24` decode side; worker stdin/stdout; encode side with original file as second input for audio).** Rationale:
1. Only ncnn-Vulkan is truly cross-vendor with ONE small dependency (NVIDIA/AMD/Intel, Windows+Linux, no CUDA/ROCm/OpenVINO). ORT needs 2-4 different EP stacks and `ort` is still RC; DirectML is in maintenance and the Linux AMD/Intel story differs.
2. Size: ncnn static in a worker ~ a few MB + models ~ few MB each; no cuDNN.
3. Licence isolation: separate process matches the project's "tools via argv/exec::process" rule (CLAUDE.md); BSD-3 + MIT/BSD-3 weights are freely redistributable.
4. Perf: ncnn matches/ beats DirectML for these small convnets (vs-mlrt notes ncnn is slower than TensorRT but broader) [S]; TRT/CUDA stays an optional later plugin.
5. Pipe cost: 960x540 rgb24 = 1.5 MB/frame in, 1080p-class 2x output ~6 MB/frame; at 30 fps ~ 200 MB/s on a pipe - fine [derived]. Trim/crop/fps-normalise at the decode ffmpeg.
6. Risk: ncnn C API drift (Video2X pinned-version failure [V doc 02]); mitigate by pinning ncnn, and keep the worker protocol (header + raw frames + JSON progress on stderr) engine-agnostic so an ORT worker can replace it later.
Not verified: a ready-made Rust ncnn binding (`ncnn-rs` etc.) and its maintenance status; building a tiny C++ worker is the safe path. Also unverified: conversion of realesr-general-x4v3 to ncnn (pnnx/onnx2ncnn) works without op issues; PReLU/PixelShuffle are supported by ncnn [K].
Fallback tiers: Tier 1 (libplacebo, section 1) and Tier 0 (section 2) need no worker.

## 5. Redistribution licences (what we would ship)
| Artifact | Licence | Redistribute? | Notes |
|---|---|---|---|
| FFmpeg BtbN GPL build n9.0.x | GPL-2.0+/3 (includes libx264, libx265) | Yes with source offer + licence text; app calling it as a subprocess is "aggregate" [K] | BtbN publishes 9.0 / 8.1 GPL builds (win64 gpl-shared 9.0 72.9 MiB) [V https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest]. x264/x265 patent/royalty issues for HEVC [K]. |
| libplacebo | LGPL-2.1+ [K] | Yes, already inside the FFmpeg build | |
| ncnn | BSD-3-Clause [K] | Yes | Include copyright notice |
| Real-ESRGAN code + weights | BSD-3 [V] | Yes, keep notice, no endorsement | `realesr-general-x4v3` / `animevideov3` / `x4plus*` |
| realesrgan-ncnn-vulkan binary | MIT [V] | Yes, but unmaintained, not needed | |
| Real-CUGAN ncnn | MIT code [V]; weights licence unconfirmed | Check bilibili/ailab LICENSE before shipping | anime only, optional |
| ort crate | MIT/Apache-2.0 [V] | Yes | ORT itself MIT [K]; CUDA/cuDNN have NVIDIA terms [unverified]; DirectML.dll is a Microsoft redistributable [K] |
| Upscayl / upscayl-ncnn | AGPL-3.0 [V] | NO (copyleft over our app if linked; subprocess possible but avoid) | |
| Video2X | AGPL-3.0 [V doc 02] | NO | |
| vs-mlrt | GPL-3.0 [V] | Reference only | |
| AnimeJaNai (all) | CC-BY-NC-SA-4.0 [V] | NO (non-commercial, share-alike) | Same for many OpenModelDB models: check each |
| SPAN code | Apache-2.0 [S] | Yes; weights depend on who trained them | |
| Anime4K GLSL | MIT [K] | Yes via `custom_shader_path` | Anime-only; unverified in this session |
| NVIDIA Maxine/RTX VSR SDK | NVIDIA EULA | Do not bundle [K, unverified] | |

## 6. Not verified (explicit)
- Everything on FFmpeg 8.x/9.x: option names, behaviour, BtbN n9.0.2 filter inventory (only 6.1.1 tested). BtbN has 9.0 and 8.1 builds [V]; the exact "n9.0.2" tag and its libplacebo/Vulkan SDK versions were not confirmed.
- libplacebo on RADV and NVIDIA Windows (not tested; only Intel ANV tested).
- NVENC / AMF / QSV command lines (no hardware here; QSV and NVENC encoders are listed [V] but not run).
- Real AI throughput on any GPU or iGPU; all fps for AI are derived/estimated except the V100 numbers.
- Model licences for Real-CUGAN weights, SPAN community weights, RealPLKSR, Anime4K, and ncnn/libplacebo licences [K].
- Existence of an official ncnn conversion of realesr-general-x4v3 (not in the ncnn repo's model list [V]).
- Whether libplacebo software-frame input uses the optimal GPU on multi-GPU/hybrid machines.

## Sources
- https://ffmpeg.org/ffmpeg-filters.html (libplacebo section; truncated fetch)
- https://ffmpeg.org/pipermail/ffmpeg-devel/2023-May/309589.html , .../309476.html (libplacebo no-hwdevice / import fallback)
- https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan and /releases
- https://github.com/xinntao/Real-ESRGAN/blob/master/LICENSE , docs/model_zoo.md , docs/anime_comparisons.md
- https://github.com/nihui/realcugan-ncnn-vulkan , https://github.com/bilibili/ailab/tree/main/Real-CUGAN
- https://github.com/upscayl/upscayl-ncnn , https://github.com/upscayl/upscayl
- https://github.com/pykeio/ort , https://crates.io/api/v1/crates/ort , https://ort.pyke.io/perf/execution-providers
- https://github.com/AmusementClub/vs-mlrt
- https://openmodeldb.info/models/2x-AnimeJaNai-HD-V3-Compact
- https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest
- Local tests: FFmpeg 6.1.1, Intel RPL-U iGPU (ANV) + llvmpipe, i7-1355U.
