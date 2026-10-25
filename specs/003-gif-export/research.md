# Research & Decisions: GIF Export for Reddit

Sources: `research/05` (GIF export), `research/03` (mobile displays), 002's `research.md` (job
model, reframe, colour), and the measurements below.

Tags:
- [V] verified on the bundled engine (n9.0.2) on 2026-10-23, or read from a primary source;
- [D] design decision;
- [U] unverified, to be calibrated during implementation.

## R1. Built-in engine: two-pass palette

- **Decision:** two FFmpeg runs per encode. Both use the GIF frame chain from refactor B1
  (`Flavor::Gif`):
  - trim and pre-processing, as in 002 (deinterlace, square pixels, tone-map);
  - the `fps` cap at `F = min(target fps, source fps)` in the existing `fps_cap` slot of
    `pre_chain_at`. It comes after trim and before scaling (research/05 §2.4). Capping at the
    source rate avoids duplicated frames;
  - the reframe crop or blur-pad, planned by `geometry::plan(display, (W, H), reframe)` straight
    at the final GIF size, with `W = min(target width, crop width)`. There is one resample only,
    with the swscale `lanczos+accurate_rnd+full_chroma_int` scaler;
  - an alpha flatten when the probe reports `has_alpha` (R15);
  - optional `unsharp` (flat-ui);
  - no denoise, CAS or `format=yuv420p` tail. The chain ends in an open label.

  The two runs:
  1. **Palette:** `<chain>,palettegen=max_colors=C:stats_mode=M:reserve_transparent=1` writes
     `p-<key>.png` into the job's scratch folder.
  2. **Encode:** `-i <source> -i p-<key>.png -lavfi "<chain>[x];[x][1:v]paletteuse=dither=D[:bayer_scale=S]:diff_mode=rectangle"`,
     with `-f gif -fflags +bitexact -loop <R15> -an -sn -dn -map_metadata -1`.

  - Both run with cwd = scratch and relative file names, as 002 R5 does. That keeps Windows
    drive letters out of filtergraphs.
  - The palette is cached by `(chain hash, W, F, C, M)`, so a step that only changes dithering
    skips pass 1.
- **Measured [V]:**
  - `testsrc2` 1280×720 at 30 fps for 4 s, sent to 640 px at 15 fps (bayer 4, stats `diff`):
    two runs gave byte-identical output (`ae1af505…`).
  - Filters present: `palettegen`, `paletteuse`, `unsharp`. Muxers present: `gif`,
    `yuv4mpegpipe`.
  - This was measured on n9.0.2 only. The exact-delay and determinism tests are gated to the
    bundled engine (`require_engine!` plus a version check). The minimum-engine job (6.1) checks
    validity, the ≥ 2 cs rule and the frame count.
- **Rationale:**
  - The two-pass design has lower memory use than the one-pass `split` pipeline, which buffers
    every frame until the palette is ready (research/05 §2.6).
  - The palette is reusable across search steps.
  - It is computed from final-size pixels (FR-002).
- **Alternatives:**
  - One-pass `split`: RAM grows with clip length, and the search could not reuse palettes.
  - `stats_mode=single` with `new=1`: it flickers and bloats.

## R2. Targets as data

- **Decision:** a `GifTarget` JSON registry following
  [contracts/gif-target.schema.json](./contracts/gif-target.schema.json).
  - Built-ins are compiled in.
  - User targets live in `<config>/gif-targets/*.json` and override by `id` (R13).
- **Shipped set** [D] (spec FR-006, research/05 §6):

  | id | soft / hard | width land / portrait (floor) | fps (floor) | colours | dither | lossy | engine | max dur |
  |---|---|---|---|---|---|---|---|---|
  | `reddit-post` (default) | 8 MB / 20 MB | 720 / 480 (320) | 15 (10) | 256 | bayer 4 | 30 | builtin | 15 s |
  | `reddit-comment` | 3 MB / 8 MB | 480 / 360 (240) | 12 (10) | 128 | bayer 5 | 60 | builtin | 6 s |
  | `max-quality` | — | 960 / 640 | 20 | 256 | floyd_steinberg | 0 | best (gifski quality 100) | — |
  | `flat-ui` | — | 720 / 480 | 15 | 64 | none, `unsharp` 0.5 | 0 | builtin | — |

  - Targets with a budget carry `"verified": false` (FR-007).
  - Floors are data (`floor_width`, `floor_fps`). Without them, that setting is not lowered.
  - The default `lossy` is [U]: it is calibrated in US2 against SC-009 (≥ 15% smaller, ≤ 1 VMAF
    point lost). The ladder starts from the target's value.
- **Rationale:** the constitution requires presets as data. A separate registry keeps the
  device-preset schema tight (plan, Complexity Tracking).

## R3. Frame timing

- **Measured [V]:** a 15 fps GIF has packet durations of 6 cs (20 frames) and 7 cs (40 frames).
  ffprobe reports `time_base=1/100`, `r_frame_rate=15/1` and `nb_frames=60` (research/05 §2.4).
- **Decision:**
  - The CLI, manifests and schema accept 1 to 50 fps for GIF; values above 50 are a usage or
    validation error, not a silent clamp.
  - Exact rates are those where 100/fps is a whole number ≥ 2: 10, 20, 25 and 50. Any other
    rate is reported as approximate with its average delay (`TIMING_APPROXIMATE`), for example
    "30 fps → 3–4 cs (≈ 33.3 ms)".
  - The rate is never changed to a different exact one behind the user's back.
  - `timing.rs` predicts the delay sequence (the muxer accumulates rounding). Verify checks that
    every packet is ≥ 2 cs and that the total is within one frame of the request.
- **VFR sources:** the fps cap resamples to a constant rate against the source clock, so the
  duration holds.

## R4. Size estimate and sample encodes

- **Sample:** three 1 s windows at 10%, 50% and 90% of the range.
  - Each window is a **separate input** with its own fast input seek (`-ss`, `-t 1`), as 002 does
    for clips. The windows are joined with `concat` after the per-input frame chains.
  - With smart framing, each window gets its own sendcmd file with `cmd_offset` = the window
    start, so crop commands land on the right frames (R16).
    - [V] T005(d), 2026-10-24: per-window files work as designed. An empty sendcmd file is an
      error ("No commands were specified"), so each window's file starts with the crop's
      position at the window start.
    - [V] T041, 2026-10-25: changed. The windows are joined with `concat` **before** one frame
      chain (`[0:v][1:v][2:v]concat=n=3[cat]` → chain → `split` → `palettegen`/`paletteuse`).
      Per-input chains would need unique pad labels in every chain. With smart framing (US4),
      one sendcmd file on the joined timeline does the same job: window `k` starts at `k` s,
      so each window's commands are shifted by `k − window start`.
  - One ffmpeg run, counted as one encode.
  - A range of 3 s or less (4 s before the US1 review: the sample stays within 3 s / 150 frames) is encoded whole: the sample is the real output, and the result is
    reused if it is chosen.
- **Model** (the code review's H3 bias fix): each window's GIF bytes are split into its first
  frame and its delta frames, read from ffprobe `packet=size`.
  `estimate = header + first_frame(window 1) + mean_delta_bytes × (frames(D) − 1)`, with no
  safety factor.
  - This avoids charging the three window key frames and the two joins to the whole clip, which
    would over-estimate static and flat clips.
  - The gifsicle `-O3` and lossy post-pass runs on the sample GIF before measuring, because it
    changes both parts.
- **What the estimate shows** (the code review's H2 fix): the estimate shown before encoding
  (dry run, desktop, US1 scenario 5, FR-019, SC-004) is the **predicted result of the budget
  search**, not the defaults.
  - It runs the same deterministic search as the job (R5) without the full encode, and reports
    the settings it expects to lower.
  - With no budget it is one sample at the target's settings.
- **Where samples run:**
  - Never in the planner.
  - The dry-run estimate runs after planning, in a prepare step (as 002's `prepare_plan` runs
    reframe analysis), using a temporary scratch folder that is removed afterwards.
  - `--no-sample` skips it: the estimate is `pending`. `batch --dry-run` defaults to
    `--no-sample`; `--sample` turns it on.
  - The job's own search runs its own samples. Repeating them is cheap, and keeps the executor
    independent of a previous process.
- [V] T036, 2026-10-25: the model's error against the real GIF (built-in engine, no gifsicle):
  Land6 +1.2%, FlatUi −1.0%, Static6 +0.0%, Busy10 −6.0%.
- **[U] calibration:** SC-004 asks for 90% within 25%. The US2 tests measure it on the fixture set
  (busy, flat, static, moving marker, testsrc). The model constants live in `estimate.rs` only.
- **Alternatives:**
  - Concatenated `trim` windows in one decode: biased, and wrong for sendcmd timestamps (code
    review H4).
  - A bytes-per-pixel formula: misses by 2–5×.

## R5. Budget search

A pure function `search(target, opts, &mut dyn Sampler) -> SearchRecord`.

**Counting** (FR-010, code review H1):
- An *encode* is an ffmpeg run that produces a GIF: a sample or the full output.
- gifsicle re-runs on an already encoded sample (lossy rungs) are post-passes, not encodes. They
  are recorded as steps.
- Budget: at most **7 samples + 1 full + 1 retry = 9**, leaving 1 spare. The executor asserts
  `encodes ≤ 10`.

**Algorithm:**
1. **s1:** sample at the target's settings. If the estimate is ≤ soft, go to the full encode.
2. **Lossy rungs** (free, gifsicle only): re-optimise s1's GIF at lossy L+30, then L+60 (capped
   at 200), until the estimate is ≤ soft.
3. **s2…s5:** one sample per rung, in order, applying any lossy level already reached:
   - dither: error-diffusion → bayer 4 → bayer 5 (`none` stays `none`);
   - colours ×0.75 per step, down to 128;
   - fps through {20, 15, 12, 10}, down to `floor_fps`;
   - width ×0.85, rounded down to even, down to `floor_width`.

   A rung that is already at its floor is skipped and costs nothing. The search stops as soon as
   the estimate is ≤ soft.
4. **s6, the floors:** if still over soft after s5, sample once with every setting at its floor
   and the maximum lossy level.
   - If even that is over **hard**: `BUDGET_UNREACHABLE`, with
     `fits_secs = D × (hard − header − first_frame) / (estimate − header − first_frame)` (no
     extra margin) and `best_bytes = estimate`.
   - With `allow_trim`: trim the range to `fits_secs` from its start, and continue to the full
     encode at the floors.
5. **s7, interpolation:** if the floors fit, the best width lies between the floors and the last
   rung. Sample once at `W = floor + (W_last − floor) × sqrt((soft − e_floor)/(e_last − e_floor))`,
   rounded to even, and keep whichever of s6/s7 fits with the larger width.
   - [V] 2026-10-25 (T050): as built, width² is interpolated instead,
     `W = sqrt(Wf² + (W_last² − Wf²) × (soft − e_floor)/(e_last − e_floor))`, because the size
     grows with the area. On Busy10 → 3 MB the formula above picked 432 px (3.35 MB, over soft:
     a wasted sample); the area form picks about 380 px.
6. **Full encode.** If the real size is over **soft** (not only over hard) and the spare budget
   allows, take one more step down (the next rung, or the floor for width) and re-encode once.
   - Over soft but under hard after that: keep it, with `OVER_RECOMMENDED`.
   - Over hard: delete it, then `BUDGET_UNREACHABLE` (FR-011: never written).

**Properties:**
- **Determinism:** the ladder is fixed and the decisions depend only on byte counts. Samples are
  deterministic for the built-in engine (R1), so repeat runs take the same steps and produce the
  same bytes (US2 scenario 4).
- **Record:** each step is stored as `{n, kind: sample | post | full, settings, bytes, estimate}`.
  Every setting lowered from the target's default is listed (FR-012).
- **Static floor check in the planner** (code review M11): a budget below 64 KiB is rejected at
  plan time with `BUDGET_UNREACHABLE`, `detail.min_bytes` = 65536 (exit 3). Anything else is
  decided by the search (step 4), before the full encode, which still satisfies the edge case
  "rejected before encoding". `allow_trim` skips the static check.
- **CLI layering** (code review M4): `--budget X` without `--hard-limit` sets **both** soft and
  hard to X. The manifest `budget: {soft}` without `hard` does the same. The target's own pair
  applies only when neither is given.

## R6. Reddit MP4

- **Decision:**
  - `libx264 -preset slow -crf <target mp4.crf> -profile:v high -pix_fmt yuv420p -movflags +faststart -tag:v avc1 -fflags +bitexact`,
    with `-an -sn -dn -map_metadata -1`.
  - Even dimensions, and the width as for GIF.
  - The rate is the source rate, capped at the target's `mp4.max_fps`. MP4 has no centisecond
    limit, and smoothness is the point of the MP4 output.
  - Budget: a CRF ladder +3, +6, +9 (samples), then width ×0.85 to the floor. The same sample,
    count and retry rules as R5 apply. Lossy, dither and colours don't apply.
- **Determinism:** x264 output depends on the thread count, so determinism is per machine, as in
  002 R11. `+bitexact` removes the encoder-version tag drift.
- **Measured [V]:** the 4 s `testsrc2` clip at 640 px and 15 fps:
  - GIF: 1 329 332 B.
  - MP4 at CRF 20: 269 613 B (4.9×); CRF 23: 182 753 B (7.3×); CRF 26: 120 359 B (11×).
  - CRF 23 is the default.
- **SC-006 test definition:** the comparison is at **the same width and frame rate**, with the MP4
  forced to the GIF's fps. It uses the busy and testsrc fixtures. Flat or static content, where a
  GIF is already tiny, is outside SC-006's "test set". The quickstart says so.
- **"MP4 would be smaller" note** (US3 scenario 2): for `reddit-post` GIF jobs, the dry run and
  the outcome carry an MP4 size estimate from one MP4 sample of the same windows. That is one
  extra encode, counted.

## R7. Process model

- **gifsicle post-pass:** `gifsicle -O3 [--lossy=N] -o out.gif -- in.gif`, through
  `exec::process::run_lines_in`, with cwd = scratch.
  - There is no `--colors`: re-quantising without dither would undo the palette.
  - The input is always the built-in engine's own output, inside scratch.
- **gifski:** ffmpeg `<chain> -pix_fmt yuv444p -f yuv4mpegpipe -` piped into
  `gifski --fps F --quality Q [--lossy-quality L] [--motion-quality M] --repeat <R15> -o out.gif -`
  (parameters in R14).
  - y4m on stdin is gifski's default `binary` feature [V crates.io features, 2026-10-23].
  - [V] gifski accepts 4:4:4 y4m (`C444 XYSCSS=444 XCOLORRANGE=FULL`): the gif-tools smoke test
    on Linux and Windows, and T013. No `yuv420p` fallback is needed.
  - [V] T043, 2026-10-25, from the pinned 1.34.0 source (`src/bin/y4m_source.rs`): `C444` is
    read. The range comes from the header's `COLORRANGE=` (ffmpeg 6.1 and 8.x write
    `XCOLORRANGE=FULL` for `out_range=pc`), but the matrix is guessed from the frame size
    (BT.601 up to 720×480, BT.709 above) unless `--y4m-color-override` is given. As built: the
    chain ends `scale=out_color_matrix=bt709:out_range=pc,format=yuv444p`, and gifski gets
    `--y4m-color-override bt709`, so small GIFs are not shifted. Its ffmpeg writes progress
    with `-progress file:progress.log`. `--fps` is rounded up to 3 decimals, because gifski
    drops a frame whenever the stream runs ahead of `--fps`.
- **Refactor B3 rewrites `exec/pipeline.rs`** (code review H6):
  - `Stages` holds `Vec<StageSpec { program, args, role: Producer | Filter | Consumer, progress:
    ProgressSource }>`. The 002 AI pipe is decode (producer) → worker (filter) → encode
    (consumer). The GIF pipe is ffmpeg (producer) → gifski (consumer).
  - **Progress:** a producer whose stdout carries frames gets `-progress progress.log` (a file in
    scratch), which the executor tails every 250 ms. The AI pipe keeps reading progress from its
    consumer's stdout.
  - **Blame:** a non-zero exit of a later stage (consumer or filter) is the cause, and a
    producer's EPIPE/SIGPIPE is ignored. If only the producer fails, it is the cause, and the
    consumer's output is discarded even if the consumer exited 0 (a truncated GIF on EOF). The
    AI worker's error-code table stays as a filter-specific refinement.
    - As built (T021, 2026-10-24): for the 2-stage pipe, a producer that failed **on its own**
      (not SIGPIPE, and its stderr does not report a broken pipe) is the cause even when the
      consumer also failed, because a consumer fed a cut-short stream fails with a misleading
      message. Otherwise a failed consumer is the cause. Stage exits are polled every 25 ms.
  - **Kill:** on Linux, stage 0 leads the process group and the others join after its pid is
    known. On Windows, each child has its own kill-on-close Job Object (as today,
    `process.rs:155-158`), and cancel kills them in order, consumer first.
  - **Tests that must stay green:** `us6_pipeline.rs` and the `blame` unit tests, plus new
    2-stage cancel and blame tests on both OSes.
- **Pipe invariants** (security review M3):
  - The stderr of both tools is read with `next_line_bounded` and stripped of control
    characters.
  - A watchdog aborts the encode when the temp file passes 2× hard, or 4 GiB without a budget.
- **Argument safety:**
  - Only generated names (`in.gif`, `out.gif`, `p-<key>.png`) reach either tool, relative to
    cwd = scratch. Never a source name, so no leading `-` and no gifsicle `#N` selectors.
  - gifsicle never reads a user-supplied GIF.
  - The children run with a cleared environment plus an allowlist (`SystemRoot`, `TEMP`/`TMP`
    on Windows, `LANG`).
- **Lookup** (FR-024, security review M1, code review M13): a new **optional** lookup mode.
  - The order is: explicit setting or flag → `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI` → bundled → `PATH`.
    `PATH` is used only when no bundled copy exists, and is labelled `source: path`.
  - An explicit or environment path that is missing or fails the banner check makes the engine
    **unavailable, with a reason**. It is not a hard error, except under `--strict-engine`.
  - Results are canonicalised and absolute. Paths inside scratch, the input folder or the
    current directory are refused.
  - A `--version` banner check (`^LCDF Gifsicle 1\.\d+`, `^gifski 1\.\d+\.\d+`) runs with a
    5 s timeout and bounded output.
  - The environment variables are trusted at the level of the user's own shell.
- **Warnings** (code review M13): `ENGINE_FALLBACK` is emitted only when the missing engine
  mattered: gifski was requested or the target's engine is `best`, or the search wanted a lossy
  rung and gifsicle was absent. A plain run without gifsicle just skips the post-pass, and the
  outcome shows `optimiser: false`.
- **Failure attribution:** `Error::EngineFailed` gains `tool: String`, with a `"ffmpeg"` default
  via a constructor, so the existing struct literals and the "ffmpeg failed:" text are unchanged
  for ffmpeg. The JSON `code` stays `ENGINE_FAILED`.
- **Fallback (FR-023):**
  - gifski chosen but unavailable: the built-in engine is used, with `ENGINE_FALLBACK`.
  - With `--strict-engine`: `ENGINE_MISSING` (exit 4).
- **CPU use of desktop estimates:** `gif_estimate` runs outside the run's job slots, at most one
  at a time (R13). While a run is active, the desktop pauses live estimates and shows the last
  one.

## R8. gifsicle version and build

- **[V]**
  - lcdf.org lists `gifsicle-1.98.tar.gz`, sha256
    `e9d4cc0816db95c64d32e79f778836db0f1f7eb15868490bd330104e5146b66e` (downloaded 2026-10-23).
    The tag `v1.98` is dated 2026-10-15.
  - COPYING is GPL-2.0.
  - The tarball has autotools (`configure`) and `src/Makefile.w32`, an nmake makefile for MSVC
    (`cl`, `setargv.obj`).
  - NEWS lists `--lossy` fixes in recent releases.
- **Decision:**
  - Linux (ubuntu-22.04): `./configure --disable-gifview --disable-gifdiff && make`, then strip.
    The binary depends only on glibc 2.35 or later, the same baseline as the bundled build
    target.
  - Windows (windows-latest): `nmake -f Makefile.w32` from a VS developer shell, with
    `/O2 /MT`, so there is no VC++ runtime dependency. That avoids the open VC++ runtime finding
    from 002.
  - [U] Makefile.w32 may need `config.h` from `win32cfg.h`. The workflow copies it, and the first
    workflow run is the check.
- **Alternatives:**
  - eternallybored.org Windows builds: third-party, unpinned provenance (research/05 §3).
  - MSYS2/MinGW static build: kept as a fallback if nmake fails.

## R9. gifski version and build

- **[V]**
  - crates.io: max stable 1.34.0 (2025-07-13), `AGPL-3.0-or-later`, crate checksum
    `c246c795a61d4a2476fb1c8ab70bedfaa825c734882adc40e117fc837df81190`, MSRV 1.74.
  - Default features: `gifsicle` (its built-in lossy LZW), `binary` (clap, y4m, png).
  - `video` (ffmpeg-sys) is off by default.
  - The GitHub release ships only a source `.tar.xz`, no binaries.
- **Decision:**
  - Download the `.crate` from `static.crates.io` (sha256 pinned to the crates.io checksum).
  - Build it with `cargo build --release --locked` on both OSes, using the crate's own
    `Cargo.lock`, with `-C target-feature=+crt-static` on Windows.
  - Default features only, so no FFmpeg is linked.
- **Hardening** (security review H1): the crate's own `Cargo.lock` is pinned by hash, and
  checked with `cargo deny` and `cargo audit` at pin time. Its dependencies are vendored once
  into a mirrored tarball.
  - The build job runs `--frozen --offline` with a fresh `CARGO_HOME`, and no tokens or caches.
  - It refuses a crate that ships `.cargo/` or `rust-toolchain*`.
  - Details: [contracts/gif-tools-lock.md](./contracts/gif-tools-lock.md).
- **Alternatives:** `cargo install gifski@1.34.0 --locked` resolves the same lock, but downloads
  outside the pin and mirror path, and runs dependency build scripts with network access.

## R10. Licences, notices and the release gate

- **Decision:**
  - Both engines run as separate executables, an aggregate as with FFmpeg (research/05 §3).
  - `licenses/GIFSICLE-NOTICE.md` and `licenses/GIFSKI-NOTICE.md` name the version, the
    licence, the upstream URL and the **mirrored source** (the pinned tarball and crate on the
    mirror release). That is the source offer.
  - The licence texts are copied from the sources by the workflow.
  - `check-pins` verifies the notices and the mirror, as it does for FFmpeg.
- **Release gate:** `cargo xtask check-release-gate` runs in every `release.yml` job, for both
  tag pushes and `workflow_dispatch`, because downloadable artifacts are distribution too.
  - It fails when `binaries/rmedia-gifski*` is staged and the root `Cargo.toml` lacks a recorded
    licence decision: a root `LICENSE` plus `[workspace.metadata.rmedia] app_licence`, with an
    SPDX id from an allowlist.
  - It checks the staged file, not the lock, and xtask tests cover it.
  - Today no decision exists: the manifests' `MIT OR Apache-2.0` is a placeholder.
  - CI does not run `release.yml`, so it is unaffected.
  - Interim artifacts can be built with `fetch-tools --no-gifski`.
  - **Existing release lines are unaffected** (code review M12). The gate only fires when the
    engine is staged, and staging needs `gif-tools.lock.json`, which does not exist at the
    `v0.1.0`/`v0.2.0` commits. Tagging those commits works as before.
- **Not legal advice:** the owner decides the app licence (roadmap item). If it ends up being
  incompatible with distributing gifski next to it, the bundle drops gifski and keeps detection,
  which is a one-line change in the lock file.

## R11. Verification

- **Decision:** after each full encode, the temp output is checked before commit (FR-018):
  - GIF:
    - the `GIF89a` header;
    - the loop matches the request through the R15 table. For "forever" or N ≥ 2 plays, the
      NETSCAPE2.0 block holds the repeat count; for 1 play there is no block. (A [V] GIF had the
      block at byte 784 with repeat 0.)
    - packet durations from `ffprobe -show_entries packet=duration` are all ≥ 2;
    - the frame count is within ±1 of the plan;
      - [V] 2026-10-25: gifsicle `-O3` merges identical consecutive frames and adds up their
        delays (Static6: 3 frames for 90 planned; a VFR source 75 for 90). After the post-pass
        the frame count may only drop (≤ plan + 1), and the total display time must match the
        plan within one frame instead.
    - the width and height match;
    - the size is ≤ hard.
  - MP4:
    - no audio, subtitle or data streams;
    - `moov` before `mdat` (faststart);
    - width, height and duration within one frame.
- The testkit gets the same checks as helpers (`gif_delays`, `gif_loop`, `mp4_faststart`).

## R12. Desktop playback

- **Decision:**
  - The media server's `content_type()` gains `gif → image/gif`. A finished GIF plays in an
    `<img>`, already allowed by `img-src http://127.0.0.1:*`. A Reddit MP4 plays in `<video>`.
  - The new `open_output(job_id)` registers only outputs of the current run, using the
    eviction pattern of `register_preview`.
  - The estimate uses `gif_estimate` (one sample), cancellable and debounced in the store.

## R13. Untrusted inputs to the new surfaces (security review M2, M4, M5)

- **User target files:**
  - They are loaded with `read_preset_file`'s protections: regular files only
    (`symlink_metadata`), at most 64 KiB, a bounded read, at most 64 files.
  - `name` and `description` are length-capped (schema) and refused if they contain control
    characters.
  - Rules checked in code:
    - `soft ≤ hard ≤ 1 GiB`;
    - `floor_width ≤` both widths.
  - A user target may override a built-in id, as presets do. `--list-targets`, `doctor` and the
    desktop picker label it "user override".
  - Only `id` (`^[a-z0-9-]+$`) ever reaches a file name, through `{target}`.
- **Desktop settings:** `set_settings` copies `gifsicle_path` and `gifski_path` back from the
  stored settings, as it does today for `ffmpeg_path`. A page-supplied value is ignored, and a
  bridge test asserts it.
  - Engine paths are set only through `pick_engine(kind)`, with kind `ffmpeg|gifsicle|gifski`.
    The picker refuses symlinks and folders, and runs the banner check before saving.
  - `gif_defaults` goes through the same validation as CLI flags.
- **`gif_estimate`:**
  - There is one estimate in flight. A new request kills the previous process tree and waits for
    it before starting.
  - The scratch folder is a fresh 0700 tempdir under the app cache. It is removed on completion,
    cancel, error or supersession; stale ones are removed at startup. It has the same size
    watchdog.
  - `estimateId` is only a map key, never part of a path.
  - The core clamps all options again, and bounds the sample to 3 s and 150 frames.
- **CLI sizes:** `--budget` and `--hard-limit` parse with checked arithmetic. They reject
  negative, NaN or overflowing values, and anything above 1 GiB.
- **Dry run:** with sampling it executes ffmpeg (and gifsicle) on the input. The help text says
  so, and `--no-sample` avoids it.

## R14. gifski parameters and determinism (code review M1)

- **Target fields:** an optional `gifski` object `{ quality: 1..=100, motion_quality?: 1..=100,
  lossy_quality?: 1..=100 }`. `max-quality` ships `{ quality: 100 }`.
- **Ignored settings:** `colours`, `dither`, `stats_mode` and the gifsicle `lossy` don't apply to
  gifski. If the user sets them with gifski, they get a note ("--colors is ignored by gifski").
- **gifski budget ladder:** `lossy_quality` 100 → 80 → 60 (samples), then fps, then width, with
  the same counting, floors, interpolation and retry as R5. Samples are y4m windows piped
  through gifski.
- **[U] determinism:** gifski is multithreaded. Phase 0 encodes the same input twice and
  compares the bytes.
  - If they match: SC-007 and US2 scenario 4 hold for gifski too.
  - If not: the gifski outcome is marked `deterministic: false`, the byte-identity tests are
    limited to the built-in engine, and the plan's Constitution Check notes the exception
    (Principle II). Size bounds and verification still apply.
  - The same Phase 0 check covers gifsicle `--lossy` (expected deterministic) and x264 at a
    fixed thread count.
  - [V] T013, 2026-10-25, pinned builds, 12-thread Linux machine: gifski 1.34.0 gave identical
    bytes 5 times out of 5 (720×405 mandelbrot, quality 90) and 2 out of 2 with motion and lossy
    quality 60; gifsicle 1.98 `-O3 --lossy=80` 3 out of 3; x264 at `-threads 4` and at the
    default thread count, twice each. So `deterministic` is true for every engine, and
    `a_gifski_gif_is_byte_reproducible` keeps the check. Not measured: the same bytes across
    machines with different core counts.

## R15. Loop count and transparency

- **Plays → repeats** (FR-001, code review M7):

  | Request | ffmpeg gif muxer | gifski | NETSCAPE block |
  |---|---|---|---|
  | `forever` | `-loop 0` | `--repeat 0` | present, repeat 0 |
  | N plays (N ≥ 2) | `-loop N-1` | `--repeat N-1` | present, repeat N-1 |
  | 1 play | `-loop -1` | `--repeat -1` | absent |

  gifsicle keeps the block as it is (`-O3` does not touch loop extensions). Verify checks this
  after the post-pass.
  - [V] T043, 2026-10-25: gifski 1.34.0 maps `--repeat -1` to `Repeat::Finite(0)`, and gif
    0.13.3 writes that as a NETSCAPE block with count 0, which loops forever. For 1 play, the
    executor removes that exact 19-byte block (at offset 13: gifski writes no global palette)
    and leaves any other layout alone, so verification still catches a change.
- **Alpha** (edge case "transparency", code review M8): when the probe reports `has_alpha`, the
  frame chain flattens the source onto opaque black before scaling:
  `split[fa][fb];[fa]format=yuv420p,drawbox=c=black:t=fill[fk];[fk][fb]overlay=format=auto`.
  - [V] T005(e), 2026-10-24: the black base is a copy of the source, so it has the source's
    timing (a `color` source would set the output rate). It must drop the alpha plane first,
    or `overlay` keeps the base's alpha and the GIF is transparent. `premultiply=inplace=1`
    was rejected: on `yuva420p` it changed opaque colours ((0,0,254) became (13,13,156)).
  - The Alpha fixture's hidden pixels are red, so a flatten that ignores alpha shows red.
  - The output is opaque.
  - `reserve_transparent=1` stays on: paletteuse uses that index only for unchanged-pixel diffs,
    never for source alpha.
  - A testkit alpha fixture (`yuva420p` VP9/WebM) checks that no output pixel is transparent
    against the first frame.

## R16. Contracts, reframe, naming and chaining

- **JSON compatibility** (code review H5):
  - `PlannedJob.warnings` stays `[string]`, unchanged for 001/002 (insta snapshot).
  - GIF jobs add a parallel `notices: [{code, message}]`.
  - `Event::JobWarning` gains an optional `code` (additive).
  - `ErrorInfo` gains an optional `detail` object, and `Error` gains
    `BudgetUnreachable { fits_secs, best_bytes, min_bytes }`.
  - `PlannedJob::commands() -> Vec<Command>` lists every process of a job, and `args()` keeps
    returning the final encode's argv (the GIF encode, or the MP4 encode). The dry run's `argv`
    is that command; `pipeline` lists all of them, as for the AI tier.
- **Smart reframe with GIF** (code review H4):
  - Analysis runs once per job at the target's **starting** fps, keyed exactly as 002 keys it.
  - Lowering fps in the search does not invalidate it: the sendcmd commands are timestamps
    relative to the clip start, and they apply in the frame chain before the fps cap.
  - Sample windows get their own sendcmd files with `cmd_offset` = the window start (R4).
- **Refactor B2:** `ReframeInput` carries what `Prepared::new` and `analyze_cached` really read:
  - `cut` (for `input_args`), and the `GraphInput` pieces needed by `analysis_graph`;
  - the detector choice, the cache directory, `analysis_size`, `fps_out`, the crop size and
    `cmd_offset`.
  - `lookup_cached` takes it too.
  - The cache key is computed from the same `analysis_graph` string, so existing entries stay
    valid. A test asserts that the key is unchanged for a 002 job.
- **Chained `--gif --upscale`** (code review M10): the chain applies 002's **pre-processing and
  reframe geometry only**, directly at the GIF size. It never runs an upscale tier, because GIF
  sizes are below typical sources (FR-004).
  - The upscale preset contributes the shape and the reframe kind.
  - To GIF an AI-upscaled master, upscale first and pass its output to `rmedia gif` (FR-014: it
    is then downscaled).
  - A note says "upscale skipped: GIF width is below the source".
- **Output names** (code review M2), mirroring `request.rs:10-17`:
  - `GIF_NAME_TEMPLATE = "{source}_{target}.{ext}"` (whole video);
  - `GIF_CLIP_NAME_TEMPLATE = "{source}_{index:03}_{name-or-range}_{target}.{ext}"`;
  - `GIF_MULTI_CLIP_NAME_TEMPLATE` for many sources into one folder.
  - US5 tests cover two clips from one video landing in two files.
