# Quickstart: Budgeted Reddit Video (005)

How to check that 005 works end to end. The contracts define the expected output:
[cli.md](./contracts/cli.md), [json-output.md](./contracts/json-output.md),
[manifest-fit.md](./contracts/manifest-fit.md). The rules come from [research.md](./research.md).

## Prerequisites

```bash
export RMEDIA_REQUIRE_FFMPEG=1
export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu
export RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
cargo build -p rmedia-cli
R=target/debug/rmedia
# A 1080p30 source with audio, 4 minutes, busy enough to make the budget matter:
$RMEDIA_FFMPEG -v error -f lavfi -i "testsrc2=s=1920x1080:r=30,noise=alls=20:allf=t+u" \
  -f lavfi -i "sine=f=440:r=48000" -t 240 -c:v libx264 -crf 12 -preset veryfast -c:a aac src.mp4
```

## Checks per success criterion

| SC | Command | Expected |
|---|---|---|
| SC-001 | `$R clip src.mp4 -s 0:10 -e 3:10 --fit reddit-video --fit-max-size 40MB --json` | `fit.bytes ≤ 40000000`; ffprobe shows h264 High yuv420p + aac |
| SC-002 | same | `fit.bytes ≥ 36000000` (≥ 90%); `fit.mode = "budget"` |
| SC-003 | `$RMEDIA_FFPROBE -v error -show_entries stream=codec_type,start_time,duration -of json <out>` | video duration 180 s ± 1 frame; audio start within 21.3 ms of the video's |
| SC-004 | the integration test matrix (`crates/rmedia-core/tests/fit_*.rs`) | ≥ 95% of jobs have `attempts.len() == 1`; none has more than 4 |
| SC-005 | `$R clip src.mp4 -s 0 -e 3:00 --fit reddit-video --fit-max-length 1:00 --dry-run` | exit 3, `RANGE_TOO_LONG`, nothing written |
| SC-005 | `$R clip src.mp4 -s 0 -e 4:00 --fit reddit-video --fit-max-size 1MB --dry-run` | exit 3, `BUDGET_UNREACHABLE` with `min_cap` |
| SC-006 | dry run, then run, with `--json` | `fit.bytes / fit.predicted_bytes` between 0.85 and 1.0 |
| SC-007 | `apps/desktop/src-tauri/tests/bridge.rs` fit test | the desktop plan's `fit` block equals the CLI's for the same manifest |
| SC-008 | owner: upload a `reddit-video` output with sound, and a `--preset reddit-916 --fit reddit-video` one | both post and play with sound (record in `docs/operations/`) |

**Trim, lowering and quality mode:**

```bash
$R clip src.mp4 -s 0 -e 4:00 --fit reddit-video --fit-max-length 1:00 --fit-allow-trim   # 60 s output, FIT_TRIMMED
$R clip src.mp4 -s 0 -e 3:00 --fit reddit-video --fit-max-size 15MB --dry-run           # FIT_LOWERED size 1920x1080 → smaller
$R clip src.mp4 -s 0 -e 0:20 --fit reddit-video --dry-run                               # mode quality (1 GB is generous)
```

**Shapes and manifests:** `$R upscale src.mp4 -s 0 -e 1:00 --preset reddit-916 --fit reddit-video`
gives 1080×1920 within the cap. To check manifests, run the JSON and CSV manifests from
[manifest-fit.md](./contracts/manifest-fit.md) with `batch --dry-run --json`; the plans are
identical.

**Unchanged behaviour (FR-022):** the existing `clip`, `upscale` and `gif` tests pass with no
snapshot changes.

## Desktop (manual, Xvfb `:99`)

1. Open `src.mp4`, choose 0:10–3:10, and pick **Reddit video**. The panel shows the cap, 15:00, and
   the predicted size.
2. Choose 0:00–20:00 (on a longer source). The "too long" notice appears, and **Allow trimming**
   enables **Add to queue**.
3. Run the queue. The result shows `bytes of cap`, and the file matches the CLI's settings for the
   same range.

## Phase 0 measurements (research R1, R2)

The script that produced R1's table. It needs about 6 minutes on 18 threads.

```bash
FF=$RMEDIA_FFMPEG
src() { $FF -v error -f lavfi -i "$2" -f lavfi -i "sine=f=440:r=48000" -t 30 -c:v libx264 -crf 1 -preset ultrafast -c:a pcm_s16le $1.mkv; }
src hard  "testsrc2=s=1920x1080:r=30,noise=alls=40:allf=t+u"
src mixed "testsrc2=s=1920x1080:r=30,noise=alls=40:allf=t+u:enable='between(t,0,8)'"
src easy  "testsrc2=s=1920x1080:r=30"
for f in hard mixed easy; do for mb in 4 12 40; do
  cap=$((mb*1000000)); vk=$(( cap*98/100*8/1000/30 - 128 ))
  V="-c:v libx264 -preset medium -b:v ${vk}k -maxrate ${vk}k -bufsize $((vk*2))k -pix_fmt yuv420p"
  $FF -v error -y -i $f.mkv $V -pass 1 -passlogfile w_$f$mb -an -f null -
  $FF -v error -y -i $f.mkv $V -pass 2 -passlogfile w_$f$mb -c:a aac -b:a 128k -movflags +faststart w_$f$mb.mp4
  echo "$f ${mb}MB $(stat -c %s w_$f$mb.mp4) / $cap"
done; done
```

To reproduce the rejected columns:
- plain two-pass ABR: drop `-maxrate`/`-bufsize`;
- capped CRF: one pass with `-crf 20 -maxrate ${vk}k -bufsize $((vk*2))k`.
