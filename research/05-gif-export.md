# 05 - GIF export ("GIF mode", feature 003)

Researched 2026-10-06. Legend: [V] verified this session (URL given), [L] verified by running local ffmpeg 6.1.1 (Ubuntu) today, [U] unverified / third-party only / from memory.

## 0. Method caveats (read first)
- Reddit's own Help Center (support.reddithelp.com) returns HTTP 403 (Cloudflare challenge) to curl/WebFetch, and web.archive.org is blocked here. So NO Reddit limit below is confirmed from a Reddit primary source. All Reddit figures come from third-party guides and are marked [U-3P]. Before shipping, a human should open the Reddit Help article "What are the image and video file size and format requirements" in a browser and confirm.
- Local experiments used synthetic sources (lavfi testsrc2 / mandelbrot), so absolute sizes are NOT representative of real footage; only syntax validity and relative trends matter. gifski and gifsicle are not installed locally, so their numbers are from published sources only.

## 1. Reddit GIF / animated-media rules (2025-2026)

| Item | Finding | Status |
|---|---|---|
| Max GIF size, post upload | 100 MB (i.redd.it animated GIF); static images 20 MB; native video 1 GB or 15 min | [U-3P] xconvert.com/blog/mp4-to-gif-reddit-subreddit-limits; help.postpone.app/platforms/reddit/media-limits (GIF 100 MB, "Reddit accepts videos up to 1 GB") ; gifdb.com/gif-format-guide/gif-size-limits-social-media/ |
| Contradicting claim | "direct upload limit ~20 MB for GIFs" appears in some guides | [U-3P] conflicting, treat 20 MB as the conservative ceiling |
| Practical budget | Subreddit AutoMod often rejects > 8-10 MB or > 60 s; feed plays smoothly < 8 MB; comment attachments "safe" at 2-3 MB | [U-3P] xconvert (fetched 2026-10-06), whatthegif.com/guides/gif-for-reddit (published 2026-07-10, updated 2026-10-04) |
| Comment GIFs | Only in subreddits with image/GIF comments enabled; size limit for comments not found on any primary source | [U-3P]; limit for comments = [U] |
| Max dimensions | One aggregator claims 4000x2250 before auto-resize | [U-3P] (search snippet, source not fetched) |
| Max duration for GIF | No hard published GIF duration found; 60 s is an AutoMod convention; native video cap 15 min | [U-3P] |
| Transcoding | Reddit transcodes uploaded GIFs to MP4/video and serves them as looping video labelled "GIF" in the player; original .gif is not served | [U-3P] (search synthesis; Neowin article "Reddit introduces a simpler way to post videos and GIFs" is the original 2017 beta announcement, not re-read) |
| Native uploads vs links | Native upload via "Images & Video" autoplays as loop; bare .gif URL links render as links, not animation | [U-3P] whatthegif.com |
| Animated WebP / MP4 accepted | Reddit image upload formats list .jpeg/.jpg/.png/.webp/.heic (static, 20 MB); animated WebP behaviour NOT confirmed. Video uploads: MP4 and MOV | [U-3P]; animated WebP acceptance = [U], assume NOT supported |
| Aspect display | 1:1 and 4:5 recommended for mobile feed; 9:16 works for vertical content; 16:9 renders smaller in feed column | [U-3P] whatthegif.com; precise crop/letterbox behaviour [U] (see 03-mobile-display-targets.md) |

Implication: because Reddit re-encodes GIFs to MP4 anyway, the visual quality seen by Reddit users is limited by Reddit's re-encode of the GIF; the GIF's job is to be a clean, high-fidelity source under the size cap. A silent H.264 MP4 uploaded as video bypasses the GIF round trip entirely (see section 5).

Default budget constants to ship (configurable): post 20 MB hard / 8 MB soft; comment 8 MB hard / 3 MB soft; all marked "verify against Reddit Help".

## 2. FFmpeg GIF generation

### 2.1 Filter options (verified from ffmpeg.org/ffmpeg-filters.html, fetched 2026-10-06) [V]
palettegen:
- `max_colors` (<=256; palette image still 256 entries, unused black)
- `reserve_transparent` (default ON: caps at 255 colors, reserves one for transparency; "useful for GIF optimization"; disable for a standalone image). Note: with reserve on you lose 1 colour but paletteuse+diff_mode can use transparency for unchanged pixels. Keep ON for animations.
- `transparency_color`
- `stats_mode`: `full` (default, whole-video histogram), `diff` (histogram only of pixels that differ from previous frame; favours moving parts when background static), `single` (new histogram per frame; pair with paletteuse `new=1`)

paletteuse:
- `dither`: bayer, heckbert, floyd_steinberg, sierra2, sierra2_4a (default), sierra3, burkes, atkinson, none
- `bayer_scale` 0-5, default 2 (low = more visible crosshatch, less banding; high = less visible pattern, more banding)
- `diff_mode=rectangle` (default none): only re-process changed rectangle; deterministic, less temporal noise, better LZW compression
- `new=1`: take a new palette per output frame (requires palettegen stats_mode=single)
- `alpha_threshold` default 128

GIF muxer [L]: `-loop` (default 0 = infinite; -1 = no loop; N = loops), `-final_delay` centiseconds after last frame (default -1). `-gifflags` optimisations (offsetting/transdiff) exist by default.

### 2.2 Local relative results [L] (mandelbrot 1280x720 -> 640 wide, 15 fps, 4 s; high-entropy, synthetic)
| Variant | Bytes |
|---|---|
| palettegen full + sierra2_4a | 7,310,756 |
| stats diff + sierra2_4a + rectangle | 7,476,904 |
| stats diff + bayer scale 5 + rectangle | 6,675,570 |
| stats diff + dither none + rectangle | 6,333,781 |
| single + new=1 + sierra2_4a | 6,886,100 |
| stats diff + floyd_steinberg + rectangle | 7,085,692 |
| max_colors=128, bayer scale 3, rectangle | 5,339,613 |
| same clip as libwebp q80 animated (libwebp encoder) | 1,183,336 |
| same clip as H.264 crf 23 yuv420p | 307,238 |
Trend only: error-diffusion dithering costs ~10-15% vs bayer-5 and ~15% vs none; 128 colours saves ~20%; WebP is ~6x and H.264 ~24x smaller than best GIF on this clip. (Synthetic; real footage ratio typically similar order of magnitude, [U].)

### 2.3 Dither guidance
- Photographic/video content: `sierra2_4a` (default) or `floyd_steinberg` give the best look per palette, noisier for GIF compression. `bayer` with bayer_scale 3-5 gives smaller files with a visible crosshatch at low scale; scale 4-5 is acceptable and temporally stable.
- Flat graphics, UI, text-heavy clips: `dither=none` (or bayer scale 5) preserves crisp edges and compresses best; banding is the trade-off.
- `diff_mode=rectangle` is almost always a win for size and temporal stability; use it in all presets.
- `stats_mode=diff` best for static-camera clips; `full` for panning/scene-changing clips; `single`+`new=1` gives best per-frame fidelity on scene cuts but flickers colours and bloats size (and many GIF decoders handle local palettes fine, but gifsicle must keep them). Make this a "fidelity" preset option, not default.

### 2.4 Frame rate and the centisecond quirk
GIF delays are integer centiseconds (1/100 s). Verified [L] delays ffmpeg writes (counts of frames per delay, 2 s clips):
- 10 fps -> all 10 cs (exact)
- 12 fps -> 8 and 9 cs mixed (inexact: avg 8.33 cs)
- 15 fps -> 7 and 6 cs mixed (avg 6.67)
- 20 fps -> 5 cs exact
- 24 fps -> 4 and 5 cs mixed
- 25 fps -> 4 cs exact (25 fps)
- 30 fps -> 3 and 4 cs mixed (avg 3.33; ffmpeg reports 100/3 avg) 
- 33 fps -> 3,4 cs mixed (can only be approximated; no exact 33.33)
- 50 fps -> 2 cs exact
Browser clamping [V]: Chromium and Firefox override delays <= 10 ms (i.e. 0 or 1 cs) with 100 ms; 2 cs (50 fps) is the fastest honoured (gigazine.net/gsc_news/en/20220222-fastest-gif-animation, 2022-02-22; deviantart "Frame Delay Times for Animated GIFs"). So never emit delay < 2 cs; max usable fps = 50. Since Reddit re-encodes to MP4 anyway, mixed 3/4 cs is harmless there, but for exact timing prefer fps in {10, 20, 25, 50}; use 15 or 24/30 only as approximations (uneven pacing is subtle). Recommended menu: 10, 12(approx), 15, 20, 25, 30 with 50 as "max smooth" (50 fps GIFs are huge and rarely worth it).
Use `fps=N` filter AFTER trim and BEFORE scale (fewer frames to scale).

### 2.5 Scaling
- Use `scale=W:-2:flags=lanczos+accurate_rnd+full_chroma_int` (lanczos = sharpest standard kernel; accurate_rnd + full_chroma_int avoid rounding/chroma-subsampling blur when converting from yuv420p). [L] syntax verified. Use `-1` instead of `-2` for GIF (GIF has no even-dimension requirement; -2 only needed for MP4/yuv420).
- Never upscale a source for GIF unless Feature 002 supplies an already-upscaled clip; GIF size scales with pixel area. If 002 output is 1080x1920, a GIF should be down-scaled (e.g. 540x960 or 480x854).
- Scale once, in the filtergraph before palettegen, so palette is computed on final pixels.
- Sharpening after downscale for crisp text/edges: `unsharp=5:5:0.6:5:5:0` (luma only, mild) [L syntax verified]; do not sharpen before palette quantisation on noisy sources (noise is expensive in LZW). Alternative: `flags=lanczos` + `,smartblur`/`hqdn3d=1.5:1.5:3:3` denoise BEFORE palette to cut size 10-30% [U].

### 2.6 One-pass vs two-pass
- One-pass `split` pipeline [L verified]: `[0:v]fps=15,scale=...,split[a][b];[a]palettegen[p];[b][p]paletteuse`. Single ffmpeg invocation, no temp file, but ffmpeg buffers all frames until palettegen finishes (RAM: W*H*3 bytes * frames; 640x360x15fps x 20 s ~ 200 MB; hard cap clip length in memory or use two-pass).
- Two-pass (palette.png to temp dir, then paletteuse) uses less RAM and allows reusing palette for size-search iterations (generate palette once per (res,fps,colors) combo). Recommended for the iterative search.

### 2.7 Loop control
`-loop 0` infinite (default; Reddit transcodes to a loop video anyway). `-loop -1` no loop. Add `-final_delay N` (cs) to pause on the last frame for non-loop uses. For perfect loops (r/perfectloops style) offer optional crossfade via xfade [U].

## 3. gifski, gifsicle

### gifski [V]
- Licence: AGPL-3.0-or-later (crates.io metadata, fetched 2026-10-06: v1.34.0, 2025-07-13, `AGPL-3.0-or-later`; v1.33.0 2025-03-17; GitHub README: "AGPL 3 or later... can offer alternative licensing options, including commercial licenses", github.com/ImageOptim/gifski). Repo: github.com/ImageOptim/gifski. Latest release 1.34.0 (July 2025): "palette quality improvements from newer libimagequant", `--in-color-space` for ambiguous y4m.
- AGPL consequence for this app: the app already ships GPL FFmpeg, so GPL-ness is accepted; AGPL-3.0 is compatible one-way with GPLv3 (combination under AGPL network clause). Safest: spawn the `gifski` CLI as a separate executable (aggregate / "arm's-length") and ship its source offer + licence, OR link crate and license the whole app AGPL-3. If the app is ever closed-source/commercial, buy the commercial licence. libimagequant (inside gifski) is itself GPL/commercial (BSD in older versions) [U]. Legal review recommended; not legal advice.
- CLI options [V]: `--quality`, `--lossy-quality`, `--motion-quality`, `--width/--height`, `--fps`, `--repeat`, `--fast`. Ffmpeg integration: `ffmpeg -i video.mp4 -f yuv4mpegpipe - | gifski -o anim.gif -` (README). Also accepts PNG frames. Crate `gifski` can be a library (no ffmpeg needed when you feed RGBA frames from the app's own decoder).
- Quality approach [V via search synthesis]: per-frame palettes with cross-frame palette optimisation and temporal dithering -> thousands of colours per frame effectively; best perceptual quality for photographic content. Published comparison: ffmpeg 2.15 MB vs gifski 1.69 MB on same content [U-3P, search snippet, not primary]; gifski usually bigger than a well-tuned ffmpeg+gifsicle at equal fps/size on some content [U]. Gifski is slower (seconds-minutes) and wants high-quality RGBA input.
- Defaults to try: `gifski --quality 90 --motion-quality 80 --lossy-quality 70 --fps 15 --width 640`. Size-search knobs: width, fps, `--quality` (palette), `--lossy-quality`, `--motion-quality` [U values, tune empirically].

### gifsicle [V licence]
- Licence: GPL-2.0 (GitHub API license key gpl-2.0 for github.com/kohler/gifsicle, fetched 2026-10-06; repo pushed 2026-01-31). GPLv2-only vs GPLv3/AGPL mixing: invoke as separate process, do not link; compatible as aggregate [U legal].
- `-O3` = most aggressive frame-diff/transparency optimisation; `--lossy=N` (30-200, higher = smaller/more artefacts) is LZW-lossy; `--colors N`; `--gamma` interplay. Real data [V]: christianselig.com/2025/08/high-quality-low-filesize-gifs/ (2025-08): 4K H.265 19.5 MB source; naive ffmpeg GIF 409.4 MB; optimised ffmpeg (24 fps, 1000 px wide, floyd_steinberg, palettegen) 45.8 MB; + `gifsicle -O3 --lossy=65 --gamma=1.2` 23.8 MB (~48% reduction of the ffmpeg GIF; lossy levels 60-80 are commonly "visually fine" [U]).
- Typical gains: `-O3` alone 5-25% on ffmpeg output; `--lossy=30..80` another 20-50% [U for ranges beyond the data point above].
- Availability: not in BtbN/gyan ffmpeg bundles; must be bundled separately (Windows builds exist, e.g. eternallybored.org [U]). Optional dependency with graceful fallback.

### Engine decision
Engine A (default, zero extra deps): ffmpeg two-pass palette. Engine B (optional "Best quality"): gifski sidecar fed via y4m pipe or PNG frames, followed by optional gifsicle. Engine C: gifsicle post-pass on A output. Detect sidecars at startup like ffmpeg.

## 4. Hitting a size budget

Problem: GIF size is non-linear and content dependent; no closed-form predictor. Recommended algorithm (bounded, ~6-10 encodes):
1. Pre-trim: respect user clip; compute duration D. Enforce a max (default 15 s post / 6 s comment) or warn. Trimming is the most reliable lever: size ~ linear in duration.
2. Initial guess: bytes_per_pixel_frame estimate b ~ 0.05-0.15 B/px/frame for photographic video after palette+LZW [U, calibrate]; predict size = W*H*fps*D*b; choose starting (W, fps) so predicted <= 0.8 budget.
3. Probe encoding: encode a 1-2 s sample (or 3 evenly-spread 1 s windows) at candidate settings (fast, 2-3 s each) and extrapolate: size_est = sum(sample_sizes) * D / sample_total_duration * 1.1 safety margin. Calibrate bytes/frame from the probe, not from the formula.
4. Parameter ladder (reduce in this order of least visible damage): lossy (gifsicle) 0 -> 40 -> 80; dither error-diffusion -> bayer 4-5; max_colors 256 -> 192 -> 128; fps 20 -> 15 -> 12 -> 10; width stepwise x0.85 (keep multiple of 2; floor 320 px for post, 240 for comment); duration trim as last resort (offer user a choice).
   Prefer resolution drop over fps drop for fast action, fps drop over resolution for static/text content (heuristic by mean frame-diff from ffmpeg `signalstats`/`scdet` or by probe).
5. Binary search on width (monotonic) with fixed fps/colours: converges in ~5 encodes. Cache palette per (width, fps, colors, stats_mode) to skip pass 1.
6. Final verify: stat the output; if > hard limit, step down once more; surface the achieved size/resolution/fps and a warning if below a quality floor (e.g. < 360 px wide).
7. Show preview + "estimated size" live from the probe before the full encode; allow cancel (kill ffmpeg child).
All encodes: `-an -sn -dn -map_metadata -1`, write to temp + atomic rename, run with `-nostdin -progress pipe:1` for UI progress.

## 5. Alternatives

| Format | Size vs GIF | Reddit acceptance | Verdict |
|---|---|---|---|
| H.264 MP4, silent (`-an`) | local test [L]: 307 KB vs 6.3-7.5 MB GIF (~20x smaller, synthetic) | Video upload accepts MP4/MOV, up to 1 GB / 15 min [U-3P]; Reddit labels silent loops as GIF-like autoplay (loops) | Best quality-per-byte; best option for Reddit posts |
| Animated WebP (libwebp) | local test [L]: 1.18 MB (~6x smaller than GIF); libwebp present in BtbN builds (scripts.d/50-libwebp.sh: `--enable-libwebp`, github.com/BtbN/FFmpeg-Builds, 2026-10-06) and gyan builds (gyan.dev/ffmpeg/builds lists libwebp in essentials, version 9.0.2 2026-09-19) | NOT confirmed for Reddit; static .webp accepted [U-3P]; animated likely rejected/flattened [U] | Offer only as generic export, not a Reddit target |
| AVIF animated | smaller than WebP [U] | Not accepted [U] | Skip |
| Real GIF | largest | Accepted; transcoded to MP4 by Reddit [U-3P] | Needed for comments/GIF-only subs/other sites |

Recommendation: YES, include a "Reddit-optimized MP4 (looping, silent)" output next to GIF. Reasons: Reddit re-encodes GIFs to MP4 anyway; MP4 avoids the 256-colour/dither damage and gives 10-30x smaller files, so you can post 1080p / portrait 9:16 at full smoothness (reuses Feature 002 output as-is). Caveats: some subreddits require "gifs" and ban video [U]; comments need GIF/image support; MP4 posts show a player with a progress bar and may have sound icon absent when `-an`. Keep real GIF mode for those.

ffmpeg tooling note: FFmpeg 9.0 is current in BtbN builds (n9.0 gpl/lgpl archives listed, release "latest" 2026-10-05; also n8.1), gyan 9.0.2 (2026-09-19) [V]. GPL builds include libx264 and libwebp; aom and SVT-AV1 scripts exist too. The app's "6.x/7.x" assumption is outdated; all filters used here exist since FFmpeg 2.x-4.x except `palettegen reserve_transparent`/`transparency_color` (3.x-4.x) and `paletteuse alpha_threshold` (4.x) [U on exact versions]; still, feature-detect via `ffmpeg -filters` / `-h filter=paletteuse`.

## 6. Presets (exact graphs)

Common: `-an -map_metadata -1 -loop 0`. `S = flags=lanczos+accurate_rnd+full_chroma_int`. Replace `{W}` with target width. Trim with `-ss/-to` before `-i` (fast seek) when the clip is the whole file, else apply `trim` in graph.

### 6.1 "Reddit post" (target <= 8 MB soft, 20 MB hard; 16:9 -> 640-720 wide; portrait -> 480x854-ish)
```
ffmpeg -i in.mp4 -filter_complex \
"[0:v]fps=15,scale={W}:-1:flags=lanczos+accurate_rnd+full_chroma_int,split[a][b];\
[a]palettegen=max_colors=256:stats_mode=diff:reserve_transparent=1[p];\
[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
-an -loop 0 out.gif
```
Start W=720 (landscape) / 480 (portrait 9:16). Size search lowers per section 4. Optional: `gifsicle -O3 --lossy=40`.

### 6.2 "Reddit comment" (<= 3 MB soft; <= 8 s; 480 wide)
```
[0:v]fps=12,scale=480:-1:flags=lanczos+accurate_rnd+full_chroma_int,split[a][b];
[a]palettegen=max_colors=128:stats_mode=diff[p];
[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle
```
Plus `gifsicle -O3 --lossy=80`. Square or 4:5 center crop optional.

### 6.3 "Max quality" (no budget; r/HighQualityGifs style)
```
[0:v]fps=20,scale=960:-1:flags=lanczos+accurate_rnd+full_chroma_int,split[a][b];
[a]palettegen=max_colors=256:stats_mode=full[p];
[b][p]paletteuse=dither=floyd_steinberg:diff_mode=rectangle
```
Or gifski: `ffmpeg -i in.mp4 -vf "scale=960:-1:flags=lanczos" -f yuv4mpegpipe - | gifski --quality 100 --fps 20 -o out.gif -`. Expect tens of MB for > 6 s.

### 6.4 "Flat/text/UI" variant (crisp edges, small)
```
[0:v]fps=15,scale={W}:-1:flags=lanczos+accurate_rnd+full_chroma_int,unsharp=5:5:0.5:5:5:0,split[a][b];
[a]palettegen=max_colors=64:stats_mode=diff[p];
[b][p]paletteuse=dither=none:diff_mode=rectangle
```
### 6.5 Reddit-optimized MP4
```
ffmpeg -i in.mp4 -vf "fps=30,scale=-2:1080:flags=lanczos+accurate_rnd+full_chroma_int,format=yuv420p" \
 -c:v libx264 -preset slow -crf 20 -profile:v high -movflags +faststart -an out.mp4
```
(Portrait: `scale=1080:-2`; 002 output can be passed through with only `-an`.) Add `-tag:v avc1`; keep bitrate <~ 8 Mbps [U Reddit limit].

### 6.5 Portrait / Feature 002 reuse
- GIFs reuse 002's reframe by running the same crop/scale step then downscale: `crop=ih*9/16:ih,scale=480:854:flags=lanczos...` before `fps`. Do NOT upscale for GIF; take 002's upscaled intermediate and lanczos-downscale to the GIF width (downscaling a 2x upscaled image yields visibly crisper edges than a native low-res source). Sharpen mildly after downscale only.
- Size scales with pixel area: 480x854 (410 K px) ~ 1.8x the pixels of 480x270 (130 K px)... budget accordingly; portrait GIFs 480 wide at 15 fps are usually 2-3x larger than 16:9 at same width [U].

### 6.6 Crisp text/edges checklist
1. Always lanczos with accurate_rnd+full_chroma_int; avoid default bicubic.
2. Integer-ratio downscale when possible (e.g. 1920 -> 960).
3. Palette on final-size pixels (scale before split).
4. dither none/bayer-5 for text; Sierra/FS only for photographic.
5. mild unsharp (amount 0.4-0.6, 5x5) after scale; skip on noisy/grainy sources; consider denoise (hqdn3d) first.
6. Avoid yuv420 round trip: add `format=rgb24`/`format=yuv444p` before scale if source is chroma-subsampled with coloured text [U effect].
7. Do not use `-vsync vfr` tricks that produce < 2 cs delays.

## 7. Implementation notes (Rust)
- Build commands with `std::process::Command`, args as a Vec (no shell); filtergraph passed as one `-filter_complex` string.
- Detect capabilities at startup: `ffmpeg -hide_banner -filters` (palettegen, paletteuse, unsharp), `-encoders` (libwebp, libx264), `gifski --version`, `gifsicle --version`.
- Windows: path with spaces in palette temp file passed to `movie=` filter needs escaping; prefer two-input form `-i in -i palette.png -lavfi paletteuse`.
- Progress: `-progress pipe:1` and parse out_time_us.

## Recommendations
1. Offer three modes: GIF (palette engine), "Reddit MP4 (silent loop)" and, later, animated WebP as a generic export. Make Reddit MP4 the recommended default for posts; GIF for comments/GIF-only subs.
2. Default GIF engine = ffmpeg two-pass `palettegen stats_mode=diff` + `paletteuse dither=bayer bayer_scale=4 diff_mode=rectangle`, lanczos+accurate_rnd+full_chroma_int scaling, 15 fps (prefer 10/20/25/50 for exact centisecond timing, never < 2 cs delay).
3. Add optional sidecars: gifsicle (-O3 --lossy 40-80, GPL-2.0 separate process) and gifski (AGPL-3.0-or-later; run as separate CLI and decide licence strategy / commercial licence before shipping). Both optional with graceful fallback.
4. Implement budget search per section 4 (probe-based, ladder lossy -> dither -> colours -> fps -> width -> duration, floor widths 320/240) with live estimated size.
5. Default budgets: post 8 MB soft / 20 MB hard; comment 3 MB soft / 8 MB hard; configurable; clearly labelled "unverified, check Reddit/sub rules" until a human confirms against the Reddit Help Center (blocked here).
6. Portrait: reuse feature 002 reframe, then lanczos-downscale to 480 wide (post) / 360 (comment); never upscale in GIF.
7. Feature-detect filters; keep bundled build current (BtbN n9.0/n8.1 and gyan 9.0.2 include libwebp and libx264).
8. Open items to verify manually: Reddit official limits (post/comment GIF MB, dimensions, duration), whether animated WebP is accepted, comment-GIF upload size, mobile aspect-ratio crop behaviour; gifski vs ffmpeg size/quality numbers on the project's own sample clips (bench harness recommended).

## Sources (fetched 2026-10-06)
- https://ffmpeg.org/ffmpeg-filters.html (palettegen 11.192, paletteuse 11.193) [V]
- https://github.com/ImageOptim/gifski ; https://crates.io/api/v1/crates/gifski [V]
- https://github.com/kohler/gifsicle (GPL-2.0 via GitHub API) [V]
- https://christianselig.com/2025/08/high-quality-low-filesize-gifs/ [V]
- https://www.gigazine.net/gsc_news/en/20220222-fastest-gif-animation (browser delay clamping) [V]
- https://github.com/BtbN/FFmpeg-Builds (scripts.d/50-libwebp.sh, release latest 2026-10-05) [V]
- https://www.gyan.dev/ffmpeg/builds/ (9.0.2, 2026-09-19) [V]
- https://www.xconvert.com/blog/mp4-to-gif-reddit-subreddit-limits ; https://whatthegif.com/guides/gif-for-reddit ; https://help.postpone.app/platforms/reddit/media-limits ; https://gifdb.com/gif-format-guide/gif-size-limits-social-media/ [3P, not Reddit primary]
- https://support.reddithelp.com/ (403, not retrievable) [U]
