# 02 - Upscaling Engines (research outline)

Date: 2026-10-06. Target: Linux + Windows, NVIDIA/AMD/Intel/no GPU; low-quality social video (360p-720p) -> ~1080x2400..1320x2868 portrait.

Confidence legend: [V] verified this session, [K] from background knowledge (pre-mid-2026), re-verify before committing. Web access this session was minimal; treat all speed numbers as order-of-magnitude.

Verified locally [V]: dev box has FFmpeg 6.1.1 (Ubuntu) with filters `libplacebo`, `scale_vulkan`, `zscale`, `nlmeans`, `nlmeans_vulkan`, `cas`, `dnn_processing`. Distro FFmpeg is a poor bundling basis though; ship own build (BtbN / gyan.dev style builds, or static builds).

Verified via search [V]: Video2X latest packaged release is 6.4.0 (AUR 6.4.0-3, Feb 2025); backends Real-ESRGAN, Real-CUGAN, RIFE via ncnn+Vulkan, plus Anime4K and libplacebo. A Feb 2026 AUR issue reports build failure from pinned librife-ncnn-vulkan vs. newer ncnn API (i.e. fragile to build from source).
Sources: [AUR video2x](https://aur.archlinux.org/packages/video2x), [Video2X CLI docs](https://docs.video2x.org/running/command-line.html), [Video2X 6.0.0 release](https://newreleases.io/project/github/k4yt3x/video2x/release/6.0.0).

---
## 1. Classical scaling in FFmpeg

### 1.1 Scalers
| Filter | Where | Notes |
|---|---|---|
| `scale` (swscale) `flags=lanczos` / `spline` / `bicubic` | CPU, everywhere | Lanczos (a=3) is the sane default; `spline` slightly softer/less ringing. Use `accurate_rnd+full_chroma_int` for quality. Multithreaded but slow-ish at 4K-ish output; 480p->1080x2400 is real-time-ish on modern 6+ core CPU. |
| `zscale` (zimg) | CPU, needs `--enable-libzimg` | Higher-precision (16-bit float path), better colorspace/transfer handling and dither; `filter=lanczos`. Good when HDR/10-bit or exact colorimetry matters. Single-ish thread, slower than swscale. |
| `libplacebo` | GPU via Vulkan (all vendors) | Best classical quality: `upscaler=ewa_lanczossharp` (polar EWA, near best), `downscaler=ewa_lanczos`, custom GLSL hooks via `custom_shader_path` (e.g. Anime4K GLSL, FSRCNNX, ravu, NVScaler). Also tone mapping, debanding (`deband=1`), dithering. Needs FFmpeg built `--enable-libplacebo --enable-vulkan`, Vulkan runtime (all GPUs incl. llvmpipe/lavapipe CPU software Vulkan - very slow). Moderate speed: 1080p-class output at hundreds of fps on mid GPU for EWA. Filter option names changed between FFmpeg 6.x and 7.x (e.g. `w/h` vs `out_w`, `upscaler` vs `scaler`) - pin FFmpeg version. |
| `scale_vulkan` | GPU Vulkan | Only bilinear/nearest (+ bicubic variants depending on version); fast but low quality. Not worth it vs libplacebo. |
| `scale_cuda` | NVIDIA | bilinear/bicubic/lanczos (newer FFmpeg 7+), keeps frames on GPU for NVENC. |
| `scale_npp` | NVIDIA, non-free build | `interp_algo=lanczos` / `super`; not redistributable in builds (non-free). |
| `scale_qsv` / `vpp_qsv` | Intel | Hardware scaling; mediocre quality (QSV has no great scaler "AI"). |
| `scale_vaapi` | Linux AMD/Intel | `mode=hq` reasonable; Linux-only. |

Quality ranking (rough): libplacebo ewa_lanczossharp ≈ zscale lanczos > swscale lanczos/spline > bicubic > cuda/vaapi/qsv hw scalers > bilinear.

Portrait-specific: sources are often landscape or portrait video at 9:16. Plan the output as: scale to cover, crop/pad per user choice. Scale by integer-ish ratios where possible; do a single scaling step (never 2 stages of resize unless the AI stage is x2/x4 followed by a downscale to exact target).

### 1.2 Sharpening
- `unsharp=5:5:0.8:5:5:0.0` (luma only) - cheap, can halo; use mild (0.5-1.0).
- `cas` (Contrast Adaptive Sharpen, from FidelityFX; FFmpeg 4.4+ `cas=strength=0.4`) - less haloing, better for upscaled content; strength 0.3-0.6. [V filter present].
- `libplacebo` has no standalone sharpen but `ewa_lanczossharp` is pre-sharpened; shaders can add.
- Sharpen AFTER upscale, BEFORE encode, and keep mild: encoder spends bits on sharpened noise.

### 1.3 Denoise / deblock for compressed sources (pre-processing)
Order: deblock/denoise at SOURCE resolution -> scale -> light sharpen -> (optional) light grain/dither.
- `deblock=filter=weak|strong:block=8` (H.264/MPEG-style blocking; FFmpeg 4.0+). Cheap. Reddit/TikTok-style recompressed sources benefit.
- `hqdn3d=2:1.5:3:3` - fast temporal+spatial denoise; gives temporal stability (reduces flicker pre-SR). Cheap, CPU.
- `nlmeans` (CPU, very slow; `nlmeans_vulkan` / `nlmeans_opencl` GPU variants, [V] vulkan variant present in 6.1.1). Best quality; use `s=2.0:p=5:r=11` style small params at low res (480p) so cost is bounded.
- `bm3d`, `atadenoise`, `vaguedenoiser` - other options; `atadenoise` good temporal-cheap.
- `libplacebo deband=1` or `deband` filter for gradient banding in low-bitrate sources (very common in compressed social video).
- Caveat: AI models (Real-ESRGAN) already handle compression artifacts; extra heavy denoise before them can over-smooth. For AI tier use mild or none (`hqdn3d` light). For classical tier use deblock + hqdn3d.

Speed vs quality summary (480p source -> 1080x2400, 30fps clip, mid-range desktop; indicative [K]):
- swscale lanczos + hqdn3d + cas: 60-200+ fps on CPU.
- libplacebo EWA on iGPU/dGPU: >200 fps.
- nlmeans CPU: <5 fps even at 480p; GPU variants 30-100 fps.

Example classical chain (CPU only, ffmpeg 6/7):
```
-vf "deblock=filter=weak:block=8,hqdn3d=1.5:1.5:4:4,scale=1080:1920:flags=lanczos+accurate_rnd+full_chroma_int,cas=0.4,format=yuv420p10le"
```
GPU: `-init_hw_device vulkan -vf "hwupload,libplacebo=w=1080:h=1920:upscaler=ewa_lanczossharp:deband=1,hwdownload,format=yuv420p"` (verify option names for the pinned build).

---
## 2. AI/ML upscalers (state as of 2026-10; [K] unless marked)

### 2.1 Comparison table
| Engine | License (code) | Model licensing | Platforms | GPU vendors | CPU viable? | Speed 480p->x4 (indicative) | Bundling |
|---|---|---|---|---|---|---|---|
| **realesrgan-ncnn-vulkan** (xinntao) | MIT | Models BSD-3 (Real-ESRGAN weights) ; ship `realesr-animevideov3`, `realesrgan-x4plus`, `realesrgan-x4plus-anime`, `realesr-general-x4v3` | Win/Linux/mac prebuilt zips | Any Vulkan (NVIDIA, AMD, Intel) | Technically via lavapipe, effectively no (<0.2 fps) | animevideov3 (tiny SRVGG, ~1.2M params): ~10-30 fps on RTX 3060-class at 480p->1080p-ish; x4plus (RRDB, 16M): 1-4 fps. AMD/Intel iGPU: 1/3 to 1/10 of that | Easy: single binary + `models/` dir, ncnn statically linked, needs only Vulkan driver. Project is low-maintenance (last meaningful updates ~2022-2023) but stable. |
| **realesr-general-x4v3** | (in Real-ESRGAN repo) | BSD-3 | as above | as above | - | small model, ~same as animevideov3; general-photo/video use, has a denoise-strength interpolation (`-dn` with wdn model) | Included in PyTorch repo; ncnn port exists in the release zip as `realesr-general-x4v3` [verify] |
| **Real-CUGAN (ncnn-vulkan)** (bilibili/nihui) | MIT code | Model: MIT-ish (check); anime only | Win/Linux/mac | Any Vulkan | No | Comparable to animevideov3; 2x/3x/4x with conservative/denoise variants; good for anime | Easy (same ncnn pattern) |
| **Video2X 6.x** (k4yt3x) | AGPL-3.0 (!) | Bundles Real-ESRGAN, Real-CUGAN, RIFE, Anime4K (GLSL via libplacebo) models | Win (installer/portable), Linux (AppImage, Arch pkg, Docker) | Vulkan any vendor | No | = underlying engine | v6 is a C++ rewrite: libvideo2x (FFmpeg libav* + ncnn + libplacebo) with CLI `video2x -i in -o out -p realesrgan -s 4 -m realesr-animevideov3`. AGPL license is a **blocker for closed-source embedding**; fine as external subprocess only if app is AGPL-compatible or user-installed. Latest packaged 6.4.0 [V]. Build fragility with ncnn API drift [V]. Recommend: do NOT depend on it; copy the architecture, link ncnn directly or use per-engine ncnn binaries. |
| **Anime4K** (bloc97) | MIT | GLSL shaders MIT | via mpv / libplacebo `custom_shader_path` / Video2X | Any Vulkan (libplacebo) | libplacebo on lavapipe: slow | Real-time (60fps @1080p on iGPU for mode A/B light; heavier "Ultra" modes needs mid dGPU) | Very easy: just .glsl files with ffmpeg libplacebo. **Anime-tuned**; on live action it creates painterly artifacts. Great for the lightweight GPU tier for anime content. |
| **waifu2x-ncnn-vulkan** (nihui) | MIT | models MIT | Win/Linux/mac | Any Vulkan | No | Older, fast, 2x only; denoise levels 0-3 | Easy but lower quality than Real-ESRGAN for video; legacy. |
| **RIFE (rife-ncnn-vulkan)** | MIT | Models (practical-RIFE) research/MIT-ish, some versions non-commercial - **check per version** | Win/Linux/mac | Any Vulkan | No | 2x interpolation 480p: 50-200 fps on mid GPU | Optional: frame interpolation 30->60 fps. Not an upscaler; adds artifacts on scene cuts & big motion; skip in v1. |
| **SwinIR / HAT / BasicVSR++ / RealBasicVSR** | Apache-2.0 / Apache / Apache (MMagic) | Weights mostly Apache or research-only; HAT weights ok; **verify** | PyTorch only, CUDA | NVIDIA realistically (ROCm Linux-only, partial) | No (seconds/frame) | SwinIR/HAT: 0.1-1 fps; BasicVSR++/RealBasicVSR: 1-5 fps on 4090-class at low res, need multi-GB VRAM and frame windows | Heavy (PyTorch + CUDA ~ GBs). Quality best on real video (VSR with temporal propagation), but not suited to a lean cross-vendor app. Possible "pro" optional plugin on NVIDIA. |
| **NVIDIA RTX Video Super Resolution (VSR)** | Proprietary | n/a | Windows (browser/driver, mpv/VLC, Chrome/Edge); **NVIDIA Video Effects SDK (Maxine VFX)** Windows + Linux | RTX 20/30/40/50 only | No | Real-time 1080p/4K playback; SDK offers upscale up to 4x with quality levels, ~several ms/frame | SDK is free to download but license restricts redistribution/terms (NVIDIA SDK EULA; check); requires NVIDIA runtime libs; large (~GB w/ models). Optional "NVIDIA bonus" tier. Dev machine has no NVIDIA - untestable locally. Note: RTX VSR in the driver is playback-only, no export API besides the SDK. |
| **Intel / AMD equivalents** | - | - | - | - | - | AMD FSR 1/2/3 are game/shader-level spatial/temporal (FSR1 EASU/RCAS shaders are MIT and can be run as GLSL hooks in libplacebo/mpv, but are classical, not AI). AMD "Fluid Motion Frames" / driver video upscaling (RSR/"Video Upscaling" in Adrenalin) are playback-only, no API. Intel "Video Super Resolution" (Arc/Core Ultra driver) is playback-only; Intel Xe SR (XeSS) is game-only. **Conclusion: no vendor-neutral offline API other than Vulkan/ONNX.** |
| **FFmpeg `dnn_processing` / `sr` filter** | LGPL (FFmpeg) | user supplied models | all | backends: TensorFlow (C lib), OpenVINO (Intel CPU/iGPU), native (tiny, CPU only), torch (libtorch, 7.x) | Native CPU backend ok only for tiny nets (ESPCN/SRCNN, 2x); `sr` filter deprecated-ish | ESPCN/SRCNN quality ≈ bicubic+; far below Real-ESRGAN | Poor ergonomics: FFmpeg builds rarely compiled with these; ONNX backend NOT in mainline FFmpeg (as of knowledge; verify 7.1/8.0). Local FFmpeg 6.1.1 lists `dnn_processing` [V] but backend support unverified. Not recommended. |
| **ONNX Runtime (ORT)** | MIT | any ONNX model (Real-ESRGAN, SPAN, Compact, etc. converted) | Win/Linux/mac | EPs: **DirectML** (Windows, any DX12 GPU: NVIDIA/AMD/Intel; DirectML is in maintenance mode, Microsoft moved to Windows ML / WinML in 2025), **CUDA/TensorRT** (NVIDIA), **OpenVINO** (Intel CPU/iGPU/NPU, also faster than plain CPU), **ROCm/MIGraphX** (AMD Linux), **CPU** (XNNPACK/MLAS) | CPU feasible only for very light nets (SPAN/Compact: ~1-5 fps at 480p->2x with 8 threads) | DirectML on mid GPU ~ 0.5-1x of ncnn-vulkan for same net | Medium: ORT CPU ~10-20 MB; GPU EPs add 100-500 MB (CUDA/cuDNN, ~GBs). No single package covers all vendors on Linux (AMD Linux needs ROCm; Intel needs OpenVINO). Pro: access to the newer, better-than-RealESRGAN-compact models (e.g. from OpenModelDB: SPAN, Compact, RealPLKSR, ESRGAN variants). |

### 2.2 Notes per engine
- **Real-ESRGAN family**: dominant practical choice. For compressed live-action social video, `realesr-general-x4v3` or `realesr-animevideov3` (despite the name, widely used for video; sharper-but-plasticky on faces) are the efficient models. x4plus is slow and heavily hallucinates. Native 4x output must be downscaled to target (e.g. 480x854 -> 1920x3416 -> 1080x1920): wasteful. Better: run x2-style (use x4 model on a *pre-downscaled* source, or run 4x then lanczos-downscale), or tile at 2x input if model supports it. Check `-s 2` for animevideov3 (it has 2x/3x/4x variants).
- **OpenModelDB** (openmodeldb.info) hosts community models with per-model licenses (CC-BY-4.0, CC-BY-NC, etc.) - **check each model's license**; commercial use of NC models is not allowed. Convert PyTorch -> ncnn (via pnnx) or ONNX with `chaiNNer`/`spandrel` tooling.
- **Compact/SPAN/RealPLKSR families** (2023-2025): faster-than-ESRGAN, ONNX-friendly, popular in the video community (VapourSynth-based pipelines like `vs-mlrt`, which wraps TensorRT/ORT/OpenVINO/NCNN-Vulkan). `vs-mlrt` (AmusementClub) is the reference for how multi-backend (TRT, ORT-CUDA/DML/OpenVINO, ncnn-vulkan) is wired; GPL-ish license, use as reference.
- **ncnn**: Tencent BSD-3; Vulkan compute, supports NVIDIA/AMD/Intel/ARM/Apple(MoltenVK); CPU fallback exists (ncnn CPU with fp16/int8 and SIMD) but slow for these nets; as a library, can call `ncnn::Net` directly with `opt.use_vulkan_compute`; in-process via C API/pybind is possible.
- **Vulkan on Windows/Linux GPU drivers**: all three vendors ship Vulkan; Linux AMD needs Mesa RADV (bundled in distros), Intel ANV, NVIDIA proprietary. Old/ancient iGPUs (Intel Gen7/8) may lack features; ncnn falls back with `-g -1` to CPU.
- **CPU-only**: AI tier effectively not viable for 720p->1080p at usable speed; allow as opt-in "slow, offline" using ORT-CPU + Compact/SPAN 2x model or ncnn CPU, with ETA warning. Default for no GPU = classical tier.

---
## 3. Temporal consistency / flicker in per-frame SR
Problems:
- Frame-independent hallucination: textures (skin, hair, grass, text) "boil/shimmer" frame to frame; edges crawl.
- Input compression artifacts differ per frame (I vs P/B frame quality pumping, GOP-periodic flicker) and get amplified by GAN models.
- Tiling seams/ varying tile boundaries -> block flicker if tiles not overlapped or if tile padding differs.
- Scene cuts handled fine; fast motion cause ghosting only with multi-frame models.
- Color/brightness shifts from model normalization per tile.
Mitigations (cheap -> expensive):
1. Pre-denoise temporally (`hqdn3d`, `atadenoise`) at source res so model sees stable input (best ROI).
2. Choose less-hallucinating models (`realesr-general-x4v3` with denoise strength, Compact/SPAN, animevideov3); avoid x4plus GAN-heavy for live action.
3. Post temporal smoothing on output: `tmix` / `atadenoise` / `hqdn3d` with low temporal-only strength (e.g. `hqdn3d=0:0:3:3`) or `minterpolate` (don't). Risk: ghosting; keep weights tiny and reset at scene cuts (`select='gt(scene,0.3)'` split or `tmedian`).
4. Blend model output with classical upscale: `blend=all_mode=average:all_opacity=0.3` of Lanczos reference into AI output - reduces hallucination and flicker, costs almost nothing. Worth offering as "AI strength" slider.
5. Overlapping tiles (pad 16-32 px, ncnn `-t` tile size with automatic padding) and fixed tile grid across frames.
6. Optical-flow-warp previous output and blend (per-frame + flow, like "temporal consistency via flow warp"): needs RAFT/RIFE flow; expensive; v2+ only.
7. True video SR models (BasicVSR++, RealBasicVSR, TecoGAN-like, NVIDIA VSR): inherently temporal; heavy and mostly NVIDIA/CUDA. NVIDIA RTX VSR/Maxine is built for temporal stability but is NVIDIA-only.
8. Fixed random seeds irrelevant (Real-ESRGAN is deterministic); but fp16 vs fp32 nondeterminism between runs can differ slightly - use same precision throughout.
9. Keep grain: add light film grain/dither (`noise=alls=3:allf=t` temporal uniform or libplacebo dither) so residual shimmer is masked and encoder avoids banding.

---
## 4. Recommended architecture: tiered pipeline

### 4.1 Tiers
- **Tier 0 (always)**: FFmpeg CPU: `deblock -> hqdn3d -> scale lanczos -> cas`. Works everywhere, deterministic, fast. 
- **Tier 1 (any Vulkan GPU, preferred)**: libplacebo (EWA) and/or Anime4K shaders - no model download; real-time. Good "Quality-lite".
- **Tier 2 (Vulkan GPU, AI)**: ncnn-vulkan engine (Real-ESRGAN `realesr-general-x4v3` / `animevideov3`, Real-CUGAN for anime), blended with classical reference (strength slider), then final Lanczos downscale to exact target.
- **Tier 3 (optional plugin)**: NVIDIA Maxine VFX / TensorRT/ORT-CUDA with Compact/SPAN/RealPLKSR or VSR models when RTX present; ORT-OpenVINO for Intel; ORT-CPU "slow mode".
- Auto-detect: query Vulkan (`vulkaninfo --summary` or ncnn `get_gpu_count()`), reject `llvmpipe`/`lavapipe` (software), benchmark 5-10 frames on first run and cache result per GPU/driver to choose default tier and tile size; show ETA.

### 4.2 Integration approaches
| Approach | Pros | Cons |
|---|---|---|
| **A. Subprocess + raw frame pipes**: `ffmpeg -i in -vf <pre> -f rawvideo -pix_fmt rgb24 -` -> engine -> `ffmpeg -f rawvideo ... -i - <encode>` | Language-agnostic; license isolation (GPL/AGPL binaries separate process); crash isolation; easy per-engine swap. | Pipe copy overhead (480x854x3 ~1.2 MB/frame - trivial; 4x output 1080p-class ~6-9 MB/frame, fine up to ~100fps); audio/timestamps handled by 3rd ffmpeg mux step; need engine that supports stdin/stdout raw (realesrgan-ncnn-vulkan only does files/dirs, not stdin - you'd need a tiny custom wrapper). |
| **B. Image-sequence temp dir** (extract PNG/JPG -> engine -> encode) | Simplest (what realesrgan-ncnn-vulkan, Video2X v4/5 did) | Huge disk I/O (TBs for long clips), slow, bad UX; use only as fallback/for debugging. |
| **C. Link libs in-process** (libav* + ncnn C API, like Video2X 6 libvideo2x) | Zero copy, best throughput, tight progress/cancel control, GPU-resident frames possible | Build complexity cross-platform, LGPL/GPL linking implications of FFmpeg build, ncnn API drift [V]. |
Recommendation: **Write own thin engine runner executable ("upscale-worker")** using ncnn C++ API, speaking a simple protocol: reads raw frames from stdin (w,h header), writes upscaled raw frames to stdout, emits JSON progress lines on stderr. App (any language) orchestrates 2 ffmpeg processes + worker via pipes (approach A). This keeps ffmpeg (GPL/LGPL) and ncnn (BSD) separated, avoids AGPL Video2X, and allows swapping to an ORT-based worker with identical protocol.
- Keep A/V sync: pass through audio from original with `-map 1:a? -c:a copy` in final ffmpeg by reading source file twice (video from pipe, audio from original). Preserve frame timestamps: output CFR via `-r` equals source fps; for VFR sources, normalize with `fps` filter first (social videos are often VFR) or pass PTS via sidecar.
- Clipping: apply `-ss/-to` at the decode ffmpeg (input seeking, accurate with re-encode) so only the clip gets upscaled - big cost saving. Do trim before upscale, always.
- Portrait/crop: do crop/scale-to-cover in the decode stage so upscaler gets minimal pixels (e.g. crop 9:16 from 16:9 source before AI -> 3x fewer pixels).

### 4.3 Memory / tiling
- ncnn tile size: default auto by VRAM; typical 200-400 px for 4GB; `-t 0` auto. Tile padding 10-32 px. 4x output of 720p source = 5120x2880 intermediate: **avoid**; instead decide scale factor from source height so output >= target (e.g. 480x854 -> target 1080x1920 needs 2.25x -> run 2x model then lanczos 1.125, or 4x model on... ). Prefer models with native x2; otherwise run x4 and downscale tile-wise on GPU.
- VRAM budgeting: Real-ESRGAN compact fp16 ~0.5-1 GB w/ 256-tile; RRDB x4plus 2-4 GB. iGPU shares system RAM; set conservative tile (128) on iGPU / <4GB.
- Use fp16 where supported (`-x` TTA off). Avoid TTA (8x cost).
- Pipeline with bounded queues (e.g. 4-8 frames) between decode/worker/encode to prevent RAM blow-up; GPU upscaling is the bottleneck, so decode/encode should be hardware-light or threaded.
- Frame batching not needed; use multi-thread load/proc/save (`-j 1:2:2`).
- Handle VK_ERROR_DEVICE_LOST/OOM: retry with smaller tile, then fall back to next tier.

### 4.4 Progress reporting
- Total frames = `ffprobe -count_packets` or `duration*fps` (clip-aware). Worker emits `{"frame":n,"total":N,"fps":x}` per frame/N frames; app computes ETA smoothed (EMA). Also parse ffmpeg `-progress pipe:2` for encode stage. Cancel via SIGTERM / closing stdin; ensure child process-tree kill on Windows (Job Objects).
- Run a quick 10-frame preview/benchmark and a before/after single-frame preview (extract frame -> classical vs AI) before committing a long job.

---
## 5. Encoding output for quality

### 5.1 Codec choice
- Output target is phones: **H.264 High profile yuv420p** is universally compatible (social upload platforms re-encode anyway). **HEVC/H.265** (10-bit optional) gives ~25-40% bit savings but compatibility on upload to some platforms/old devices is mixed; **AV1** (SVT-AV1/hardware on RTX 40+/Arc/RDNA3+) is smaller still but compat varies. Default: H.264 for max compat; offer HEVC option.
- Because upscaled content has no genuine fine detail, it compresses surprisingly well at moderate bitrate, but GAN-hallucinated texture is expensive: noisy texture eats bits - another reason to blend/denoise.

### 5.2 Rate control
- **libx264**: `-preset slow -crf 17-19` (for upscaled 1080x1920 content, 18 is a good default; 20-22 acceptable for small files), `-tune film` (or `-tune animation` for anime), `-profile:v high -pix_fmt yuv420p -movflags +faststart`. `-aq-mode 3` helps flat regions/banding. If output intended for further social upload, go CRF 16-18 so the platform re-encode starts from a high-quality source.
- **libx265**: `-preset medium -crf 20-22` (x265 CRF ≈ x264 CRF+4 for similar quality), `-tag:v hvc1` for Apple compatibility, `-x265-params aq-mode=3:no-sao=0`; 10-bit `-pix_fmt yuv420p10le` reduces banding even from 8-bit sources (dither at conversion) at a ~small bitrate cost; hardware support for HEVC 10-bit decode is universal on modern phones.
- **libsvtav1**: `-crf 28-32 -preset 6`.
- **Hardware**:
  - NVENC (`h264_nvenc` / `hevc_nvenc` / `av1_nvenc`): `-preset p5 -tune hq -rc vbr -cq 19 -b:v 0 -spatial-aq 1 -temporal-aq 1 -rc-lookahead 32`; quality now close to x264 medium on RTX 30+/Ada (Turing+ much improved over Pascal) but still less efficient per bit than x264 slow.
  - QSV (`h264_qsv`, `hevc_qsv`, `av1_qsv`): `-preset veryslow -global_quality 20 -look_ahead 1`; quality acceptable on Arc/Gen11+; Windows+Linux (needs oneVPL/libvpl runtime).
  - AMF (`h264_amf`, `hevc_amf`): Windows (and Linux via AMDGPU-PRO); `-quality quality -rc cqp/vbr_peak -qp_i 20 -qp_p 22`; historically weakest quality-per-bit, improved on RDNA3/4; use higher bitrate.
  - VAAPI (`h264_vaapi`, `hevc_vaapi`): Linux AMD (Mesa)/Intel; `-qp 20` or `-rc_mode ICQ -global_quality 20` (Intel); AMD VAAPI quality fair.
  - Vulkan video encode (`h264_vulkan`, `hevc_vulkan`, FFmpeg 7.1+) - emerging, not production-ready; skip.
  - **Default policy**: software x264/x265 for "Best quality" (CPU is idle anyway while the GPU does AI; but AI + x264 slow compete for CPU on small machines -> use `-preset medium` and cap threads); hardware encoder for "Fast" mode or when encode is the bottleneck (classical tier at hundreds of fps).
- Single-pass CRF is fine for clips; two-pass only if a target size is needed (e.g. upload limits - compute bitrate from duration).

### 5.3 Avoiding re-compression artifacts
- Do all processing in one decode -> filter -> encode chain; never encode an intermediate lossy file. If intermediates are required (e.g. Tier 2 pipe), use raw RGB/`yuv420p`/`yuv444p` pipes or lossless FFV1/`x264 -qp 0`/ProRes/ UT Video; **do not use JPEG intermediates**.
- Prefer 16-bit or at least 10-bit intermediates when AI model supports (most produce 8-bit RGB; fine). Convert RGB->YUV with correct matrix (`-vf scale=out_color_matrix=bt709:out_range=tv`, `zscale=m=709:t=709:p=709`) and tag output (`-colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv`) to avoid color shifts; most phone-sourced video is BT.709 limited (Rec.601 for SD sources; convert).
- Limited vs full range: detect; HDR sources (HLG/PQ from iPhones) need tone-mapping to SDR (libplacebo `tonemapping=bt.2390`) or kept HDR with HEVC 10-bit - AI models are SDR-trained: tone map first.
- Output resolution: use even dimensions; target exact phone-native sizes (1080x1920, 1080x2400 (20:9), 1170x2532, 1290x2796, 1320x2868); letterbox/crop per option rather than stretch. H.264 level 4.2/5.1 for 1080x2400@60 (level 5.0 needed for >2.0M pixels at 60fps? check levels: 1080x2400 = 2.6 MP -> Level 5.1 for high fps; Level 4.2 fine at 30 fps up to ~2.1 MP; set `-level auto`).
- Frame rate: keep source fps (don't interpolate by default); no frame rate conversion unless RIFE is explicitly enabled.
- Deblocking at source stage and mild grain at output reduces visible "blockiness" masks.

---
## Recommendations

1. **Default engine/stack**: FFmpeg (own LGPL/GPL build bundled, with libplacebo+Vulkan, zimg, x264, x265, nvenc/qsv/amf/vaapi headers enabled) with a **tiered pipeline**.
2. **Tier 0 default (no GPU / always works)**: `deblock -> hqdn3d (light) -> scale lanczos (or zscale) -> cas 0.4 -> x264 slow CRF 18, yuv420p, bt709`. Ship it first; it is the safe fallback.
3. **Tier 1 (any Vulkan GPU, quick win)**: swap scale for libplacebo `ewa_lanczossharp` + deband; optionally Anime4K shaders when content is flagged anime. No model downloads.
4. **Tier 2 AI (Vulkan GPU, default when benchmark >= ~8 fps)**: own small ncnn worker (BSD-3 ncnn, MIT Real-ESRGAN code, BSD-3 models) with `realesr-general-x4v3` (live action) / `realesr-animevideov3` (anime) / Real-CUGAN (anime option), 2x effective (or x4 + downscale), tile auto, then blend 20-40% with Lanczos reference, light temporal denoise before and `hqdn3d` temporal-only after; final downscale to exact target. Integrate by **subprocess raw-frame pipes** (ffmpeg decode -> worker -> ffmpeg encode), with trim/crop done before upscale and JSON progress on stderr.
5. **Avoid as dependencies**: Video2X (AGPL, build fragility, only reuse as design reference), FFmpeg dnn_processing/sr (poor quality/ergonomics), PyTorch-based SwinIR/HAT/BasicVSR++ (too heavy), RIFE (out of scope v1; optional later - check model license first), vendor playback features (RTX VSR/AMD/Intel driver upscaling have no offline API).
6. **Optional later plugins**: NVIDIA Maxine VFX SDK / TensorRT or ORT (CUDA/DirectML/OpenVINO) for faster/higher-quality models (Compact/SPAN/RealPLKSR from OpenModelDB - verify per-model licenses, many are CC-BY-NC); ORT-CPU slow mode for GPU-less users who accept long runtimes.
7. **Encoding default**: libx264 `-preset slow -crf 18 -tune film` (or `animation`), High profile, yuv420p, `+faststart`, bt709 tags; HEVC 10-bit as an option; use NVENC/QSV/AMF/VAAPI only for a "Fast" preset (cq/qp ~19-20, quality preset slowest). Never use intermediate lossy files.
8. **First-run**: detect Vulkan GPUs (exclude software lavapipe), run a ~10-frame benchmark, cache result, choose tier and tile size, display ETA; allow user override. Test matrix: NVIDIA (Win/Linux), AMD (Win; Linux Mesa RADV), Intel Arc/iGPU, and CPU-only (dev machine) - note dev machine lacks NVIDIA so NVENC/Maxine/CUDA paths need CI/remote testing.
9. **Open items to verify before implementation**: current FFmpeg release option names for libplacebo; realesrgan-ncnn-vulkan release status/maintenance in 2026 and available `-s 2` models; Real-CUGAN and RIFE model licenses; DirectML vs Windows ML status; Maxine SDK redistribution terms; whether FFmpeg 8.x added ONNX/torch DNN improvements.

## Measured on the bundled engine (FFmpeg n9.0.2, 2026-10-06)
Machine: Intel Raptor Lake-P, UHD iGPU (Mesa ANV Vulkan), 12 threads. Source: 480x854 H.264 CRF 35,
600 frames (20 s @ 30 fps), upscaled to 1080x1920, filters only (`-f null`):

| Chain | Wall time | Throughput |
|---|---|---|
| `libplacebo=w=1080:h=1920:upscaler=ewa_lanczossharp:format=yuv420p` (Vulkan, `-init_hw_device vulkan`) | 8.7 s | ~69 fps |
| `hqdn3d=1.5:1.5:6:6,zscale=…:filter=spline36,cas=0.5` (CPU) | 6.0 s | ~100 fps |
| `nlmeans=s=2,zscale…,cas` (CPU) | 107 s | ~5.6 fps — too slow as a default |

- [V] libplacebo works on the Intel iGPU with software frames in/out (no explicit hwupload needed;
  Mesa prints a harmless `FINISHME` warning on stderr). With libx265 `-preset fast` encoding, both
  chains take ~3 s for 5 s of video: **the encoder, not the scaler, dominates**.
- Consequence: Tier 0 vs Tier 1 is a quality choice, not a speed one, on iGPU-class hardware.
  nlmeans is only viable on GPU (`nlmeans_vulkan`, present in the build) or as an opt-in.
