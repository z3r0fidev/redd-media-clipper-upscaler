# Quickstart: validating feature 003

These are runnable checks that show the feature working end to end. They are not the test suite;
`cargo test --workspace` covers the automated versions.

## Prerequisites

```bash
cargo xtask fetch-tools          # bundled FFmpeg n9.0.2 + gifsicle 1.98 + gifski 1.34.0, SHA-256 verified
cargo xtask fetch-models         # YuNet, for smart framing (US4)
B=$PWD/binaries; T=x86_64-unknown-linux-gnu
export RMEDIA_FFMPEG=$B/rmedia-ffmpeg-$T RMEDIA_FFPROBE=$B/rmedia-ffprobe-$T \
       RMEDIA_GIFSICLE=$B/rmedia-gifsicle-$T RMEDIA_GIFSKI=$B/rmedia-gifski-$T
cargo build -p rmedia-cli && alias rmedia=$PWD/target/debug/rmedia
F=$RMEDIA_FFMPEG; P=$RMEDIA_FFPROBE; mkdir -p /tmp/g && cd /tmp/g
```

Generate the test sources. Nothing binary is committed.

```bash
$F -f lavfi -i testsrc2=s=1280x720:r=30:d=6 -c:v libx264 land6.mp4                  # US1
$F -f lavfi -i "mandelbrot=s=1280x720:r=30,trim=duration=10" -c:v libx264 busy10.mp4  # US2: busy
$F -f lavfi -i "color=white:s=1280x720:r=30:d=6,drawgrid=w=64:h=64:t=2:c=black" -c:v libx264 flat.mp4
```

The "delays" check used below:

```bash
delays() { $P -v error -select_streams v -show_entries packet=duration -of csv=p=0 "$1" | sort -n | uniq -c; }
```

## US1: a GIF that fits a Reddit post

```bash
rmedia gif land6.mp4 --dry-run           # reddit-post (limits unverified), 720x405, 15 fps ≈6.67 cs, predicted size, MP4 hint
rmedia gif land6.mp4
ls -l gifs/land6_reddit-post.gif         # ≤ 8 000 000 bytes
delays gifs/land6_reddit-post.gif        # only 6 and 7; nothing under 2
$P -v error -show_entries stream=width,height,nb_frames -of csv=p=0 gifs/land6_reddit-post.gif   # 720,405,90 (±1)
rmedia gif land6.mp4 --fps 30 --on-exists rename   # note: 30 fps approximate (3–4 cs); never under 2
rmedia gif land6.mp4 --start 1 --end 3 --name '{source}_cut.{ext}'   # 2 s ±1 frame
rmedia gif land6.mp4 --width 1600 --dry-run       # "kept source width 1280" (no upscaling)
rmedia gif busy10.mp4 --target max-quality        # engine gifski; with RMEDIA_GIFSKI unset: built-in + ENGINE_FALLBACK
```

**Cancel:** start `rmedia gif busy10.mp4 --target max-quality`, and press Ctrl-C during
`encode`. Within 2 s it should exit 130, `ls -a gifs/` should show no `.part` or `.d` entry,
and `pgrep -f rmedia-gifski; pgrep -f rmedia-ffmpeg` should print nothing (`-f`: the
triple-suffixed names are longer than the 15-character process name).

## US2: hit a size budget

```bash
rmedia gif busy10.mp4 --budget 3MB --json | tail -1 | jq '.outcome | {bytes, search: .search | {encodes, fit, lowered}}'
#   bytes ≤ 3 000 000 (--budget alone sets the hard limit too), encodes ≤ 10,
#   lowered in the order lossy → dither → colours → fps → width
rmedia gif busy10.mp4 --budget 3MB --on-exists rename && cmp gifs/busy10_reddit-post.gif gifs/busy10_reddit-post-1.gif   # identical
rmedia gif busy10.mp4 --budget 150k --hard-limit 200k        # BUDGET_UNREACHABLE, "fits ≈ N s", nothing written, exit 1
rmedia gif busy10.mp4 --budget 150k --hard-limit 200k --allow-trim   # trimmed to ≈ N s, reported
rmedia gif busy10.mp4 --budget 10k --dry-run --no-sample     # static floor: rejected at plan time (min 64 KiB), exit 3
RMEDIA_GIFSICLE=/nonexistent rmedia gif busy10.mp4 --budget 3MB --on-exists rename   # works; no lossy rung; ENGINE_FALLBACK
```

## US3: Reddit MP4

```bash
rmedia gif land6.mp4 --format mp4
$P -v error -show_entries stream=codec_type,codec_name -of csv=p=0 gifs/land6_reddit-post.mp4   # video,h264 only
python3 -I -c "d=open('gifs/land6_reddit-post.mp4','rb').read(); print(d.find(b'moov') < d.find(b'mdat'))"  # True (faststart)
rmedia gif land6.mp4 --format mp4 --fps 15 --name '{source}_mp4at15.{ext}'   # SC-006: same width and fps as the GIF
ls -l gifs/land6_reddit-post.gif gifs/land6_mp4at15.mp4   # mp4 at least 5x smaller (busy/testsrc content; research R6)
```

## US4: portrait and square via smart reframe

Use `land.mp4` from `specs/002-mobile-upscale/quickstart.md` (a real face panning).

```bash
rmedia gif land.mp4 --shape 4:5 --framing smart --dry-run     # 002 reframe summary + 480x600
rmedia gif land.mp4 --shape 9:16 --framing blur
rmedia upscale low.mp4 && rmedia gif upscaled/low_phone-fill.mp4 --target reddit-comment   # downscaled to 360 wide, never up
```

## US5: batch and clip

```bash
cat > m.json <<'EOF'
{"version":1,"defaults":{"gif":{"target":"reddit-post"}},
 "videos":[{"path":"land6.mp4","clips":[{"start":"0","end":"2"},{"start":"2","end":"4","options":{"gif":{"target":"reddit-comment"}}}]},
           {"path":"busy10.mp4","clips":[{"start":"1","end":"4"}]}]}
EOF
rmedia batch m.json                       # 3 GIFs; the second one is comment-sized; no intermediate .mp4 left
rmedia clip land6.mp4 -s 0:01 -e 0:03 --gif --shape 1:1
printf '0:00 0:02\n0:02 0:04\n' > list.txt && rmedia clip land6.mp4 -l list.txt --gif   # two clips, two files (clip template)
rmedia clip land.mp4 -s 0:01 -e 0:05 --gif --upscale --preset reddit-feed-45   # one job: 4:5 reframe → GIF; note "upscale skipped"
```

## US6: desktop

Run `npm --prefix apps/desktop run tauri dev`, then:
1. Open `land6.mp4`, set In/Out, and choose the **GIF** tab. The estimate should appear within
   5 s and change when you pick "Reddit comment".
2. Click **Export now**. The GIF should play in the result card, showing its size, width, fps
   and the lowered settings. The real size should be within 25% of the estimate.
3. Export the queue as a manifest, then run `rmedia batch rmedia-queue.json --on-exists rename`.
   `cmp` against the desktop output should show no difference.

## Doctor

```bash
rmedia doctor                             # gif: yes, reddit mp4: yes, gifsicle 1.98 (bundled|config), gifski 1.34.0
env -u RMEDIA_GIFSKI -u RMEDIA_GIFSICLE PATH=/usr/bin rmedia doctor   # engines "no (not found…)", status still ready
```
