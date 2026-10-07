# 06 - Device Targets, Player Behaviour, Encoding (verification of 03)

Date: 2026-10-06. Tags: [V] read on a vendor/major-outlet page this session (URL given); [S] secondary aggregator/search snippet; [R] rumour; [M] author knowledge, NOT verified; [U] could not verify.
Method note: Gemini CLI hit free-tier quota (429, 5 req/min) and returned nothing, so everything below came from direct web search/fetch. Several Apple pages were read through a summarizing fetcher; numbers quoted are exact strings from those pages but small extraction errors are possible (see conflicts).

## 0. Headline corrections to research/03
| # | 03 said | Now |
|---|---|---|
| 1 | iPhone 18 Pro 1206x2622 "assumed" [S/M] | [V] apple.com/iphone-18-pro/specs: 6.3" 2622x1206 @460 ppi; 18 Pro Max 6.9" 2868x1320 @460 ppi; ProMotion up to 120 Hz |
| 2 | Duo "assumed" HDR, dates [V MacRumors] | [V] apple.com/iphone-duo/specs: inner 7.6" 1878x2670 @430 ppi, outer 5.4" 1398x2034 @460 ppi, up to 120 Hz, HEVC/H.264/AV1 playback, DV/HDR10+/HDR10/HLG. Dates/price still only from 03's MacRumors cite (not re-fetched) |
| 3 | iPhone 17e not listed | [V] exists: apple.com/iphone/compare shows 17e 6.1" 2532x1170 @460 ppi |
| 4 | Base iPhone 18 / 18e / Air 2 "spring 2027 [R]" | Still [S]/[R]: absent from Apple compare page; multiple outlets (ithinkdiff, mezha, whathifi, jordannews) say spring 2027. No Apple confirmation. |
| 5 | Pixel 11 "single snippet" | [S] Pixel 11 family released 2026-08-20 (announced 08-12); Pixel 11 1080x2424, Pixel 11 Pro 1280x2856, Pro XL 1344x2992 (486 ppi), Pro Fold inner 2076x2152 (phonearena/nanoreview/smartprix snippets). Not on store.google.com this session. |
| 6 | Fold8/Flip8 "not verified" | [V] samsung.com/uk compare page lists Galaxy Z Fold8 (cover 5.5", main 7.6"), **Z Fold8 Ultra** (cover 6.5", main 8.0"), Z Flip8 (cover 4.1", main 6.9"); no pixel counts on that page. [S] sammobile/others: launched 2026-07-22; Fold8 inner 1828x2448 unfolded / cover 1248x1972 folded; Fold8 Ultra cover FHD+, inner QHD+; Flip8 main 1080x2520, cover 948x1048 [S]. NOTE cover is **1048x948 in 03 vs 948x1048 (w x h?) here**: orientation ambiguous, treat as ~1:1.1. |
| 7 | iPhone 16/15 base listed 60 Hz | CONFLICT: Apple compare-page extraction said every model "ProMotion up to 120Hz" incl. 15/16/16e. That is almost certainly summarizer error (15/16 base are 60 Hz panels; 16e/17e 60 Hz) [M]. Do not rely on Hz for fps decisions; output <=60 fps regardless. |
| 8 | Duo "unknown video behaviour" | Still unknown [U]. |
| 9 | Reddit "no data" | Partially resolved, see section 3; the main Reddit help article returned 403 so core limits are [S]. |

## 1. Verified native portrait resolutions (W x H px)
Aspect = H/W. HDR for all Apple rows: Dolby Vision, HDR10+/HDR10, HLG playback; codecs HEVC, H.264, AV1 playback [V: apple.com specs 16/17/Air/18 Pro/Duo; 18 Pro also ProRes].

### Apple
| Model | W x H | H/W | ppi | Hz | Tag / source | vs 03 |
|---|---|---|---|---|---|---|
| iPhone 15, 15 Pro | 1179x2556 | 2.168 | 460 | 60 / 120 | [V compare page: 15 = 2556x1179] 15 Pro [M] | same |
| 15 Plus, 15 Pro Max | 1290x2796 | 2.167 | 460 | 60 / 120 | [M] not re-fetched | same |
| 16, 16 Plus | 1179x2556 / 1290x2796 | 2.168 | 460 | 60 [M] | [V] 16 = "2556-by-1179" apple.com/iphone-16/specs; Plus [M] | same |
| 16e | 1170x2532 | 2.164 | 460 | 60 [M] | [V] apple.com/iphone/compare | same |
| 16 Pro / Pro Max | 1206x2622 / 1320x2868 | 2.174/2.173 | 460 | 120 | [M] (identical to 17/18 Pro panels) | same |
| 17 | 1206x2622 | 2.174 | 460 | up to 120 | [V] apple.com/iphone-17/specs (6.3", ProMotion) | upgraded S -> V |
| iPhone Air | 1260x2736 | 2.171 | 460 | up to 120 | [V] apple.com/iphone-air/specs | S -> V |
| 17e | 1170x2532 | 2.164 | 460 | (see conflict 7) | [V] compare page | NEW row |
| 17 Pro / Pro Max | 1206x2622 / 1320x2868 | 2.174/2.173 | 460 | 120 | [S/M] (same panels as 18 Pro) | same |
| 18 Pro | 1206x2622 | 2.174 | 460 | 120 | [V] apple.com/iphone-18-pro/specs | S/M -> V |
| 18 Pro Max | 1320x2868 | 2.173 | 460 | 120 | [V] same | S -> V |
| iPhone 18 / 18e / Air 2 | n/a | | | | [R] spring 2027, not announced | |
| Duo outer | 1398x2034 | 1.455 | 460 | 120 | [V] apple.com/iphone-duo/specs | S -> V |
| Duo inner | 1878x2670 | 1.422 | 430 | 120 | [V] same | S -> V |
Duo is the only Apple screen well outside 2.17; keep as a separate preset (section 2).

### Google Pixel (HDR: HDR10+ on all; 10-bit HDR video playback supported)
| Model | W x H | H/W | Tag / source |
|---|---|---|---|
| Pixel 8 | 1080x2400 | 2.222 | [M] |
| Pixel 8 Pro | 1344x2992 | 2.226 | [S] phonearena snippet (2992x1344, 120 Hz, HDR) |
| Pixel 9 | 1080x2424 | 2.244 | [M] |
| Pixel 9 Pro | 1280x2856 | 2.231 | [S] (494 ppi) |
| Pixel 9 Pro XL | 1344x2992 | 2.226 | [S] (487 ppi) |
| Pixel 9 Pro Fold outer / inner | 1080x2424 / 2076x2152 | 2.244 / 1.037 | [S] (422-425 / 374 ppi) |
| Pixel 10 / 10 Pro | 1080x2424 / 1280x2856 | 2.244 / 2.231 | [M] |
| **Pixel 10 Pro XL** (user's "10 XL Pro") | **1344x2992** | 2.226 | [S] CDW retailer listing "2992 x 1344, 1-120 Hz, 6.8 in Super Actua"; 20:9 [S] |
| Pixel 10 Pro Fold outer / inner | 1080x2364 / 2076x2152 | 2.189 / 1.037 | [V-ish] store.google.com/my/product/pixel_10_pro_fold_specs snippet: 6.4" 1080x2364 408 ppi; 8" 2076x2152 373 ppi; 60-120 Hz | 03 matched |
| Pixel 11 / 11 Pro / 11 Pro XL | 1080x2424 / 1280x2856 / 1344x2992 | 2.244/2.231/2.226 | [S] released 2026-08-20; Pro XL 120 Hz LTPO 1-120, 3600 nits |
| Pixel 11 Pro Fold inner / outer | 2076x2152 / (6.4") | | [S] outer px [U] |
All Pixel Pro-class panels ship defaulting to a scaled "FHD+/QHD+" setting; apps see logical size, so a 1080-wide file is not under-resolved. [M]

### Samsung (HDR10+; no Dolby Vision playback in stock gallery on most) [M]
| Model | W x H | H/W | Tag |
|---|---|---|---|
| S24, S25, S26 | 1080x2340 | 2.167 | [S] S25 "2340x1080" samsung.com/ie support comparison; S26 6.3" 1080x2340 retailer listing |
| S24+, S25+ | 1440x3120 | 2.167 | [S] samsung.com support pages (3120x1440) |
| S26+ | 1440x3120 | 2.167 | [S] retailer listing (dateks.lv), 6.7" LTPO |
| S24 Ultra, S25 Ultra | 1440x3120 | 2.167 | [S] (03 had S24 Ultra 3120 too) |
| S26 Ultra | 1440x3120 | 2.167 | [S] (03), not re-fetched |
| Z Fold6 cover / inner | 968x2376 / 1856x2160 | 2.45 / 1.16 | [M] |
| Z Fold7 cover / inner | 1080x2520 / 1968x2184 | 2.33 / 1.11 | [S] multiple sources agree (cover 6.5" 2520x1080, inner 8.0" 2184x1968 as listed landscape) |
| Z Fold8 / Fold8 Ultra | see section 0 row 6 | | [V] sizes, [S] pixels |
| Z Flip6 cover / main | 720x748 / 1080x2640 | 1.04 / 2.44 | [M] |
| Z Flip7 cover / main | 1048x948 / 1080x2520 | 1.1 / 2.33 | [S] |
| Z Flip8 cover / main | 948x1048(?) / 1080x2520 | / 2.33 | [S] 4.1" / 6.9" [V sizes on samsung.com/uk] |
Diffs from 03: S24 base is 1080x2340 [S ok]; Fold7/Flip7 main agree; Z Flip6 main 1080x2640 [M] unverified; no S26 base/+ value was changed.

## 2. Player behaviour and best "fill" aspect
Evidence is mostly forum/guide-level; there is no official doc stating how apps fit/crop. [U] for Reddit specifics.
| Player | Behaviour | Tag |
|---|---|---|
| iOS Photos (video) | Aspect-fit by default. Double-tap OR top-right button toggles zoom-to-fill ("magnifies so no black bars"; it is a scale, so it crops the overflow). No automatic fill for video on iOS <27. | [S] iphonelife.com/content/double-tap-videos-make-them-fill-screen (2021); Apple Community threads |
| iOS 27 Photos | New default-on "Zoom Photos to Fill" for photos "already too tall and nearly the same aspect as the screen"; article does NOT say it applies to videos. | [V] idownloadblog.com (iOS 27); video applicability [U] |
| Google Photos | Aspect-fit; pinch/zoom only [M]. | [M] |
| Samsung Gallery | Aspect-fit; some One UI builds have a fit/fill toggle [M]. | [M] |
| Reddit in-app | Full-screen player and Watch feed exist (Adweek/AlternativeTo coverage) and handle vertical and horizontal; Reddit ads support 1:1, 4:5, 16:9, recommend 4:5 for mobile. Exact cover-vs-fit for tall vertical: [U]. A Reddit Android bug (v2025.41.0, 2025-10) stretched videos in the feed player, so aspect handling is fragile. Reddit re-transcodes everything server-side. | [S] piunikaweb.com 2025-10-22; adweek.com/media/reddit-tests-full-screen-video-player |
| TikTok / Reels / Shorts | 9:16 is the canonical master (all three). Non-9:16: 1:1 and 16:9 are letterboxed (TikTok, Shorts) or cropped (Reels, 1:1). Behaviour for 9:19.5 (taller than 9:16) not documented; likely cropped or fit, [U]. | [S] teds.com.au 2026 guide |

Conclusion on aspect:
- There is no single aspect that fills every phone in stock viewers (2.167 vs 2.226 vs 2.33-2.44 vs Duo 1.42-1.46). 03's argument stands: 2.167-2.19 loses at most ~1.5% per side on either iPhone/Galaxy or Pixel when cover-zoomed.
- For Reddit/social destinations (primary): upload **9:16 1080x1920** (platforms master at it; Reddit re-encodes; unknown crop behaviour for taller). For local viewing in Photos/Gallery: 2.167 fills iPhones/Galaxy S with bars <1.5% on Pixels.
- Recommended preset set (supersedes 03 table where different):
| ID | WxH | Covers | Notes |
|---|---|---|---|
| **`reddit-916`** (default for upload) | 1080x1920 | Reddit, TikTok, Reels, Shorts; iPhone/Pixel/Galaxy via fit (bars ~18%) | H.264 or HEVC, L4.2 |
| **`phone-fill`** (default for local playback) | 1080x2340 | iPhone 15-18 non-Pro/Pro/Max, Galaxy S22-S26 all, Pixel 6-11 (with ~2.5% crop), Fold7/Flip7 cover/main approx | 2.167 |
| `phone-fill-hd` | 1440x3120 | S+/Ultra, Pixel Pro XL, 18 Pro Max, OnePlus | only if source detail justifies it |
| `android-tall` | 1080x2400 | Pixel 8/9/10/11 base, OnePlus | optional, 2.222 |
| `ios-pro` | 1206x2622 | iPhone 16/17/18 Pro | optional; 1320x2868 for Pro Max |
| `fold-inner` | 1080x1350 (4:5) | Fold/Pixel Fold inner, Reddit 4:5 sweet spot | Reddit prefers 4:5 [S] |
| `duo` | 1080x1566 (1.45) | Duo outer; Duo inner 1.42 | [U] no player data |
Default recommendation: user wants "fill the screen in portrait" -> `phone-fill`; if destination is Reddit, `reddit-916`. Expose both. Do not claim true fill in Photos on iOS <27; zoom-to-fill is a manual toggle.

## 3. Encoding compatibility
### Reddit upload limits (2026)
| Item | Value | Tag |
|---|---|---|
| Post video max size / length | 1 GB; 15 min (non-Premium), Reddit Premium 30 min | [S] search over support.reddithelp.com snippets; main upload article 403 [U], two reddithelp URLs tried |
| Containers / codec | MP4, MOV; H.264/AVC (HEVC acceptance not documented) | [S] multiple third-party guides (tella, zight, aiarty) agree; not on official page seen |
| Resolution | 4K accepted but playback capped at 1080p; recommend >=720p | [S] liftburst/vid-crush; [U] official |
| Aspect | ads: 1:1, 4:5, 16:9 (MP4/MOV, 1 GB, 15 min) | [S] martech.org / affectgroup |
| Comment videos | <1 GB; uploaded up to 3 min; min 2 s, min 50x50 px | [V] support.reddithelp.com/hc/en-us/articles/48109333836692-How-do-I-add-video-in-comments (via search snippet) |
Plan: assume Reddit transcodes to <=1080p H.264; use H.264 High 8-bit for upload presets to be safe; keep <1 GB; 1440-wide output offers no Reddit benefit.

### Decode support
| Item | Finding | Tag |
|---|---|---|
| iOS formats | Apple specs for 16/17/Air/18 Pro/Duo: playback HEVC, H.264, AV1; DV, HDR10+/HDR10, HLG | [V] apple.com/iphone-*/specs |
| AV1 HW decode | iPhone 15 Pro+ [M]; Apple lists AV1 on 16/17/Air/18/Duo [V]. Pixel 8+ (Tensor G3 decode+encode) [S: notebookcheck/wccftech]. Snapdragon 8 Gen 3 (S24 US) AV1 decode [S androidauthority]; Exynos 2400 (S24 intl) reported AV1 decode, specifics [U]. Android 14+ mandates AV1 decoder (HW or SW) [V developer.android.com/media/platform/supported-formats]. | mixed |
| Android HEVC | decoder mandatory; docs only guarantee Main L3 on mobile, flagships do 5.x [M] | [V] doc page |
| `hvc1` | required for Apple QuickTime/Photos; `hev1` often fails [M]; Apple HLS spec page not readable by fetcher, claim stays [M] | [M] |
| H.264 level | MBs/frame: L4.2 max 8704, L5.0 22080, L5.1 36864; MB/s: L5.0 589,824, L5.1 983,040, L5.2 2,073,600. 1080x1920 = 8160 MB (L4.2 ok at 60 fps); 1080x2340 = 9996 MB (L5.0 @30, L5.1 @60 needs 599,760 MB/s); 1440x3120 = 17,550 MB (L5.0 @30, L5.2 @60 at 1,053,000). Arithmetic [verified here]; level table [M]. Devices' max decoded H.264 level not found on a vendor page [U]. | |
| Practical | 1080x2340@60 H.264 -> L5.1/5.2; HEVC L5.1 fine. Reddit re-encodes anyway so level matters only for local files. | |

### Bitrates (unchanged from 03, [M], validate with VMAF)
1080x1920/2340 @30: x264 CRF 18-20 maxrate 8-10M; x265 CRF 23-24 maxrate 6-8M. 1440x3120: x264 12-16M, x265 8-11M. A fetch of Apple HLS authoring spec failed, so Apple-recommended ladder numbers are NOT verified [U] (a model-recalled 5 Mbps HEVC 1080p, 8 Mbps 1440p was returned; discard as [R]).

## 4. HDR and colour tagging
- Keep SDR sources SDR. Reasons: Reddit/social re-encode; inverse tone-mapping SDR->HDR risks blown highlights, banding, mismatched brightness in feeds (mixed SDR/HDR composition is an Android 13+ concept [V source.android.com HDR docs via search]); phones already render SDR to P3 panels correctly when tagged BT.709. Offer HDR output only when the source is HDR (HEVC Main10, HLG/PQ BT.2020 tags). Android only guarantees HDR10 and HLG10 playback; HDR10+/DV optional [V developer.android.com]; Apple plays DV/HDR10+/HDR10/HLG [V]. Android 14+ also supports HDR10+ and 10-bit; "SDR to HDR" brightness enhancers are OEM-specific [U].
- Pitfalls (verified general ffmpeg facts + [S]): `yuvj420p` is deprecated alias for full-range yuv420p; x264 with yuvj420p encodes full-range values; players that assume limited range then look crushed/contrasty, and the reverse looks washed out [S ffmpeg-user list "Correct conversion of yuvj420p?"]. Phone camera JPEG-style sources and AI upscalers often emit RGB or full-range; always `scale=out_range=tv,format=yuv420p` and tag `-color_range tv`.
- Always write explicit `-colorspace bt709 -color_primaries bt709 -color_trc bt709` for SDR (HD and SD alike after convert); untagged video gets guessed (601 vs 709 shift for <=576p sources, iOS/Android may differ) [M].
- Tag HEVC `hvc1`; for DV profile 8.4 use `dvh1` [M]; do not tag SDR as BT.2020.
- 8-bit upscaled output bands; optional Main10 even for SDR on iOS/Android hardware (both decode Main10) [M]. Reddit transcode removes the benefit; keep 10-bit as local-only "quality" tier.
- Test: ffprobe `-show_entries stream=pix_fmt,color_range,color_space,color_transfer,color_primaries,codec_tag_string`, play in iOS Photos, Google Photos, Samsung Gallery.

## 5. Could not verify (list)
1. Reddit official upload article (403): codecs/res/aspect/1080p cap are from third-party guides. 2. Reddit app cover-vs-fit for tall vertical; no official doc. 3. iOS 27 zoom-to-fill for video. 4. Apple HLS authoring numbers, hvc1 spec statement. 5. iPhone 15/15 Plus/15 Pro/Pro Max, 16 Plus/Pro/Pro Max, 17 Pro panel numbers: not re-fetched (apple.com tech-specs page fetch for 15 Pro returned nothing useful); and which iPhones are 60 vs 120 Hz (conflict 7). 6. Pixel 6-9 base, 10/10 Pro, 11 on store.google.com (S/M only). 7. Samsung S-series pixels not from samsung.com US pages (404 / only support-comparison snippets). 8. Fold8 / Flip8 exact pixels; Flip8 cover orientation. 9. Duo release date/price (03's MacRumors, not re-read); Duo video playback fit. 10. Max decodable H.264/HEVC level per device. 11. OnePlus/Xiaomi rows left as 03 [M].

## Sources (read or searched this session)
- https://www.apple.com/iphone-duo/specs/ ; https://www.apple.com/iphone-18-pro/specs/ ; https://www.apple.com/iphone-17/specs/ ; https://www.apple.com/iphone-air/specs/ ; https://www.apple.com/iphone-16/specs/ ; https://www.apple.com/iphone/compare/
- https://www.samsung.com/uk/smartphones/galaxy-z-flip8/compare/ ; https://www.samsung.com/ie/support/mobile-devices/what-are-the-differences-between-the-galaxy-s25-series-and-s24-series/
- https://store.google.com/my/product/pixel_10_pro_fold_specs ; https://nanoreview.net/en/phone/google-pixel-11-pro-xl ; https://www.phonearena.com/phones/compare/Google-Pixel-11-Pro,Google-Pixel-11-Pro-Fold/phones/12940,12952
- https://developer.android.com/media/platform/supported-formats
- https://support.reddithelp.com/hc/en-us/articles/48109333836692-How-do-I-add-video-in-comments ; https://piunikaweb.com/2025/10/22/reddit-android-bug-stretching-and-distorting-videos/ ; https://www.adweek.com/media/reddit-tests-full-screen-video-player/
- https://www.idownloadblog.com/?p=1060335 ; https://www.iphonelife.com/content/double-tap-videos-make-them-fill-screen
- https://www.notebookcheck.net/Source-code-confirms-Google-Tensor-G3-as-the-first-smartphone-SoC-to-have-hardware-accelerated-AV1-encoding-support.806980.0.html ; https://androidauthority.com/snapdragon-8-gen-3-vs-tensor-g3-3382154
- https://lists.ffmpeg.org/pipermail/ffmpeg-user/2020-September/049981.html
