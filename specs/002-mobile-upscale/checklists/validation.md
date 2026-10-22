# Validation log: 002 Mobile Upscaling & Smart Reframe

## Baseline before 002 (T001)

- Date: 2026-10-14, commit 649f925, bundled engine n9.0.2, `RMEDIA_REQUIRE_FFMPEG=1`.
- `cargo test --workspace`: **108 passed, 0 failed, 2 ignored** (benches).
- Every Phase 2 refactor must keep these passing.

## US1 checkpoint (2026-10-14)

- SC-002 (CPU, synthetic fixtures): +2.78 VMAF at 480p, +2.89 at 360p over bicubic (bar +1.5).
- SC-003 bench: 60 s 480p30 → 1080x2340 in 101.5 s (bar 120 s).
- Determinism: the same CPU job twice gives identical bytes.
- Cancel (SC-006): an upscale cancels in < 2 s with no `.part`, scratch dir or engine process left.
- **Owner items:**
  - +3 VMAF over bicubic on real low-quality footage (SC-002 owner check).
  - Two-phone gallery test of `upscaled/*_phone-fill.mp4` (SC-001).
- **Review items accepted as known limits (not fixed in US1):**
  - HDR kept without mastering metadata (a warning says so).
  - A pre-existing symlinked `upscaled/` folder is followed (names are run-specific; no
    overwrite through links).
  - Rare colour tags (smpte240m, film) fall back to BT.601/709 defaults.
  - Very long source names can exceed 255 bytes in temp names.

## US2 checkpoint (2026-10-16)

- SC-004 (MovingMarker, marker detector): marker fully inside in 100% of 300 frames, never out;
  crop steps within the speed limit (+2 px for even rounding) except at the cut.
- SC-005 (release, 60 s 1080p): analysis 10.7 s vs centre render 79 s (13.5%; bar 50%).
- Real YuNet (ignored test): a public-domain NASA portrait panning across 1920x1080 stays inside the
  crop; manual quickstart US2 render checked by eye (face centred at 1/3/5/7 s).
- Engine: sendcmd toggles of `overlay@fb` land on exactly the commanded frames through the full graph.
- **Known limits:**
  - Pans horizontally only; taller sources get a centre crop.
  - Clips mixing tracked and fallback sections render both branches throughout (slower).
  - A histogram-only cut (scdet below 10) can start up to 1/3 s late.
  - `--no-analyze` only affects dry runs; a render analyses when needed.

## US3 checkpoint (2026-10-19)

- Laptop (Intel RPL-U iGPU + llvmpipe, bundled n9.0.2): `rmedia doctor` lists the Intel GPU as
  usable and llvmpipe as "software renderer"; self-tests CPU 33–36 fps, GPU 21–29 fps; first
  check 4.2 s, cached 0.13 s; `--recheck` rewrites `checked_at`.
- GPU quality (480p fixture → 1080x1920, VMAF): GPU 57.66, CPU 57.63, bicubic 54.85 with
  `ewa_lanczossharp:sigmoid=0` + `cas=0.7`. The libplacebo defaults scored 53.86 (under bicubic).
- SC-003 (60 s 480p30 → phone-fill, release): CPU 42.3 s, forced GPU 44.6 s. The iGPU is slower
  because x265 dominates both tiers, so `auto` stays on the CPU here (SC-003 amended: auto never
  picks a slower GPU). A discrete GPU is expected to pass the GPU ≥ CPU path of the bench.
- SC-006: a GPU-tier cancel stops within 2 s with no `.part` and no demotion.
- SC-008: a GPU decision pinned to a missing device fails, is retried on the CPU (warning,
  `tier_fallback`, monotonic progress), and the GPU is demoted (fixed text, no paths); the next
  plan picks the CPU. A forced GPU fails without falling back or demoting.
- Forced GPU output: 1080x2340 HEVC `hvc1`, BT.709 tags. With `VK_DRIVER_FILES`/`VK_ICD_FILENAMES`
  pointed at a missing file: `--tier gpu` exits 3 `TIER_UNAVAILABLE` ("no Vulkan device").
- Windows: `CC_x86_64_pc_windows_gnu=gcc AR_x86_64_pc_windows_gnu=ar cargo check --target
  x86_64-pc-windows-gnu --workspace --all-targets --exclude rmedia-sr` (the `CC` override lets tract-linalg's build
  script assemble without MinGW; nothing is linked) with
  `-D warnings` passes (registry code compiles); not run on Windows hardware.
- **Known limits:**
  - Windows driver fingerprint misses drivers registered only under the display-adapter key
    (`doctor --recheck` after a driver update).
  - A verify failure on the GPU tier is not retried on the CPU.
  - GPU output may differ slightly between GPUs and drivers.

## US4 checkpoint (2026-10-19)

- quickstart › US4 (bundled n9.0.2, CPU tier): `rmedia batch m.json` → `upscaled/low_phone-fill.mp4`
  1080x2340 hvc1, `clips/land_001_pan_reddit-916.mp4` 1080x1920 avc1,
  `upscaled/slides_phone-fill.mp4` 1080x2340 hvc1 in 36.6 s; `rmedia clip land.mp4 -s 0 -e 4
  --upscale --preset reddit-feed-45` → 1080x1350 avc1 in 4.2 s. No `.part`, scratch folder or
  intermediate clip left.
- Partial clips (MP4 with chapters, MPEG-TS starting at 10 s; 2–6 s): duration 4.0 s ± 0.05,
  first video frame at 0, audio within one frame of video, chapters dropped. One render stage per
  job.
- One video failing (source removed after planning) → the other completes; summary 1/1, exit 1.
- Reviews: senior-code-reviewer (0 critical/high; 2 medium fixed: `batch --upscale` flags lost
  under an `enabled:false` → re-enable, unused `batch` flags silent) and security-auditor (0
  critical/high; 1 medium fixed: every scan now skips both `clips/` and `upscaled/` plus `-o`).
- **Known limits:**
  - A CSV row cannot say "the preset's crf": an empty `upscale_crf` inherits a CLI `--crf`.
  - Folders named `clips` or `upscaled` inside a scanned tree are always skipped (a note says so;
    pass the folder directly to use it).
  - Dry runs may run the machine check (once per hardware or app change), as `upscale` already did.

## US5 checkpoint (2026-10-19)

- quickstart › US5 on Linux X11 (a private Xvfb display, debug build, bundled n9.0.2):
  - Opened `land.mp4` (moving marker): the YuNet model finds no face in a magenta square, so the
    whole video is marked as a blurred-background range (hatched on the timeline, note on the
    video). Correct behaviour for that fixture.
  - Opened `face.mp4` (a real face moving left to right, 8 s): "Upscale for phone" shows the
    before/after at 1080x2340 and the crop rectangle on the face; at 6 s the rectangle has
    followed it. Analysis plus first still in a few seconds; `us5_preview.rs` measures ≈1 s for a
    1080p still.
  - "Upscale now" → `upscaled/face_phone-fill.mp4` HEVC hvc1 1080x2340; run panel "Done · CPU
    tier". `rmedia upscale face.mp4 -o cli/` gives the same sha256 (FR-022), as does the bridge
    test with an exported manifest through `rmedia batch`.
  - Not checked by hand: Wayland, Windows. The overlay geometry follows `object-fit: contain`
    from the analysis' display size.
- Reviews: senior-code-reviewer (0 critical; 2 high fixed: the analysis no longer depends on the
  tier, and previews are cancelled when superseded or on close; 6 medium fixed: stale stills
  cleared on source change and on leaving the view, preview errors retried, GPU preview retried
  on the CPU, sync waits for the source and the saved choices, "Upscale now" checks it can run
  before queueing, analysing state reset), security-auditor (0 critical/high; 3 medium fixed:
  one analysis at a time with id checks, one preview at a time, preview URLs capped and evicted
  stills forgotten by the media server; low fixed: time inputs validated, settings saves
  serialised, `jobs` clamped to 1–64, `.part` sweep and LRU touch), premium-ui-designer
  (accessibility and copy applied: toggle-button groups with `aria-pressed`, visible slider
  focus with value text and PageUp/PageDown, plain-language labels and notes, legend for
  hatched ranges, analysis errors with **Try again**).
- **Known limits:**
  - The "before" still uses `zscale`; an ffmpeg without it (never the bundled one) cannot preview.
  - CSP `img-src`/`media-src http://127.0.0.1:*` allow any local port (accepted; see the
    security model).
  - Models from a chosen models folder are hashed into the cache key but not pinned to a known
    hash.
  - Each smart preview reads the face model to compute the cache key (fast for YuNet).
  - The `< 5 s` preview assertion is wall-clock based and may need slack on a loaded CI runner.

## US6 checkpoint (2026-10-20)

- Machine: Intel Raptor Lake-P iGPU (Mesa ANV) + llvmpipe; no discrete GPU. Bundled ffmpeg n9.0.2.
- `rmedia-sr --list-gpus`: the iGPU as `integrated`, llvmpipe as `cpu`/`software`; forcing the
  llvmpipe index exits 3 `SOFTWARE_DEVICE`. Worker binary depends only on glibc (`ldd`).
- `rmedia doctor --recheck`: `tier: ai no, 0.2 fps (too slow on Intel(R) Graphics (RPL-U)
  (integrated): about 177 min for 60 s of 480p (limit 10 min))` (debug build; a release-build
  self-test measured ≈ 8.6 s/Mpx, ≈ 106 min). `rmedia upscale --tier ai` exits 3
  `TIER_UNAVAILABLE` with that reason.
- Pipeline (`us6_pipeline.rs`, builtin nearest model, runs everywhere): 1080x2340 HEVC output,
  durations and A/V offsets within one frame for input-seek, -copyts trim, whole-video (audio
  offset kept within 17 ms), VFR and interlaced sources; cancel stops all three processes within
  2 s with no `.part` or scratch left; a crashing worker is `ENGINE_FAILED` with its message;
  AI-vs-CPU framing PSNR 49.6 dB (smart), 52.9 dB (partial fallback), 61.2 dB (blur), and
  breaking the crop path or the fallback toggle drops it under 30 dB.
- Real model on the iGPU (`us6_ai.rs --ignored`, gate bypassed): SC-002 AI 51.84 vs GPU tier
  43.73 VMAF on the degraded 360p fixture (+8.1, target +3); static-source flicker AI 1.105 vs
  GPU 1.900 (source 2.474) before the anti-flicker step. The 10 s render took 496 s here.
  Owner re-run on the same iGPU (2026-10-22, release, bundled FFmpeg, with the anti-flicker
  step): SC-002 AI 51.78 vs GPU tier 43.73 (+8.04), render 402 s; flicker AI 0.731 vs GPU 1.900
  (source 2.474). SC-009 was stopped on purpose (about 106 min predicted on this iGPU).
- Owner run on a discrete GPU (2026-10-22, Quadro M4000 8 GB, Maxwell, PCI passthrough to an
  Ubuntu 24.04 VM, driver 580; release; FFmpeg n9.0.2-22): all three `us6_ai` tests pass.
  - SC-002: AI 50.40 vs GPU tier 42.23 VMAF (+8.17, target +3); the AI render took 77 s.
  - Flicker: AI 0.746, GPU tier 1.906, source 2.506.
  - SC-009: 60 s of 480p (portrait 480x854, default preset) took 600 s against the 600 s limit,
    predicted 605 s (0.8 % off; ±30 % allowed). Self-test 0.999 s/Mpx. The M4000 (2015) sits
    exactly at the bound, so any current mid-range card should pass with margin.
  - The availability gate is stricter than this job: it prices the full 854x480 frame (0.41 Mpx,
    the blur-fit worst case), while the default preset crops this portrait source to 394x854
    (0.34 Mpx). For the M4000 it predicts 737 s and does not offer AI.
    **Owner decision (2026-10-22): keep the worst-case gate.** It never offers AI on a card that
    would exceed 10 minutes in some framing mode.
  - The GPU tier measured 6.2 fps against the CPU's 7.1 fps here, so auto picks the CPU tier.
- **Owner checks (discrete GPU):** build the worker, then run the tests with the bundled FFmpeg
  (the system one usually lacks libvmaf; a missing feature now fails the run):
  `cargo build --release -p rmedia-sr` then
  `RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu cargo test --release -p rmedia-core --test us6_ai -- --ignored --nocapture --test-threads 1`
  (SC-002 AI, flicker, SC-009 ≤ 10 min and prediction within ±30 %), then
  `rmedia doctor --recheck` should show `tier: ai yes`. Windows: CI's Windows job is the first
  build of the MSVC-linked worker (CRT/OpenMP linking unverified here).
- Reviews: senior-code-reviewer (0 critical; 1 high fixed: a stage that had already exited was
  marked killed, so a worker error could be reported as ffmpeg #1's broken pipe; 3 medium fixed:
  an early clean ffmpeg #2 exit no longer fails the job, timing tests for the other seek paths,
  worker start errors reported). security-auditor (0 critical/high; 2 medium fixed: model files
  pinned by SHA-256 and checked on the bytes the worker loads, bounded stderr/progress line
  reads; low fixed: output size before allocation, null extractor, checked ROI arithmetic,
  single GPU instance, dup2 checked, 64 Mpx frame cap, sanitised worker messages, RMEDIA_SR
  logged, fresh release fetches, curl limits).
- **Known limits:**
  - The worker is crash-contained, not sandboxed (no seccomp/Landlock); pinned models limit what
    ncnn parses.
  - The worker inherits the environment (Vulkan loader variables, `LD_PRELOAD`), like ffmpeg.
  - AI previews use the standard scaler (flagged `classical`).
  - Smart reframe's fallback ranges are classically scaled on the AI tier (the background comes
    from the source; only the tracked crop goes through the worker).
  - A stuck GPU driver after a kill is waited for without a deadline.
  - ffmpeg #2's overlay queue on long blur/fallback clips was not measured for memory.


## Polish: quickstart and final validation (T111, 2026-10-22)

Linux x86_64 (Ubuntu, kernel 7.0), debug build, bundled FFmpeg n9.0.2-31 unless noted. The
scripted parts of `quickstart.md` were run end to end.

- **US1:** `low.mp4` → `hevc,hvc1,1080,2340,tv,bt709,bt709,bt709`, source unchanged (same sha256).
  `--preset reddit-916` → H.264 avc1 1080x1920 with `moov` before `mdat` (faststart). An output
  already at 1080x2340 warns "no upscaling needed". Ctrl-C after 1 s: exit 130 within 0.06 s, no
  `.part`, scratch folder or `rmedia-ffmpeg` process left.
- **US2:** the quickstart's marker source could not be built (a filter graph passed to `-i`, a
  source pad used twice), and a coloured marker is not a face for the real detector, so the
  quickstart now builds `land.mp4` from the public-domain test portrait. Result: `ready`, 2 shots,
  no blurred ranges, 1080x2340; the crop's left edge goes 74 → 300 → 540 → 720 during the pan
  (0, 2, 4, 5.5 s) and holds at 856 after the cut (the face is at x=900). `center` and `blur` run.
- **US3:** `doctor` lists cpu 27 fps, gpu 17 fps (Intel RPL-U), llvmpipe rejected; forced
  `--tier gpu` succeeds; `auto` picks the CPU with "measured slower than the CPU here (17 vs 27
  fps)"; with no Vulkan driver, `--tier gpu` exits 3 "no Vulkan device".
- **US4:** the manifest gives `low` and `slides` at 1080x2340 and the `pan` clip at 1080x1920;
  `clip --upscale --preset reddit-feed-45` gives 1080x1350; no intermediate files.
- **US5 / SC-007 (private Xvfb display :99, never the real display):** open `face.mp4` (1),
  **Upscale for phone** (2): analysis "Finding faces…", then "Following faces across 2 scenes"
  with the crop rectangle on the face and the before/after showing it, about 12 s for the 10 s
  clip in a debug build; **Upscale now** (3): "Finished · 1 succeeded",
  `upscaled/face_phone-fill.mp4` hvc1 1080x2340, byte-identical to `rmedia upscale` (FR-022).
  Observed: the job stays listed in the queue afterwards with **Run 1 job** enabled (running it
  again skips the existing output).
- **US6:** `doctor` on this iGPU: AI unavailable, about 127 min for 60 s of 480p (debug worker);
  forced `--tier ai` exits 3 with that estimate. The quickstart said it warns "too slow"; that
  warning applies only on a card that passes the gate, and the quickstart now says so. Discrete
  GPU: see the owner run (Quadro M4000) under the US6 checkpoint.
- **Minimum engine (FFmpeg 6.1.1, Ubuntu 24.04 distro):** the whole suite passes (290 passed,
  0 failed, 9 ignored; worker crate excluded as in CI), and `upscale` of `low.mp4` and `land.mp4`
  gives hvc1 1080x2340 BT.709. Before the polish fix, `parse_filters` read no filters from 6.x
  and upscaling was refused there.
- **Bundled engine:** the whole suite passes (299 passed, 0 failed, 10 ignored).
- **CI on GitHub (first runs since the remote was created):** fixed the FFmpeg 6.x filter list,
  the MSVC C runtime of the worker's shim (LNK2038), mingw for the Windows type-check, models for
  the minimum-engine job, the pinned engine for the desktop backend tests, the Windows path
  separators in the clip JSON snapshots, and documented RUSTSEC-2024-0370 in `deny.toml`.
