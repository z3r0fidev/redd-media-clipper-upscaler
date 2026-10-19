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
  x86_64-pc-windows-gnu --workspace --all-targets` (the `CC` override lets tract-linalg's build
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

