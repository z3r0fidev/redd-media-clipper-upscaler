# 03 - Mobile Display Targets for Portrait Video Output

Research date: 2026-10-06. Project: reddit_media video upscaler.
Confidence tags: [V] verified against vendor/major-outlet page this session; [S] from secondary spec aggregators (smartprix/phonearena/nanoreview) - spot-check before hard-coding; [M] from author knowledge, not re-fetched this session; [R] rumor/unconfirmed.

Caveat: several 2026 devices (iPhone 18 Pro, iPhone Duo, Pixel 11) were found only through search snippets and aggregators in this session. Treat 2026 rows as [S] unless noted.

---

## 1. Device landscape

### 1.1 What "Duo" / Air / Fold actually are (as of 2026-10-06)
- **iPhone Air**: real, shipped. Announced 2025-09-09, released 2025-09-19. 6.5" 1260x2736, 460 ppi, 120 Hz ProMotion. [V via TechCrunch/Apple comparison, S for numbers]
- **"iPhone 17 Air"** is a pre-launch rumor name only. The product is just "iPhone Air".
- **iPhone Duo** = Apple's first foldable (the rumored "iPhone Fold"). Announced 2026-09-09 with iPhone 18 Pro / 18 Pro Max. Pre-orders 2026-10-16, on sale 2026-10-23 (some regions 10-30). $1,999 (256GB). A20 Pro, Touch ID. [V: MacRumors roundup; Axios/WHRO confirm name + 5.4"/7.6"]
  - Outer: 5.4", 1398x2034, 460 ppi. Inner: 7.6", 1878x2670, 430 ppi. Both ~1:1.45 (about 3:4.4, i.e. 0.687 w/h), 120 Hz OLED. [V MacRumors; same ratio for both]
  - NOTE: the Duo's outer screen is NOT a tall 19.5:9 phone screen; it is a squat ~2:3 ratio. Portrait 9:16 video will be pillar/letter-boxed or heavily cropped there. Inner is also near-2:3 portrait (taller than wide), unlike Samsung/Pixel near-square inners.
  - Developer coverage mentions odd viewport/scale quirks (a "1.42 problem") - single blog source [R/S], ignore for video.
- **iPhone 18 lineup**: iPhone 18 Pro and 18 Pro Max announced 2026-09-09, released 2026-09-18 [S]. Pro Max listed as 1320x2868, 460 ppi, 120 Hz, 6.9" - identical to 17 Pro Max [S]. Pro presumed 1206x2622 (same as 17 Pro) [S/M]. Base iPhone 18 / iPhone 18e / Air 2: widely reported (pre-event) to be shifted to spring 2027 [R - not verified this session; confirm on apple.com].
- **iPhone 16e** (Feb 2025): 6.1" 1170x2532, 460 ppi, 60 Hz. [M]
- **"iPhone Fold"** as a product name does not exist; Apple chose "Duo".

### 1.2 Device table (portrait w x h)
Aspect shown as h/w ratio. "HDR" column: DV = Dolby Vision, H10 = HDR10, H10+ = HDR10+, HLG. Apple devices: DV/HDR10/HLG (no HDR10+). Samsung: HDR10+ (no Dolby Vision playback in gallery for most; S-series gallery plays HDR10+, some DV). Pixel: HDR10/HDR10+ (Pixel 8+ also Ultra HDR photos; Pixel video HDR10+ capture/playback; DV decode on Pixel 10 [M/R]).

#### Apple
| Model | Release | Native px (WxH) | h/w | PPI | Hz | HDR | Src |
|---|---|---|---|---|---|---|---|
| iPhone 15 / 15 Pro | 2023-09-22 | 1179x2556 | 2.168 | 460 (15 Pro: 460) | 60 / 120 | DV/H10/HLG | M |
| iPhone 15 Plus / Pro Max | 2023-09-22 | 1290x2796 | 2.167 | 460 | 60 / 120 | DV/H10/HLG | M |
| iPhone 16 | 2024-09-20 | 1179x2556 | 2.168 | 460 | 60 | DV/H10/HLG | M |
| iPhone 16 Plus | 2024-09-20 | 1290x2796 | 2.167 | 460 | 60 | same | M |
| iPhone 16e | 2025-02-28 | 1170x2532 | 2.164 | 460 | 60 | same | M |
| iPhone 16 Pro | 2024-09-20 | 1206x2622 | 2.174 | 460 | 120 | same | M |
| iPhone 16 Pro Max | 2024-09-20 | 1320x2868 | 2.173 | 460 | 120 | same | M |
| iPhone 17 | 2025-09-19 | 1206x2622 | 2.174 | 460 | 120 | same | S |
| iPhone Air | 2025-09-19 | 1260x2736 | 2.171 | 460 | 120 | same | V/S |
| iPhone 17 Pro | 2025-09-19 | 1206x2622 | 2.174 | 460 | 120 | same | S |
| iPhone 17 Pro Max | 2025-09-19 | 1320x2868 | 2.173 | 460 | 120 | same | V/S |
| iPhone 18 Pro | 2026-09-18 | 1206x2622 (assumed) | 2.174 | 460 | 120 | same | S/M |
| iPhone 18 Pro Max | 2026-09-18 | 1320x2868 | 2.173 | 460 | 120 | same | S |
| iPhone Duo outer | 2026-10-23 | 1398x2034 | 1.455 | 460 | 120 | same (assumed) | V |
| iPhone Duo inner | 2026-10-23 | 1878x2670 | 1.422 | 430 | 120 | same (assumed) | V |

Points are logical x3 (393x852, 402x874, 430x932, 440x956, 420x912 for Air). All "19.5:9"-class (2.164-2.174).

#### Google Pixel
| Model | Release | Native px | h/w | PPI | Hz | HDR | Src |
|---|---|---|---|---|---|---|---|
| Pixel Fold (outer / inner) | 2023-06 | 1080x2092 / 1840x2208 | 1.94 / 1.20 | 408 / 380 | 120 | H10+ | M |
| Pixel 8 | 2023-10 | 1080x2400 | 2.222 | 428 | 120 | H10+ | M |
| Pixel 8 Pro | 2023-10 | 1344x2992 | 2.226 | 489 | 120 LTPO | H10+ | M |
| Pixel 9 | 2024-08 | 1080x2424 | 2.244 | 422 | 120 | H10+ | M |
| Pixel 9 Pro | 2024-09 | 1280x2856 | 2.231 | 495 | 120 LTPO | H10+ | M |
| Pixel 9 Pro XL | 2024-08 | 1344x2992 | 2.226 | 486 | 120 LTPO | H10+ | M |
| Pixel 9 Pro Fold (outer / inner) | 2024-09 | 1080x2424 / 2076x2152 | 2.24 / 1.04 | 422 / 373 | 120 | H10+ | M |
| Pixel 10 | 2025-08-28 | 1080x2424 | 2.244 | 422 | 60-120 | H10+ | M/S |
| Pixel 10 Pro | 2025-08-28 | 1280x2856 | 2.231 | 495 | 1-120 | H10+ | M/S |
| Pixel 10 Pro XL | 2025-08-28 | 1344x2992 | 2.226 | 486 | 1-120 | H10+ | S (nanoreview/oled-info) |
| Pixel 10 Pro Fold (outer / inner) | 2025-10 | 1080x2364 / 2076x2152 | 2.19 / 1.04 | 408 / 373 | 120 | H10+ | S |
| Pixel 11 Pro XL | announced 2026-08-12, avail 2026-08-20 | 1344x2992 | 2.226 | ~486 | 1-120 | H10+, 3600 nits | S (single snippet; verify) |

#### Samsung Galaxy S
| Model | Release | Native px | h/w | PPI | Hz | HDR |
|---|---|---|---|---|---|---|
| S21 / S21+ | 2021-01/04 | 1080x2400 | 2.222 | 421 / 394 | 48-120 | H10+ |
| S21 Ultra | 2021 | 1440x3200 (often run at 1080p) | 2.222 | 515 | 10-120 | H10+ |
| S22 / S22+ | 2022-02 | 1080x2340 | 2.167 | 425 / 393 | 10-120 | H10+ |
| S22 Ultra | 2022-02 | 1440x3088 | 2.144 | 500 | 1-120 | H10+ |
| S23 / S23+ | 2023-02 | 1080x2340 | 2.167 | 425 / 393 | 48-120 | H10+ |
| S23 Ultra | 2023-02 | 1440x3088 | 2.144 | 500 | 1-120 | H10+ |
| S24 | 2024-01 | 1080x2340 | 2.167 | 416 | 1-120 | H10+ |
| S24+ / S24 Ultra | 2024-01 | 1440x3120 | 2.167 | 513 / 505 | 1-120 | H10+ |
| S25 | 2025-02 | 1080x2340 | 2.167 | 416 | 1-120 | H10+ |
| S25+ / S25 Ultra | 2025-02 | 1440x3120 | 2.167 | 513 / 498 | 1-120 | H10+ |
| S26 | 2026-03 | 1080x2340 | 2.167 | ~416 | 1-120 | H10+ |
| S26+ | 2026-03 | 1440x3120 | 2.167 | ~513 | 1-120 | H10+ |
| S26 Ultra | 2026-03-11 | 1440x3120 (6.9", 498 ppi) | 2.167 | 498 | 1-120 | H10+ |
Sources: [M] for S21-S25; S26 Ultra [S via phonearena/smartprix]. S26/S26+ values inferred from S25 generation [R]. Samsung default user setting is FHD+ even on QHD-capable models; the Gallery/player renders to the logical display resolution, so 1080-wide video is ample.

#### Samsung Foldables (portrait orientation: W x H)
| Model | Release | Cover px | Inner px | Inner h/w | Hz |
|---|---|---|---|---|---|
| Z Fold3 | 2021-08 | 832x2268 (6.2") | 1768x2208 | 1.25 | 120 / 48-120 |
| Z Fold4 | 2022-08 | 904x2316 | 1812x2176 | 1.20 | 120 |
| Z Fold5 | 2023-08 | 904x2316 | 1812x2176 | 1.20 | 120 |
| Z Fold6 | 2024-07 | 968x2376 | 1856x2160 | 1.16 | 120 |
| Z Fold7 | 2025-07-25 | 1080x2520 (6.5") | 1968x2184 (spec lists 2184x1968) | 1.11 | 120 |
| Z Flip3 / Flip4 | 2021 / 2022 | cover 260x512 / 260x512 | 1080x2640 (6.7") | 2.44 | 120 |
| Z Flip5 / Flip6 | 2023 / 2024 | cover 720x748 (3.4") | 1080x2640 | 2.44 | 120 |
| Z Flip7 | 2025-07-25 | cover 1048x948 (4.1") | 1080x2520 (6.9", 397 ppi) | 2.33 | 120 |
Sources: Fold7/Flip7 [S: smartprix/phonearena]; others [M]. Fold8 / Flip8 (expected ~July 2026) NOT verified this session [R] - check before adding.

#### Other (representative, [M])
| Model | Native px | h/w | Note |
|---|---|---|---|
| OnePlus 12 / 13 | 1440x3168 / 1440x3168 | 2.2 | 120 Hz LTPO, DV on OnePlus |
| Xiaomi 14 / 15 | 1200x2670 / 1200x2670 | 2.225 | DV, H10+ |
| Xiaomi 14/15 Ultra | 1440x3200 | 2.222 | |
| Nothing Phone (2)/(3) | 1080x2412 / 1260x2800 | 2.23 / 2.22 | |
| Motorola Razr 40/50/60 (inner) | 1080x2640 / 1080x2640 | 2.44 | cover small |
| Pixel 6/7 | 1080x2400 | 2.222 | 90/120 Hz |
| Typical budget Android | 720x1600, 1080x2400 | 2.22 | |

---

## 2. Clustering into presets

### 2.1 Aspect clusters (h/w)
| Cluster | h/w | Devices |
|---|---|---|
| A. Apple/Samsung "19.5:9" | 2.164-2.174 (~2.167) | all iPhone 15-18 non-fold; S22-S26 base/Plus/Ultra(2.14-2.17) |
| B. Pixel/older Samsung "20:9" | 2.222-2.244 | Pixel 8/9/10 family, S21, Xiaomi, OnePlus |
| C. Flip inner "22:9" | 2.33-2.44 | Z Flip3-7 inner, Razr |
| D. Social/legacy 16:9 | 1.778 | TikTok/Reels/Shorts canonical |
| E. Fold cover (tall-ish) | 2.2-2.6 | Fold3-7 cover (Fold4/5 cover 2.56) |
| F. Fold inner ~square | 1.04-1.25 | Pixel 9/10 Fold inner (1.04), Fold7 inner (1.11), Fold3-6 inner (1.16-1.25) |
| G. Apple Duo | 1.42-1.455 | iPhone Duo inner + outer |

Clusters A and B differ by only ~3% (2.167 vs 2.222). Cropping a 2.19 video to either costs <=1.5% each side; aspect-fit leaves slivers. Hence a single ~2.19:1 (1080x2364) or 2.167 (1080x2340) preset serves both A and B with negligible visible bars.

### 2.2 Resolution tiers
| Preset id | WxH | h/w | Matches natively | Use |
|---|---|---|---|---|
| `p1080-19.5` | 1080x2340 | 2.167 | S22-S26 base, S26+/Ultra at FHD+, iPhones ~(scale 1179/1080=1.09 upscale on device) | **DEFAULT** |
| `p1080-20` | 1080x2400 | 2.222 | Pixel 8/9/10 base-ish, S21 | Android-oriented |
| `p1170-ios` | 1170x2532 (~1179x2556) | 2.164 | iPhone 12-16 base/16e | iOS native |
| `p1206-ios` | 1206x2622 | 2.174 | iPhone 16/17/18 Pro, 17 | iOS Pro native |
| `p1290-ios` | 1290x2796 | 2.167 | 15 Plus/Pro Max, 16 Plus | |
| `p1320-ios` | 1320x2868 | 2.173 | 16/17/18 Pro Max | max Apple |
| `p1344-pixel` | 1344x2992 | 2.226 | Pixel 8/9/10/11 Pro (XL) | |
| `p1440-qhd` | 1440x3120 | 2.167 | S24+/Ultra, S25+, S26+/Ultra | QHD |
| `social-916` | 1080x1920 | 1.778 | TikTok/Reels/Shorts | social |
| `fold-inner` | 1920x2160 or 1080x1350 (4:5) | ~1.1-1.25 | Fold/Pixel Fold inner | optional |
| `duo` | 1080x1566 (1.45) | ~1.45 | iPhone Duo | optional, [R: no player-behavior data yet] |

All dimensions even; heights divisible by 2 (H.264 4:2:0 requirement); 16-alignment not required but 2340 -> 147 MB rows (coded height 2352, cropped via SPS) - fine.

### 2.3 Exact-native vs universal presets
Recommendation: **do not encode per-device exact native**. Reasons:
- Players resample at decode/composite time with high-quality GPU scaling; a 1080-wide source on a 1179-wide iPhone is a 9% upscale - visually negligible vs source softness (upscaled Reddit content rarely has detail beyond 1080p anyway).
- Per-device matrix (15+ sizes) multiplies encode cost and storage; clusters A/B differ by 3% aspect.
- Because the app is an upscaler, effective detail ceiling is the model output; encoding beyond 1440x3120 buys nothing on phones.
- Offer exact-native only as an "advanced / device picker" feature that maps device -> nearest preset and, if wanted, emits exact WxH. Implement as data table (JSON) so adding Duo/Pixel 12 is trivial.
Tiering: `1080` tier (default), `1440` tier (Ultra/Pro Max, ~ +2x bitrate), optional `iOS-native` tier for Apple users who view 1:1 on 460 ppi (marginal gain).

### 2.4 Player scaling / crop behavior
Evidence level: author knowledge [M]; no primary doc found for each app - verify with device tests.
- **iOS Photos / Camera Roll / Files (AVPlayer, AVLayerVideoGravity)**: Photos viewer uses aspect-fit inside the screen minus chrome; tapping toggles chrome (full-bleed dark/black background). A 9:16 video on 19.5:9 shows letterbox bars top/bottom (~18%); a video matching ~2.17 fills edge-to-edge in the safe-area-agnostic full-screen state (still avoids notch/Dynamic Island overlap only through UI chrome, content is not cropped). Pinch-to-zoom/double-tap can fill; no auto-fill. Camera captures at 9:16 / 16:9 so Photos is optimized for those. Custom apps default `.resizeAspect`.
- **Google Photos / Samsung Gallery / Android MediaPlayer/ExoPlayer**: aspect-fit by default (RESIZE_MODE_FIT); black bars on mismatched ratio. Samsung Gallery has a "fit to screen / full screen" toggle in video player for some versions; Google Photos has pinch-to-zoom only. ExoPlayer/Media3 `AspectRatioFrameLayout` supports FIT, FILL (stretch), ZOOM (crop).
- **TikTok / Instagram Reels / YouTube Shorts**: feed is full-bleed vertical; players use aspect-fill (cover) for 9:16-ish content. A 9:16 video fills most 19.5:9 screens, with UI overlays (caption, buttons) covering lower ~20% and right ~15%, top ~10%; safe zone ~ center 1080x1420 guidance. Landscape/square content is fit with bars (TikTok/Reels) or shown in a letterboxed frame (Shorts). Wider-than-9:16 vertical (e.g. 2.17) is typically cropped on the sides (cover) or fit - platform-dependent and subject to change [R]. Instagram feed/profile grid crops to 3:4 / 4:5 center (changed 2025) [M].
- Takeaway: a 2.167/2.19 preset is *right* for local playback (Photos/Gallery/VLC/etc.) and a 9:16 preset is *right* for social upload. They are different goals - expose both.
- Safe area: keep faces/captions out of the top ~8% (status bar/Dynamic Island) and bottom ~15% when targeting local full-screen playback with UI chrome overlay; social safe zone larger.

---

## 3. Reframing strategies for landscape/odd-aspect sources -> portrait

Example: a 16:9 source (w/h 1.778) filled into 1080x2340 (w/h 0.4615) keeps only 0.4615/1.778 = 26% of source width (74% cropped). That loss is why strategy choice matters.

| Strategy | How | Pros | Cons |
|---|---|---|---|
| Center crop | `crop=ih*W/H:ih` then scale | simple, deterministic, full-bleed | drops ~74% of 16:9 width; cuts off subjects, text |
| Smart/saliency reframing | Google AutoFlip (MediaPipe), face/person detection (YOLO, MediaPipe, RetinaFace), saliency, tracking + smoothed camera path | keeps subject, pro look | heavy deps, jitter if unsmoothed, wrong on multi-subject/text-heavy, needs scene-cut detection (PySceneDetect) |
| ffmpeg `cropdetect` | detect black bars/letterbox, then crop | removes baked-in bars (many Reddit videos are letterboxed or have bars from vertical-in-landscape) - essential pre-step | only detects borders, not subject |
| Blurred-background pad | scale source to fit width, blurred zoomed copy behind (`split`, `boxblur`/`gblur`, overlay) | no information loss, fills screen, familiar TikTok style | small subject on large canvas (16:9 in 2.167 = 26% of height); wastes upscaled detail; blur cost; bitrate spent on blur |
| Scale-to-fill (stretch) | non-uniform scale | none | distortion; reject |
| Aspect-fit + black bars | | lossless | not "fill"; violates requirement |
| Mixed/auto | choose per-source by rule | best UX | needs heuristics |
| Generative outpaint | AI fill | full-bleed no crop | expensive/artifacts; out of scope [R] |

Decision heuristics (default fill strategy):
1. Run cropdetect (sample 3-5 frames; `cropdetect=24:16:0`) and strip static bars first.
2. Compute source aspect after trim `a = w/h`:
   - a <= ~0.50 (already taller than 2:1): scale-to-fill with <=3% crop (near-lossless).
   - 0.50 < a <= 0.75 (9:16 to 3:4): scale-to-fill center/saliency crop (9:16 into 2.167 crops ~18% of width).
   - 0.75 < a <= 1.0 (square-ish): blurred background with foreground fit width, or saliency crop if single subject.
   - a > 1.0 (landscape): blurred-background pad by default (preserve content); offer "Smart crop" (saliency) as toggle when a face/single subject is detected with confidence.
3. Always upscale AFTER choosing crop region (crop first, so GPU spends pixels on kept area) - or upscale then crop if model needs context; test both.
4. Smooth the crop path (Kalman / EMA, max pan speed), lock crop within shots, reset at scene cuts.
Example ffmpeg (blurred bg, 1080x2340):
```
ffmpeg -i in.mp4 -filter_complex \
"[0:v]split=2[bg][fg];[bg]scale=1080:2340:force_original_aspect_ratio=increase,crop=1080:2340,gblur=sigma=30[b];\
[fg]scale=1080:-2[f];[b][f]overlay=(W-w)/2:(H-h)/2,format=yuv420p" ...
```
Example center crop to fill: `-vf "scale=1080:2340:force_original_aspect_ratio=increase,crop=1080:2340"`.

---

## 4. Codec / container compatibility

### 4.1 Containers
- **MP4 (ISO BMFF) with `-movflags +faststart`** (moov at front) - universal on iOS (Photos, Safari, Files) and Android. Prefer MP4 over MKV/WebM for sharing; iOS cannot play MKV/WebM-in-Photos.
- MOV acceptable on iOS; use MP4 for cross-platform.
- HEVC in MP4 **must be tagged `hvc1`** (`-tag:v hvc1`) for Apple; `hev1` fails or won't play in QuickTime/Photos [M, well-established].

### 4.2 Video codecs
| Codec | iOS | Android | Note |
|---|---|---|---|
| H.264 High, yuv420p 8-bit | all | all | safest default; use High (not High 4:4:4/10-bit). Baseline unnecessary. |
| HEVC Main / Main10 (hvc1) | HW all A10+ (iPhone 7+) | HW on virtually all Android 5+ SoCs since ~2016 (Pixel 2+, Snapdragon 8xx, Exynos) | ~40% smaller; Main10 for HDR |
| AV1 | HW decode: iPhone 15 Pro (A17 Pro) and later; base iPhone 16+; SW (dav1d) on Safari/Mac for earlier. Photos app support requires HW (M) | HW decode: Pixel 8+ (Tensor G3), Snapdragon 8 Gen 2+ (S23+), Dimensity 9200+, Exynos 2200+ (S22 intl) (M); SW dav1d via system fallback on older | encode cost very high (SVT-AV1 / libaom); avoid as default; offer opt-in "Small file" |
| VP9 | Safari partial; no Photos | yes | skip |
| HEVC 10-bit HDR (HLG/PQ + DV profile 8.4/8.1) | iOS plays HLG/HDR10/DV | HDR10/HDR10+ on most; DV only on some | only if source/upscaler produces HDR; default SDR |

### 4.3 Level/profile limits (compute from frame size x fps)
H.264 macroblocks: ceil(W/16)*ceil(H/16).
- 1080x1920: 68*120 = 8160 MB. L4.2 max frame 8704 MB, max 522,240 MB/s -> 60 fps = 489,600 OK. **L4.2 suffices** for 1080x1920@60.
- 1080x2340: 68*147 = 9,996 MB > 8704 -> **needs L5.0+**; at 30 fps 299,880 MB/s (L5.0 limit 589,824 OK); at 60 fps 599,760 > 589,824 -> **L5.1 (983,040) or 5.2 (2,073,600)**.
- 1080x2400: 68*150 = 10,200 MB; same as above (L5.1 for 60 fps).
- 1440x3120: 90*195 = 17,550 MB; 30 fps = 526,500 (L5.0 ok), 60 fps = 1,053,000 -> L5.2.
- iPhones decode H.264 High up to L5.2 (4K60) [M]. Android decoders: Pixel/Galaxy decode L5.1/5.2 (4K60) on flagships; very old/budget may cap at L4.1-4.2 - a rationale for HEVC or limiting default to 1080x2340@30.
HEVC levels (CTB-based, simpler): L5.0 max luma PS 8,912,896 samples (1440x3120 = 4.49M OK); 60 fps fine at L5.1; 120 fps at L5.2. Set `-level 5.1` default (Main profile High tier optional). Use `-tag:v hvc1`.
Dimensions: some hardware decoders choke on odd cropped heights; keep W,H multiples of 2 (ideally 16 for H.264 coded size; coded 2352 with SPS crop is standard).

### 4.4 Frame rates
- Source typically 24/25/30/60. Preserve source fps (VFR -> CFR via `-fps_mode cfr` / `-r`), cap at 60. **Do not frame-interpolate by default.** 120 Hz displays play 30/60 fine (ProMotion/VRR matches; LTPO Androids may use 24/30/48/60). 120 fps content is rare and costly; optional only if RIFE etc. is in scope.
- Avoid odd rates like 29.97 mismatching; keep 30000/1001 if source is that. Always CFR for compatibility.

### 4.5 Bitrate guidance (phone-sized, upscaled content)
Upscaled content (AI SR) adds texture/grain; pure H.264 at low rates smears it. Starting points [M, tune with VMAF/SSIM on samples]:
| Preset | H.264 (x264) | HEVC (x265) | AV1 (SVT) |
|---|---|---|---|
| 1080x1920 @30 | CRF 20 / ~6-8 Mbps cap | CRF 24 / ~4-6 Mbps | CRF 32 / ~3-4 Mbps |
| 1080x2340 @30 | CRF 20 / ~7-9 Mbps | CRF 24 / ~5-7 Mbps | CRF 32 |
| 1080x2340 @60 | CRF 21 / ~10-14 Mbps | CRF 25 / ~7-10 Mbps | CRF 33 |
| 1440x3120 @30 | CRF 20 / ~12-16 Mbps | CRF 24 / ~8-11 Mbps | CRF 32 |
Use `-maxrate`/`-bufsize` (2x) with CRF for compatibility; `-preset slow` (x264) / `medium` (x265); `-pix_fmt yuv420p`; GOP ~2 s (`-g 60`), `-bf 2`; `-profile:v high` for H.264, `-profile:v main` for HEVC SDR (`main10` + `-pix_fmt yuv420p10le` for HDR). Hardware encoders (VideoToolbox/NVENC): use `-b:v` target ~1.3x above; quality lower per bit than x264/x265 - fine for previews.
- Audio: AAC-LC, 48 kHz, stereo, 128-192 kbps (`-c:a aac -b:a 160k`); copy source AAC when compatible. Opus-in-MP4 is not universally supported; avoid.

### 4.6 Color metadata (SDR default)
- Tag explicitly: `-colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv` plus `-vf ...,format=yuv420p` and x264 `-x264-params colorprim=bt709:transfer=bt709:colormatrix=bt709`. Missing tags cause iOS/Android to guess (BT.601 vs 709 shift on SD sources). For SD source upscale (<720p BT.601) convert with `scale=...:out_color_matrix=bt709` and tag 709.
- Do not tag SDR as HDR. HDR (HLG/PQ BT.2020) only if source HDR or HDR-capable pipeline; then HEVC Main10, `colorprim=bt2020 transfer=smpte2084|arib-std-b67 colormatrix=bt2020nc`, plus mastering display / MaxCLL SEI for HDR10. Reddit-sourced content is overwhelmingly SDR - default SDR; pseudo-HDR inverse tone-mapping is a risk (blown highlights, banding) [R].
- Banding: 8-bit SDR from AI upscaling shows gradient banding; consider dithering (`-vf "gradfun"` or `format=yuv420p10le` -> HEVC Main10 even for SDR) as an optional "Quality" tier.
- Display: iPhone/Pixel/Galaxy panels P3/DCI-P3 capable but SDR video is interpreted BT.709 - fine.

---

## 5. Social platform specs (secondary targets) [M - verify against current platform help pages; limits change often]
| Platform | Aspect | Recommended px | Length | Codec/container | Other |
|---|---|---|---|---|---|
| TikTok | 9:16 (also 1:1, 16:9) | 1080x1920 | up to 10 min in-app record; uploads up to ~60 min | MP4/MOV, H.264 (HEVC accepted) | file cap hundreds of MB to ~4 GB by platform/device; fps 24-60; top ~130px / bottom ~480px / right ~120px UI overlap safe zone |
| Instagram Reels | 9:16 | 1080x1920 | up to 3 min (extended from 90s in Jan 2025) | MP4/MOV, H.264 + AAC | max 4 GB; fps >=23; bitrate cap re-encodes anyway; profile grid now shows 3:4 crop, so keep key subject centered (also 4:5 safe) |
| YouTube Shorts | 9:16 (also 1:1; vertical) | 1080x1920 | up to 3 min (since Oct 2024) | MP4, H.264 + AAC (YouTube accepts others) | Shorts defined by vertical/square + <=3 min; fps up to 60 |
All three recompress server-side; upload at 1080x1920, ~8-12 Mbps H.264 (higher bitrate survives re-encode better), keep AAC 128 kbps+. Ingest of 1440p+ vertical does not help (platforms downscale to ~1080 for Reels/TikTok) though Shorts may serve higher.

---

## Recommendations

### Presets
| ID | WxH | Codec | Profile/Level | Rate control | fps | Container |
|---|---|---|---|---|---|---|
| **`phone-default`** | **1080x2340 (2.167)** | **HEVC (x265)** | Main, L5.1, `hvc1` | CRF 24, maxrate 8M, bufsize 16M | source fps, cap 60 | MP4 +faststart |
| `phone-compat` | 1080x2340 | H.264 (x264) | High, L5.0 (30 fps) / L5.2 (60 fps) | CRF 20, maxrate 10M, bufsize 20M | source fps, cap 60 | MP4 +faststart |
| `android-tall` | 1080x2400 (2.222) | HEVC | Main, L5.1 | CRF 24 | source | MP4 |
| `ios-pro` | 1206x2622 (or 1320x2868 for Pro Max) | HEVC | Main, L5.1, hvc1 | CRF 23, maxrate 12M | source | MP4 |
| `qhd-ultra` | 1440x3120 | HEVC | Main, L5.1 | CRF 23, maxrate 14M | source | MP4 |
| `social-916` | 1080x1920 | H.264 | High, L4.2 | CRF 18, maxrate 12M | source, cap 60 | MP4 |
| `small-av1` (opt-in) | 1080x2340 | AV1 (SVT-AV1 preset 6) | Main | CRF 32 | source | MP4 |
| `fold-inner` (opt-in) | 1080x1350 (4:5) or 1920x2160 | HEVC | Main L5.1 | CRF 24 | source | MP4 |
| `duo` (opt-in, [R]) | 1080x1566 | HEVC | Main | CRF 24 | source | MP4 |

Common flags: `-pix_fmt yuv420p -colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv -movflags +faststart -tag:v hvc1 (HEVC) -c:a aac -b:a 160k -ar 48000 -ac 2 -g <2*fps>`.

### Defaults and policy
1. **Default preset**: `phone-default` (1080x2340, HEVC Main, hvc1) with automatic fallback to `phone-compat` (H.264 High) if a "max compatibility" toggle is on or the target device list includes pre-2016/low-end Android.
2. **Default fill strategy**: cropdetect to strip baked-in bars -> choose by source aspect (section 3): near-vertical sources scale-to-fill center/saliency crop; landscape/square sources get **blurred-background pad** by default; offer "Smart crop (AutoFlip-style, face/saliency tracked, smoothed)" as an option and plain center crop as a fast option. Never stretch; never leave black bars in the "fill" mode.
3. **Universal vs native**: ship universal presets above; a device picker maps phone -> nearest preset (data-driven table from section 1.2). Do not generate per-device exact-native encodes by default.
4. 1080x2340 is within ~3% aspect of every iPhone/Galaxy S and ~2.5% of Pixels; residual bars/crop are imperceptible. For Flip inner (2.33-2.44) the same preset leaves small side-crop under cover fit; acceptable.
5. FPS: preserve source CFR, cap 60, no interpolation by default. 120 fps not needed for phones.
6. SDR BT.709 by default; HDR only when source is HDR and the user opts in (HEVC Main10 + proper tags).
7. Test matrix: verify in iOS Photos, Google Photos, Samsung Gallery, VLC, TikTok/Reels/Shorts upload; confirm `hvc1`, level, `faststart` (`ffprobe -v error -show_entries stream=codec_tag_string,profile,level,pix_fmt:format=...`).

### Open items / to verify
- iPhone 18 Pro (non-Max) native resolution, iPhone 18/18e/Air 2 timing (spring 2027 [R]).
- Pixel 11 series full spec; Galaxy Z Fold8/Flip8 (mid-2026) specs; Samsung S26/S26+ exact panels.
- Platform specs (TikTok/IG/Shorts) and exact app crop/fit behavior need device tests; changes frequently.
- iPhone Duo video playback behavior (aspect-fit on ~1.42-1.45 screens) is unknown; ships 2026-10-23.

### Sources
- [MacRumors - iPhone Duo roundup](https://www.macrumors.com/roundup/iphone-duo/) (announce date, price, display res)
- [WHRO/Axios - Apple announces foldable iPhone](https://www.whro.org/2026-09-10/apple-announces-foldable-iphone-what-to-know)
- [TechCrunch - All iPhone 17 models compared](https://techcrunch.com/2025/09/10/all-of-the-iphone-17-models-compared/)
- [Apple - iPhone 17 Pro tech specs](https://www.apple.com/uk/iphone-17-pro/specs)
- [Use Your Loaf - iPhone 17 screen sizes](https://useyourloaf.com/blog/iphone-17-screen-sizes/)
- [NanoReview - iPhone 18 Pro Max](https://nanoreview.net/en/phone/apple-iphone-18-pro-max)
- [OLED-Info - Pixel 10 family](https://www.oled-info.com/google-pixel-10-pixel-10-pro-pixel-10-pro-xl-pixel-10-fold)
- [NanoReview - Pixel 10 Pro XL vs Pro Fold](https://nanoreview.net/en/phone-compare/google-pixel-10-pro-xl-vs-google-pixel-10-pro-fold)
- [Smartprix - Pixel 11 Pro XL](https://us.smartprix.com/mobiles/google-pixel-11-pro-xl-ppd1u7n9v5a1)
- [PhoneArena - Z Flip 7 vs S26 Ultra](https://www.phonearena.com/phones/compare/Samsung-Galaxy-Z-Flip-7,Samsung-Galaxy-S26-Ultra/phones/12646,12802)
- [PhoneArena - Z Fold 7 vs S26 Ultra](https://www.phonearena.com/phones/compare/Samsung-Galaxy-Z-Fold-7,Samsung-Galaxy-S26-Ultra/phones/12645,12802)
