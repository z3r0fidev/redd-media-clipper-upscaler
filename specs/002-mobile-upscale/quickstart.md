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
# landscape with a moving magenta marker and a hard cut at 6 s
$F -f lavfi -i "color=gray:s=1280x720:r=30:d=6[a];color=magenta:s=80x80:r=30:d=6[m];[a][m]overlay=x='100+150*t':y=320[s1];color=0x406080:s=1280x720:r=30:d=4[b];[b][m]overlay=x=1000:y=320[s2];[s1][s2]concat=n=2" land.mp4
```

The marker filter graph is illustrative; the testkit's `moving_marker` fixture is the reference.

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
rmedia upscale land.mp4 --dry-run           # 2 shots; crop path summary; no fallback
rmedia upscale land.mp4 --dry-run --json | jq '.jobs[0].upscale.reframe_analysis | {shots, fallback_ranges, n: (.path.keyframes|length)}'
rmedia upscale land.mp4 --crop-path path.json
```

**Expected:** the output is 1080×2340 and the marker is visible in every sampled frame:

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
```

## US4: batch and chained clipping

```bash
cat > m.json <<'EOF'
{ "version": 1, "defaults": { "upscale": {} },
  "videos": [ { "input": "low.mp4" },
              { "input": "land.mp4", "options": { "upscale": { "preset": "reddit-916" } },
                "clips": [ { "start": 1, "end": 5, "name": "pan" } ] } ] }
EOF
rmedia batch m.json
rmedia clip land.mp4 -s 0 -e 4 --upscale --preset reddit-feed-45
```

**Expected:** three outputs with the sizes their presets set, and no intermediate clip files.

## US5: desktop

```bash
(cd apps/desktop && npx tauri dev)
```

Steps:
1. Open `land.mp4`.
2. Choose "Upscale for phone". The before/after still appears within 5 s, and the crop rectangle
   follows the marker during playback.
3. Add to queue and Run.
4. Export the manifest, then run `rmedia batch` on it. The output hash must match.

## US6: AI tier (machine with a discrete GPU)

```bash
rmedia doctor               # ai: available, sec_per_mpx shown
rmedia upscale low.mp4 --tier ai --on-exists rename
```

**Expected:** progress and ETA are shown, and Ctrl-C stops all three processes within 2 s. On an
integrated GPU, `--tier ai` warns "too slow: estimated N min".

## Owner validation (manual, research/09 [O])

1. Play `low_phone-fill.mp4` and `land_phone-fill.mp4` in the stock gallery app on one iPhone and
   one Android phone. Check for no bars and correct colours (SC-001).
2. Upload `reddit-916` and `reddit-feed-45` outputs to a test subreddit, and note how each looks in
   the feed and in the full-screen player.
