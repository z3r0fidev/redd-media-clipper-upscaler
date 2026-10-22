# Quickstart: validating feature 002

These are runnable checks that show the feature works end to end. They are not the test suite;
`cargo test --workspace` covers the automated versions.

## Prerequisites

```bash
cargo xtask fetch-tools          # bundled FFmpeg n9.0.2
cargo xtask fetch-models         # YuNet (+ Real-ESRGAN for US6), SHA-256 verified
export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
       RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
cargo build -p rmedia-cli && alias rmedia=$PWD/target/debug/rmedia
F=$RMEDIA_FFMPEG; P=$RMEDIA_FFPROBE; mkdir -p /tmp/q && cd /tmp/q
```

Test sources are generated here; nothing binary is committed.

```bash
# soft 480x854 portrait, heavily compressed
$F -f lavfi -i testsrc2=s=480x854:r=30:d=20 -f lavfi -i sine=d=20 -c:v libx264 -crf 35 -c:a aac low.mp4
# landscape with a real face (the public-domain test portrait from fetch-models) panning right,
# then a hard cut at 6 s to a still shot with the face at x=900
FACE=$OLDPWD/models/test_face_nasa_jsc2003e41874.jpg
$F -loop 1 -i "$FACE" -filter_complex "[0]scale=-2:300,format=yuv420p,split[f1][f2];color=gray:s=1280x720:r=30:d=6[a];[a][f1]overlay=x='100+120*t':y=200:shortest=1[s1];color=0x406080:s=1280x720:r=30:d=4[b];[b][f2]overlay=x=900:y=200:shortest=1[s2];[s1][s2]concat=n=2[v]" -map "[v]" -t 10 -c:v libx264 land.mp4
# 4:3 slides-like source for the batch check
$F -f lavfi -i testsrc2=s=960x720:r=30:d=8 -c:v libx264 slides.mp4
```

Smart reframe follows faces (YuNet), so `land.mp4` uses a real face. The automated tests use the
testkit's `MovingMarker` fixture with a test-only marker detector instead; a plain coloured marker
is not a face and would be shown over the blurred background by the real app.

## US1: phone-ready output from a soft portrait clip

```bash
rmedia upscale low.mp4 --dry-run          # preset phone-fill 1080x2340, tier, estimate, output name
rmedia upscale low.mp4
$P -v error -select_streams v:0 -show_entries stream=width,height,codec_tag_string,color_space,color_primaries,color_transfer,color_range -of csv=p=0 upscaled/low_phone-fill.mp4
```

**Expected:** `1080,2340,hvc1,bt709,bt709,bt709,tv`, and `low.mp4` is unchanged (sha256 the same
before and after).

```bash
rmedia upscale low.mp4 --preset reddit-916 --on-exists rename   # 1080x1920 H.264, +faststart
rmedia upscale upscaled/low_phone-fill.mp4 --dry-run             # "no upscaling needed"
```

To check cancellation, run `rmedia upscale low.mp4 --on-exists overwrite` and press Ctrl-C after
about 1 s.

**Expected:** exit code 130 within 2 s, no `.part` file or scratch folder left, and no `rmedia-ffmpeg`
process running.

## US2: landscape to portrait with smart reframe

```bash
rmedia upscale land.mp4 --dry-run           # "reframe: smart, 2 shot(s)"; no blurred ranges
rmedia upscale land.mp4 --dry-run --json | jq '.jobs[0].upscale.reframe_analysis | {status, shots: (.shots|length), fallback_ranges}'
rmedia upscale land.mp4 --crop-path path.json
jq '{fps, crop, x0: .x[0], x60: .x[60], x120: .x[120], x210: .x[210], x270: .x[270]}' path.json
```

**Expected:** `status` `ready`, 2 shots, `fallback_ranges` `[]`; the output is 1080×2340. The
crop's left edge `x` rises during the pan (about 74, 300, 540 at 0, 2 and 4 s at 30 fps) and holds
near the face after the cut (856 at 7 and 9 s). Sampled frames show the face:

```bash
for t in 1 3 5 7 9; do $F -v error -ss $t -i upscaled/land_phone-fill.mp4 -frames:v 1 f$t.png; done
```

Also try `--reframe center` and `--reframe blur`. Blur shows the whole frame over a blurred,
darkened background.

## US3: tiers

```bash
rmedia doctor               # tiers: cpu yes; gpu yes on a Vulkan GPU (llvmpipe rejected); ai with reason
rmedia doctor --recheck
rmedia upscale low.mp4 --tier gpu --on-exists overwrite    # forced GPU; exit 3 on a machine without one
rmedia upscale low.mp4 --tier auto --dry-run --json | jq '.jobs[0].upscale | {tier, tier_reason}'
# → gpu when the GPU self-test is at least as fast as the CPU's; otherwise cpu with
#   "auto: <GPU> measured slower than the CPU here (21 vs 35 fps); the CPU tier"
VK_DRIVER_FILES=/none.json VK_ICD_FILENAMES=/none.json RMEDIA_STATE_DIR=$(mktemp -d) \
  rmedia upscale low.mp4 --tier gpu --dry-run    # exit 3: … not available on this machine: no Vulkan device
```

## US4: batch and chained clipping

```bash
cat > m.json <<'EOF'
{ "version": 1, "defaults": { "upscale": {} },
  "videos": [ { "input": "low.mp4" },
              { "input": "land.mp4", "options": { "upscale": { "preset": "reddit-916" } },
                "clips": [ { "start": 1, "end": 5, "name": "pan" } ] },
              { "input": "slides.mp4", "options": { "upscale": { "reframe": "blur", "tier": "cpu" } } } ] }
EOF
rmedia batch m.json
rmedia clip land.mp4 -s 0 -e 4 --upscale --preset reddit-feed-45
```

**Expected:** three outputs from the manifest (`low` and `slides` at 1080×2340, the `pan` clip at
1080×1920) and one from `clip --upscale` at 1080×1350, each the size its preset sets, and no
intermediate clip files.

## US5: desktop

```bash
(cd apps/desktop && npx tauri dev)
```

Steps:
1. Open `land.mp4`.
2. Choose "Upscale for phone". The before/after still appears within 5 s, and the crop rectangle
   follows the marker during playback.
3. Select **Upscale now** (or **Add to queue**, then Run).
4. Export the manifest, then run `rmedia batch` on it. The output hash must match.

## US6: AI tier (machine with a discrete GPU)

```bash
rmedia doctor               # ai: available, sec_per_mpx shown
rmedia upscale low.mp4 --tier ai --on-exists rename
```

**Expected:** progress and ETA are shown, and Ctrl-C stops all three processes within 2 s. On a
card that does not pass the self-test (an integrated GPU, typically), `--tier ai` stops with exit 3:
`the ai tier is not available on this machine: too slow on … about N min for 60 s of 480p (limit
10 min)`. On a card that passes, a clip predicted at over 10 times its length warns
`too slow: estimated N min …` and runs.

## Owner validation (manual, research/09 [O])

1. Play `low_phone-fill.mp4` and `land_phone-fill.mp4` in the stock gallery app on one iPhone and
   one Android phone. Check for no bars and correct colours (SC-001).
2. Upload `reddit-916` and `reddit-feed-45` outputs to a test subreddit, and note how each looks in
   the feed and in the full-screen player.
