# Research: Budgeted Reddit Video (005)

Phase 0 for [plan.md](./plan.md). `[K]` marks background knowledge not measured here. Measurements used the bundled FFmpeg n9.0.2
(`binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu`, libx264 core 165) on 2026-11-19.

**Test sources** were 30 s, 1080p30 lavfi, with a 48 kHz sine as audio:

| Name | Video | What it tests |
|---|---|---|
| `hard` | `testsrc2` with full-frame temporal noise (`noise=alls=40:allf=t+u`) | worst case for a rate controller |
| `mixed` | that noise for the first 8 s only | uneven complexity |
| `easy` | plain `testsrc2` | simple content |

Each source was encoded against caps of 4, 12 and 40 MB.

The numbers below are output size ÷ cap, with a 2% margin and AAC at 128 kbit/s. The script is
reproduced in [quickstart.md](./quickstart.md) › Phase 0 measurements. Inputs: the spec,
`research/10-reddit-limits-2026-11-13.md`, `research/09-open-items-settled.md` §5 and
`research/05-gif-export.md` §5.

## R1. Rate control: two-pass ABR with a VBV ceiling

**Decision.** When the budget is the limit, encode the video in two passes at the budget bitrate
`B`, with `-b:v B -maxrate B -bufsize 2B`, using the same video options in both passes. Pass 1
writes no file (`-an -f null`). Pass 2 writes the output.

**Evidence** (size ÷ cap):

| Source / cap | Two-pass ABR | Capped CRF 20 (`-maxrate B -bufsize 2B`) | **Two-pass + VBV (chosen)** |
|---|---|---|---|
| hard 4 MB | 0.980 | 0.998 | **0.982** |
| hard 12 MB | **1.600** | 1.006 | **0.975** |
| hard 40 MB | 0.987 | 1.004 | **0.976** |
| mixed 4 MB | 0.964 | 1.003 | **0.976** |
| mixed 12 MB | **1.721** | 1.003 | **0.990** |
| mixed 40 MB | 0.935 | 0.884 | **0.997** |
| easy 4 MB | 0.963 | 1.009 | **0.971** |
| easy 12 MB | 0.982 | 1.003 | **0.977** |
| easy 40 MB | 0.971 | 0.802 | **0.956** |

- **Two-pass + VBV** stays under the cap and above 95% of it in all nine cases (SC-001, SC-002).
- **Plain two-pass ABR overshot by 60–72%.** This happened on the noisy sources at 3 Mbit/s, and
  was reproducible (run twice with identical pass options) with no x264 warning.
- **Capped CRF** passes the cap by up to 0.9% even with the 2% margin. It undershoots to 80–88%
  when the cap is generous for the content, so it fails both SC-001 and SC-002 as the
  budget-limited mode.

**Cost.** Pass 1 took 7.8–9.9 s, about the same as pass 2 (8.0–10.0 s), so a budgeted encode
costs about twice a single pass. (Single-pass CRF 20 on `mixed` took 29.6 s, because CRF spends
far more bits on noise.)

**Alternatives rejected.**
- Single-pass ABR: x264 documents it as the least accurate mode [K], and the more accurate two-pass
  already needs a ceiling.
- CRF with a size check and retries: unpredictable, and each retry is a full encode.
- A sample encode to predict CRF size, as 003 does for GIFs: unnecessary, because two-pass hits
  its target bitrate.

**Consequence.** The plan's size prediction is arithmetic (R2), with no sample encode, and it
lands within 5% of the result (SC-006 asks for 15%).

## R2. Budget arithmetic, margin and overhead

**Decision:**

```
usable  = cap × (1 − margin)        margin = 2 %
audio   = audio_kbps × 1000 / 8 × len
video_B = (usable − audio) × 8 / len / 1000   kbit/s
```

**Evidence:**
- AAC at 128 kbit/s for 30 s, alone in an MP4 with fast start, is 486,831 bytes against a nominal
  480,000 (+1.4%, including the `moov`).
- With the 2% margin, two-pass + VBV used 95.6–99.7% of the cap (R1).

**Retries (FR-005, FR-010).** If the measured size is over the cap, pass 2 runs again at

```
video_B' = video_B × (usable − audio) / (measured − audio)
```

Pass 1's stats are reused: x264 allows a different bitrate in pass 2 [K; to verify in the first
retry test]. Retries are capped at 3,
then the job fails `FIT_FAILED` with the sizes. Nothing over the cap reaches the output name,
because the size is measured on the `.part` file before commit.

**Alternative rejected.** A 5% margin (002's `maxrate_for_limit`). It wastes 3 points of quality
budget, and the measurement shows 2% is enough.

## R3. Quality ceiling: don't fill a generous cap (FR-007)

**Decision.** A target sets a quality ceiling:
- `quality.crf` (default 20);
- `quality.max_bpp`, bits per pixel per frame (default 0.12, which is 7.5 Mbit/s for 1080p30).

The ceiling bitrate is `C = max_bpp × w × h × fps`.

| Case | Mode | Encode |
|---|---|---|
| `video_B < C` | **budget mode** | R1 |
| `video_B ≥ C` | **quality mode** | one pass at `-crf 20 -maxrate C -bufsize 2C` |

Quality mode never needs more than `C`, so it stays far below the cap. It is still measured,
like every output.

**Evidence.** Capped CRF lands at 0.80–1.009 of its ceiling (R1, capped-CRF column). The 0.9%
overshoot of `C` is harmless, because `C ≤ video_B`. Easy content uses less than `C`, so it
isn't inflated.

**Sources for the default:**
- 0.12 bpp sits within YouTube's published H.264 upload guidance (8 Mbit/s for 1080p30) [K].
- It is above 002's `est_bpp` for the Reddit presets, which are size estimates, not ceilings.

## R4. Resolution ladder and the unreachable case (FR-008, FR-009)

**Decision.** A target sets `min_bpp` (default 0.03, which is 1.9 Mbit/s at 1080p30 and 0.83 at
720p30) and a floor of 480 px on the short side.

- **Ladder.** While `video_B < min_bpp × w × h × fps`, the long side is multiplied by 0.75, kept
  even with the aspect ratio. The ladder never goes below the floor (or below the source, if it is
  smaller). Each lowering is a `FIT_LOWERED` notice.
- **Unreachable.** At the floor, if the video bitrate is still below `min_bpp`, the audio drops
  toward its floor (64 kbit/s). If that's not enough:
  - without trimming, the job is refused `BUDGET_UNREACHABLE` (exit 3). The message gives the
    smallest cap that would work: `(min_kbps(floor) + audio_floor) × len / 8 / (1 − margin)`;
  - with `--fit-allow-trim`, the range is cut to the longest start that fits,
    `len' = usable × 8 / (min_kbps(floor) + audio_floor)`, with a `FIT_TRIMMED` notice.

**Frame rate.** The frame rate isn't on the ladder (YAGNI: halving it doubles the bits per frame,
but changes motion; 003's GIF ladder needs it, video seldom does). It is only capped at the
target's `max_fps` (60).

**Alternative rejected.** A search with sample encodes, as GIF export does. Bitrate is
predictable enough for video that arithmetic is sufficient.

## R5. Length limit (FR-011)

**Decision.**
- A range longer than `max_duration` (900 s for `reddit-video`) is an error `RANGE_TOO_LONG`
  (exit 3), naming both lengths.
- With `--fit-allow-trim`, the range ends at `start + max_duration`, with a `FIT_TRIMMED` notice.
- Both checks happen at plan time, so nothing is encoded (SC-005).

GIF's `LONG_RANGE` stays a warning for GIF targets. A video target's limit is Reddit's hard
limit, so it is an error.

## R6. Audio (FR-013)

**Decision.**
- **Track:** the first default audio track (the stream selection of 001), as AAC-LC at 48 kHz,
  with at most 2 channels (`-ac 2` for more).
- **Bitrate:** the target's (128 kbit/s), lowered only by R4.
- **No audio:** no audio track, and the audio share goes to the video.
- **Extra tracks:** dropped, with a notice.
- **Sync:** cutting reuses 001's accurate cut (`-ss` after the input seek, or `trim`/`atrim`),
  so audio stays in sync. A test checks the audio stream's start against the video's (SC-003,
  within one AAC frame: 1024 samples, 21.3 ms).

## R7. Where the fit runs

**Decision.** `FitPlan` is an optional field on `ClipSpec` (plain clips) and `UpscaleSpec` (clips
with a device preset). It changes the video rate control, the size and the audio. The executor's
existing encode step calls one shared function, `exec::fit::encode`. That function:
1. runs pass 1 then pass 2, or the single quality pass;
2. measures the `.part` file;
3. retries;
4. reports `FitOutcome`.

Pass 1 writes its stats in the job's scratch folder, with relative names and cwd = scratch (003's
convention, no path escaping). A clip job with a fit gets a scratch folder as GIF jobs do.

- **AI tier:** both passes run the AI pipeline (the stats must come from the same frames), so the
  AI time doubles; the dry run's time estimate says so.
- **GPU tier:** works unchanged.

**Alternatives rejected.**
- A new `JobSpec::Fit` variant: it would copy the clip and upscale paths.
- Rendering the AI frames once to a lossless intermediate: too large (minutes of 1080p RGB).

## R8. Output format and compatibility (FR-016)

**Decision.**
- **Video:** MP4 with `-movflags +faststart` (checked: `moov` right after `ftyp`), H.264 High,
  `-pix_fmt yuv420p`, `-tag:v avc1`, even dimensions.
- **Plain clips:** scaled only when R4 lowers the size (lanczos, as 002), otherwise only cropped to
  even.
- **HDR:** tone-mapped with 002's chain, which plain clips also use when the target needs SDR.

**Evidence.** The R1 outputs decode cleanly with `-v error`, ffprobe reports `h264 High yuv420p`
plus `aac LC`, and the MP4 starts `ftyp`, `moov`.

## R9. Verification before commit

**Decision.** On top of the existing accurate-mode checks (duration ±1 frame, streams present),
the executor:
- checks the `.part` size against the cap (R2);
- checks the audio start offset (R6);
- reports `FitOutcome { bytes, cap, video_kbps, audio_kbps, attempts, trimmed, mode }`.

## R10. Targets as data (FR-001 to FR-004)

**Decision.**
- **Location:** video targets are JSON files, schema 1, in `<config>/video-targets/*.json`.
  `reddit-video` is compiled in.
- **Rules:** the same as GIF targets: the id replaces a built-in; at most 64 user files; files
  are read capped; problems are reported by `doctor` and never fatal; a `verified` flag.
- **Shared loader:** a generic `data_files::load<T>` serves both registries. This is a refactor
  of `gif/target.rs`'s loader, with no behaviour change, and its tests move with it.

**Defaults** come from `research/10` (as of 2026-11-13; `verified: false`):

| Field | `reddit-video` |
|---|---|
| `max_bytes` | 1,073,741,824 |
| `max_duration` | 900 s |
| `long_side` | 1920 |
| `floor_short_side` | 480 |
| `max_fps` | 60 |
| `quality` | `crf` 20, `max_bpp` 0.12, `min_bpp` 0.03 |
| `audio` | `kbps` 128, `floor_kbps` 64 |

## R11. Surfaces (FR-017 to FR-021)

**Decision:**
- **CLI:**
  - `--fit TARGET` on `clip`, `upscale` and `batch`;
  - `--fit-max-size SIZE` (for example `50MB`) and `--fit-max-length DUR` override the target for
    one run, and may only lower its limits;
  - `--fit-allow-trim`;
  - `rmedia clip --list-fit-targets [--json]`;
  - `doctor` reports target problems.
- **Manifests:** a `fit` object (`target`, `max_size`, `max_length`, `allow_trim`, `enabled`) at
  every level, with CSV columns `fit`, `fit_max_size`, `fit_max_length`, `fit_allow_trim`.
- **Desktop:** a target picker in the clip panel. The prediction comes from the core's dry-run
  plan (no sampling, R1), and the queue shows the fit block and the result. A new
  `list_fit_targets` command.
- **Conflicts:**
  - `--fit` with `--gif` is a usage error (exit 2): a GIF has its own targets;
  - `--fit` with `--mode fast` or `smart` cuts accurate. Smart gets the `SMART_NOT_APPLIED`
    notice. Fast gets no notice: it is the default mode, so the planner can't tell an explicit
    `--mode fast` from the default, and upscale and GIF jobs already ignore it silently (decided
    in T017; a planned `FIT_REENCODES` code was dropped);
  - `--fit` with a container other than mp4 is refused (exit 3).

## R12. Determinism and parity (Principle II, SC-007)

- The CLI and the desktop app call the same core plan and executor, so their settings are
  identical (checked by a bridge test).
- Output bytes are deterministic per machine, as for accurate mode. This holds only with R13's
  threading (one encoder thread); threaded VBV encodes are not repeatable (measured).

## R13. Determinism of the VBV encodes (found in T014, 2026-11-19)

**Finding.** R12 assumed x264 repeats its output. With a VBV ceiling it doesn't: two runs of
the same command gave different stats and different frames (`framemd5`):
- two-pass at 2,485 kb/s, run concurrently and run one after the other;
- a one-pass capped CRF (`-crf 20 -maxrate -bufsize`), run one after the other.

The cause is frame threads: a single-threaded encode repeats exactly. Accurate mode is
unaffected, because CRF without VBV is deterministic with frame threads (004 R10).

**Measured** on a 6 s, 1080p30 noisy source with 18 threads:

| Encode | Repeats? | Time (pass 1 + pass 2) |
|---|---|---|
| frame threads (x264 default) | no | 2.1 + 2.5 s |
| `sliced-threads=1` in both passes | no (pass 1's stats differ) | 3.4 + 3.5 s |
| **pass 1 `-threads 1`, pass 2 `sliced-threads=1`** | **yes, byte-identical** | 3.7 + 3.6 s |
| quality pass with `sliced-threads=1` | yes | — |

**Decision (revised 2026-11-20).**
- Every fit encode runs with `-threads 1`: pass 1, pass 2 and the quality pass.
- The output is identical for identical inputs on one machine (Principle II), as for accurate
  mode. A batch still uses the CPU: several clips run at once.

**Revision (US5 desktop check, 2026-11-20).** The first decision ran pass 2 and the quality pass
with `sliced-threads=1`, which repeated on the 6 s test above but not in general. On 10 s of the
busy 1080p fixture at 5 MB (3,792 kb/s), with 12 threads:

| Pass 2 | Repeats? | Time |
|---|---|---|
| `sliced-threads=1` | no (three runs, three outputs) | 6.0–6.3 s |
| `sliced-threads=1:lookahead-threads=1` | no | 7.3–7.6 s |
| `sliced-threads=1:threads=12` | no | 6.0–6.2 s |
| **`-threads 1`** | **yes, byte-identical** | 13.8–14.1 s |

A one-pass capped CRF with `sliced-threads=1` did not repeat either. Pass 1 with `-threads 1`
always repeated. The owner chose exact repeats over speed: about 2.3× the sliced-thread time for
pass 2. `a_fit_is_deterministic` now checks this case and quality mode.

**Cost (first decision).** About 1.6× the frame-threaded time in the 6 s test. Single-threaded
pass 2 costs more (above). The bitrate is fixed by the budget, so the size class is the same.

**Alternative rejected.** Frame threads, with determinism documented as an exception. That
breaks a MUST in the constitution for a speed gain under 2×.
