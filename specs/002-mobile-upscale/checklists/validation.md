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
