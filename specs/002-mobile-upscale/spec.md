# Feature Specification: Mobile Upscaling & Smart Reframe

**Feature Branch**: `002-mobile-upscale`

**Created**: 2026-10-06

**Status**: Draft

**Input**: User description: "Upscale low-quality videos to a format optimized for modern cellphone
displays such as iPhone 15–18 including Duo, Pixel 10 XL Pro, and similar devices released in the
past 5 years. Resolutions must be crisp and the video must fill the screen when held in portrait
(standard orientation)." Owner decisions (2026-10-06): smart reframe; tiered quality (classical on
CPU, GPU-accelerated when available, AI when a capable GPU is present — AI delivered last); FFmpeg
stays bundled; default output is "phone fill" (1080×2340); the AI tier uses a bundled worker that
runs on any major GPU vendor. Research: `research/02`, `03`, `06`, `07`, `08`, `09`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Make a low-quality portrait video crisp and full-screen on my phone (Priority: P1)

A user has a soft, low-resolution or heavily compressed video (for example a 480p or 720p clip saved
from the web, or a clip made with feature 001). They run one command, or press one button, and get a
new file that looks sharp on a modern phone and fills the screen edge to edge when played in
portrait in the phone's gallery app. They did not have to know the phone's resolution, codec or
colour settings.

**Why this priority**: This is the core promise of the feature. On its own it delivers value for
any source that is already roughly portrait, with no detection, GPU or AI work.

**Independent Test**: Upscale a degraded 480×854 test video with default settings. The output is
1080×2340, plays on the reference devices without bars, and objectively scores higher against the
clean original than a plain resize does.

**Acceptance Scenarios**:

1. **Given** a 480×854 source, **When** the user upscales it with defaults, **Then** a new file is
   created at the default "phone fill" size (1080×2340) in a format and tagging that iPhones,
   Pixels and Galaxy phones from the last 5 years play natively, and the source is unchanged.
2. **Given** the same source, **When** the user picks the "Reddit 9:16" preset, **Then** the output
   is 1080×1920 in the most widely accepted upload format.
3. **Given** a source whose aspect differs slightly from the preset (for example 9:16 into 9:19.5),
   **When** it is upscaled, **Then** the output still fills the frame with no black bars, using the
   chosen reframe behaviour (US2), with smart reframe as the default.
4. **Given** a source already at or above the preset's resolution, **When** it is processed,
   **Then** it is scaled down or reframed only (never enlarged and then shrunk), and the user is told
   that no upscaling was needed.
5. **Given** any run, **When** the user asks for a dry run, **Then** they see the planned output
   size, preset, reframe mode, quality tier and file name, and no output files are written (only the
   analysis and capability caches may be updated, plus a crop-path file when the user explicitly asks
   for one).
6. **Given** a running upscale, **When** the user cancels, **Then** processing stops within
   2 seconds and no partial output remains.

---

### User Story 2 - Turn a landscape video into a portrait one that follows the action (Priority: P2)

A user has a landscape (16:9 or 4:3) video, such as a gameplay clip, an interview or a sports
moment. They want a portrait version for their phone where the important part, such as the
speaking person, the player or the moving subject, stays in view. The crop moves smoothly like a
camera operator would, without jitter, and jumps only at scene cuts.

**Why this priority**: Most source material is landscape. Without reframing, "fill the screen"
means either cutting off the subject (center crop) or leaving large bars. This is the owner's
chosen differentiator.

**Independent Test**: Process a synthetic landscape video with a subject that moves across the
frame and one scene cut. The subject stays inside the output frame in at least 95% of frames, crop
motion stays within the speed and smoothness limits, and the crop changes abruptly only at the cut.

**Acceptance Scenarios**:

1. **Given** a landscape video with one person who moves left to right, **When** it is processed with
   smart reframe, **Then** the portrait output keeps the person in frame and the crop pans smoothly.
2. **Given** a video with a hard scene cut, **When** it is processed, **Then** the crop may reposition
   instantly at the cut but never drifts across it.
3. **Given** a section with no detectable subject, or with subjects too far apart to fit in one
   portrait frame, **When** it is processed, **Then** that section falls back to "fit with blurred
   background", so nothing important is cut off, and the run reports which time ranges fell back.
4. **Given** any landscape video, **When** the user chooses "center crop", "fit with blurred
   background" or "smart", **Then** the chosen behaviour is applied for the whole video.
5. **Given** a smart reframe run, **When** the user asks for a preview or dry run, **Then** they can
   see the planned crop path (where the frame will be over time) before committing to the full render.

---

### User Story 3 - Use my graphics card automatically, with a choice (Priority: P3)

A user with a capable graphics processor gets better or faster results without configuring
anything: the app detects a usable GPU, picks the best quality tier the machine can run, and says
which tier it used. Users without a GPU, or with an unreliable one, still get correct output
from the CPU tier. A user can force a specific tier.

**Why this priority**: Higher-quality scaling at no extra effort. It builds on US1 and only changes
how frames are scaled.

**Independent Test**: On a machine with a working GPU, the automatic choice is the GPU tier and the
output passes the US1 checks. With GPU use disabled or unavailable, the same command falls back to
the CPU tier, says so, and produces a valid output.

**Acceptance Scenarios**:

1. **Given** a machine with a usable GPU, **When** the user upscales with automatic tier, **Then** the
   GPU tier is used and reported.
2. **Given** a machine whose only "GPU" is a software emulation, **When** the tier is chosen
   automatically, **Then** the CPU tier is used.
3. **Given** the GPU tier fails mid-run (driver error), **When** the job fails, **Then** it is retried
   once on the CPU tier automatically, and the outcome notes the fallback.
4. **Given** the user forces a tier the machine cannot run, **When** they start, **Then** they get a
   clear error before any processing (forced tiers never fall back silently).
5. **Given** the app has checked the machine's capability once, **When** later runs start, **Then**
   they do not repeat the check until the hardware, driver or app version changes.

---

### User Story 4 - Upscale many videos at once and chain with clipping (Priority: P4)

A user upscales a whole folder, a list of files, or the clips listed in a 001 batch manifest in one
run. They can also clip and upscale in one step, so each clip comes out phone-ready. Options can
be set once and overridden per video, exactly as in feature 001.

**Why this priority**: This is where volume work happens. It reuses the 001 inputs, so the cost is
mainly integration.

**Independent Test**: A manifest with three videos, one of them overriding the preset, produces
three correctly sized outputs. A clip+upscale run on one source produces phone-ready clips without
leaving intermediate files.

**Acceptance Scenarios**:

1. **Given** a folder of videos, **When** the user upscales the folder, **Then** every video found by
   001's discovery is processed with the shared options, and non-videos are skipped.
2. **Given** a manifest with defaults and per-video overrides for preset, reframe mode and tier,
   **When** it runs, **Then** each output follows the override order defaults < video < clip.
3. **Given** a clip request that has upscaling enabled, **When** it runs, **Then** each clip is cut and
   upscaled in one pass, without the user needing an intermediate clip file.
4. **Given** a batch where one video fails, **When** the run ends, **Then** the others complete and the
   summary and exit status match the 001 conventions.

---

### User Story 5 - Preview and tune in the desktop app (Priority: P5)

In the desktop app, a user selects a video and opens an "Upscale for phone" view. They see a
before/after comparison of a frame. For a landscape source, they also see the planned portrait crop
drawn over the video and moving through time. They choose a device preset, a reframe mode and a
quality tier, then add the job to the same queue used for clips.

**Why this priority**: Visual confidence before a long render. It depends on US1–US3 being
available in the core.

**Independent Test**: Open a landscape video in the view. The crop-path overlay follows the subject
during playback, and the before/after still updates when the preset or tier changes. A queued job
produces the same output as the equivalent CLI command.

**Acceptance Scenarios**:

1. **Given** a selected video, **When** the user opens the upscale view, **Then** a before/after frame
   comparison at the target size is shown within 5 seconds for a typical 1080p source.
2. **Given** a landscape video with smart reframe, **When** the analysis completes, **Then** the crop
   rectangle is overlaid on playback and the fallback ranges are marked on the timeline.
3. **Given** a configured upscale job, **When** it is exported as a manifest and run from the CLI,
   **Then** the outputs are identical to the desktop run.

---

### User Story 6 - AI upscaling when my GPU can handle it (Priority: P6)

A user with a capable graphics card (from any major vendor, on Linux or Windows) gets an AI
upscaling tier that recovers more detail from very low-quality sources than classical scaling. The
app enables it only after a short self-test shows the GPU is fast enough. It warns about expected
duration before long jobs.

**Why this priority**: This tier gives the best quality, but it costs the most to build and to
run. The owner chose to deliver it last, after everything above works without it.

**Independent Test**: On a machine with a capable GPU, the AI tier becomes available after the
self-test. A degraded 360p test clip upscaled with it scores higher than the GPU-classical tier on
the objective quality check. On a machine without one, the tier is listed as unavailable with the
reason.

**Acceptance Scenarios**:

1. **Given** a GPU that passes the self-test, **When** the user selects automatic tier for a source
   below 720p, **Then** the AI tier is offered and, if chosen, used.
2. **Given** a GPU that fails the self-test, **When** the user looks at tiers, **Then** AI is shown as
   unavailable with the measured reason (for example "too slow: estimated 45 min for this clip").
3. **Given** an AI job, **When** it runs, **Then** progress and remaining time are reported like any
   other job, and cancel stops it within 2 seconds.
4. **Given** AI-upscaled video, **When** it is watched, **Then** there is no visible frame-to-frame
   shimmer on static areas beyond what the classical tiers show on the same source.

---

### Edge Cases

- **Mismatched sources:**
  - Vertical source taller than the preset (for example 9:21): crop or blur-pad top and bottom per
    the reframe mode.
  - Square source: the reframe mode applies.
  - Rotated source (rotation metadata): orientation is honoured before any decision.
  - Non-square pixels or anamorphic sources: the displayed aspect is used.
- **Very small or very large sources:**
  - Tiny sources (≤ 240p): processed, with a warning that detail cannot be invented. The AI tier is
    suggested if available.
  - 4K or larger sources: downscale only, with a note that no upscaling was needed.
- **Interlaced or awkward-timing sources:** interlaced sources are deinterlaced first. Variable or
  odd frame rates are preserved, capped at 60 fps for phone presets.
- **HDR sources:** the user chooses between keeping HDR (only presets and formats that support it)
  and converting to standard range. SDR sources are never converted to HDR.
- **Audio, subtitles and metadata:** audio is copied when compatible, otherwise re-encoded. Text
  subtitles are carried where the output format allows, otherwise dropped with a warning. Chapters
  and metadata are kept.
- **Smart reframe edge cases:**
  - Rapid subject motion: the crop speed limit means the subject may briefly touch the edge; it
    must never leave the frame for more than half a second.
  - Many faces: the largest or most central group is followed; if the group cannot fit, blur-pad.
  - Very short shots (< 1 s): handled like any shot, no smoothing across the cut.
- **GPU changes:** if a GPU disappears between capability check and run (laptop dock, driver update),
  the job detects it and falls back (automatic) or fails clearly (forced).
- **Output safety and very long sources:**
  - Output name collisions and existing files follow 001's `--on-exists` rules.
  - The output can never overwrite the source.
  - Very long sources: progress and ETA remain meaningful. Disk space is checked before the run
    against an estimate of output size.

## Requirements *(mandatory)*

### Functional Requirements

**Presets & output**
- **FR-001**: The system MUST provide device presets as versioned data, not code. Users and
  maintainers can add a preset without changing the program. Shipped presets:
  - "phone fill" 1080×2340 (default);
  - "phone fill HD" 1440×3120;
  - "Pixel tall" 1080×2400;
  - "Reddit 9:16" 1080×1920;
  - "Reddit feed 4:5" 1080×1350;
  - "Fold inner";
  - "iPhone Duo".
  Sizes for the last two are fixed in planning from `research/06`; each preset targets an aspect
  class (`research/09` §6).
- **FR-002**: Each preset MUST define the output size, pixel aspect, maximum frame rate, video format
  and tagging, audio format, target quality, and optional platform limits (maximum file size and
  duration). The plan MUST warn when an output is predicted to exceed a limit. Users MUST be able to see which device classes
  each preset is meant for.
- **FR-003**: Outputs MUST play natively, without third-party apps, on iPhone 15–18 including Air and
  Duo, Pixel 8–11 and Galaxy S22–S26 / Z Fold / Z Flip. Default-preset outputs MUST carry the format
  tagging iOS requires for high-efficiency video. The Reddit preset MUST use the most widely
  accepted upload format.
- **FR-004**: Standard-range sources MUST stay standard-range. Colour information MUST be written
  explicitly and correctly, so playback is not washed out or over-saturated on iOS and Android.
- **FR-005**: The system MUST never enlarge and then shrink the video. Sources at or above the
  target size are only reframed or scaled down.

**Quality tiers**
- **FR-006**: The system MUST provide a classical CPU tier. It reduces compression artefacts and
  noise, scales with a high-quality filter, then applies restrained sharpening. It MUST work on
  every supported machine with no GPU.
- **FR-007**: The system MUST provide a GPU tier with higher-quality scaling. It is used
  automatically when a usable hardware GPU is detected, and software-emulated GPUs MUST NOT count.
- **FR-008**: Tier choice MUST be automatic by default, reported per job, and overridable per run,
  per video and per clip. An automatic choice that fails MUST fall back once to the CPU tier with a
  warning. A forced tier MUST NOT fall back.
- **FR-009**: The machine capability check (GPU usable, speed) MUST be cached and refreshed when the
  hardware, driver or application version changes. Users MUST be able to run it again on demand.
- **FR-010** (US6): The system MUST provide an AI tier through a bundled component that works on GPUs
  from all major vendors on Linux and Windows. It is enabled only when a self-test shows the GPU can
  process the job within a reasonable estimate. Only model weights whose licence permits
  redistribution may be bundled, and their licences MUST be recorded in the repository.
- **FR-011** (US6): The AI tier MUST include measures that limit frame-to-frame flicker. When AI is
  available, it MUST be offered automatically only for sources below 720p.

**Reframing**
- **FR-012**: When the source's displayed aspect differs from the preset's, the system MUST apply the
  selected reframe mode:
  - **smart** (default): follow the subject;
  - **center crop**;
  - **fit with blurred background**: the whole frame is visible over a blurred, darkened copy that
    fills the screen.
- **FR-013**: Smart reframe MUST:
  - detect scene cuts and treat each shot independently;
  - locate faces (people as a later addition if faces prove insufficient);
  - compute a crop path that keeps the subject in frame, limits pan speed and acceleration, ignores small
    jitters, and changes abruptly only at cuts.
- **FR-014**: Smart reframe MUST fall back to fit-with-blurred-background for time ranges with no
  subject, or with subjects that cannot fit together. These ranges MUST be listed in the job
  result.
- **FR-015**: Smart reframe analysis MUST run locally on the CPU with bundled, permissively
  licensed components. No network access is needed, and copyleft-licensed detectors MUST NOT be
  used.
- **FR-016**: Users MUST be able to see the planned crop path before rendering: as data in the
  CLI dry run, and drawn over playback in the desktop app.

**Inputs, runs & surfaces** (reuse 001 conventions)
- **FR-017**: Upscaling MUST accept the inputs 001 accepts: files, folders (with discovery),
  `--inputs-from` lists and JSON/CSV manifests. Manifests gain upscale options at
  defaults / video / clip level, with 001's layering order.
- **FR-018**: Clip requests MUST be able to enable upscaling, so each clip is cut and upscaled in one
  pass without an intermediate file left behind.
- **FR-019**: Dry run, JSON output, progress with ETA, cancellation (≤ 2 s, no partial files, no
  orphan processes), crash cleanup, output naming, collision policy and exit codes MUST behave as
  in 001.
- **FR-020**: Audio, subtitles, chapters and metadata MUST be handled as in the edge cases above,
  with a warning for anything dropped.
- **FR-021**: Before a run, the system MUST estimate output size and processing time per job, and
  warn when free disk space is insufficient.
- **FR-022**: The desktop app MUST offer an upscale view with:
  - preset, reframe mode and tier choice;
  - a before/after frame comparison;
  - a crop-path overlay on playback with fallback ranges marked on the timeline;
  - adding jobs to the shared queue.
  Exported manifests MUST reproduce desktop results exactly from the CLI.
- **FR-023**: Diagnostics (`doctor` / Settings) MUST report the available tiers, the detected GPU and
  the reason any tier is unavailable.

### Key Entities

- **Device Preset**: a named, versioned output target. It holds:
  - size, aspect, maximum frame rate;
  - video and audio format with tagging, target quality;
  - the device classes it serves.
  Presets are shipped as data and can be extended by the user.
- **Upscale Options**: preset, reframe mode, tier (auto or forced), HDR handling, and quality
  overrides. They layer like 001 options.
- **Quality Tier**: CPU-classical, GPU-classical or AI. It has an availability status and a
  reason, and the per-machine capability record says which tiers are usable.
- **Capability Record**: the detected GPU(s), whether each is usable, the self-test speed, and the
  hardware/driver/app fingerprint that invalidates the record.
- **Reframe Analysis**: per-shot subject detections, the crop path (position over time) and the
  fallback ranges. It is produced before rendering and can be shown in previews and dry runs.
- **Upscale Job**: a 001 job extended with upscale options. It can be standalone or chained after a
  clip, and its outcome reports the tier used, any fallbacks and the reframe fallback ranges.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Default-preset outputs from a 480p source play edge-to-edge (no bars) in the stock
  gallery app on the reference devices: iPhone 15/16/17/18 Pro class, Pixel 10 Pro XL and Galaxy
  S2x. 100% of outputs pass an automated check of size, aspect, tagging and colour information.
- **SC-002**: Measured against the clean original, outputs from degraded 360p–480p test sources
  score higher on an objective perceptual-quality metric than a plain resize to the same size.
  The CPU tier scores ≥ 1.5 points higher on a 0–100 scale on the synthetic test sources
  (≥ 3 points is checked on real footage by the owner), and the AI tier scores ≥ 3 points higher
  than the GPU tier. (Owner decision 2026-10-14: restrained sharpening measured +2.8 on the
  synthetic sources; passing +3 there needed sharpening that visibly rings.)
- **SC-003**: The CPU tier processes a 60 s, 480p, 30 fps source to the default preset in ≤ 2× the
  clip's duration on the reference machine (12-thread laptop CPU). The GPU tier is no slower than
  the CPU tier on the same machine.
- **SC-004**: With smart reframe, a moving subject in synthetic test material stays fully inside
  the output frame in ≥ 95% of frames and never leaves it for more than 0.5 s. Crop movement
  between consecutive frames never exceeds the configured speed limit except at detected cuts.
- **SC-005**: Smart reframe analysis adds ≤ 50% to total processing time for a 60 s 1080p source on
  the reference machine.
- **SC-006**: Cancellation stops any tier within 2 s and leaves no partial files or stray processes,
  as in 001.
- **SC-007**: A user with no video knowledge can produce a phone-ready file from the desktop app in
  ≤ 3 interactions after opening a video (select, choose upscale, run) using defaults.
- **SC-008**: The automatic tier choice never selects a tier that then fails on the same machine
  more than once. After a fallback, the capability record is updated so later runs choose the
  working tier first.
- **SC-009** (US6): On a mid-range discrete GPU, the AI tier processes a 60 s 480p source in
  ≤ 10 minutes. The self-test result predicts actual duration within ±30%.

## Assumptions

- **Reference devices and players:**
  - The reference device list and sizes come from `research/06-device-targets-verified.md`.
  - Unverified rows (the base iPhone 18, Fold8/Flip8 pixel counts) are fixed in planning.
  - Exact gallery-app fit/fill behaviour is confirmed by a manual device test matrix.
- **Reddit:**
  - Reddit re-encodes uploads and caps playback at 1080p, so the Reddit presets target 1080-wide
    output in the most compatible format. Wider presets give no benefit there.
  - Limits of 1 GB and 15 min are carried as preset data. Reddit's official help page could not be
    read, so the figures come from consistent secondary sources (`research/09` §5).
  - The owner confirms feed and full-screen presentation with one test upload.
- **Output format:**
  - Phone presets default to the high-efficiency video format with iOS-compatible tagging. 60 fps is
    the cap: some base models have 60 Hz screens, and the size and battery cost of higher rates
    isn't worth it.
  - The output container is mp4 for all presets.
- **Engine and tiers:**
  - The bundled engine (FFmpeg 9) already provides the CPU and GPU scaling filters needed.
    `research/02` "Measured" shows both work on the development machine.
  - The AI tier needs a separately bundled worker. ncnn with Vulkan and the Real-ESRGAN model family
    were chosen; both are permissively licensed (`research/09` §1–3).
  - Measured on the reference iGPU, the AI tier runs at about 0.7 fps, so in practice it is a
    discrete-GPU feature, and the self-test (FR-010) rejects integrated GPUs for normal clip lengths.
- **Smart reframe in v1:** v1 follows faces horizontally, using a small permissively licensed
  detector. It measured 68 ms per analysis frame on the reference CPU (`research/09` §4). Person
  detection is added only if needed. Saliency models, vertical
  movement, zoom and split-screen layouts are out of scope (deferred per `research/08`).
- **Out of scope:** GIF export (feature 003), frame-rate interpolation, colour grading, stabilization
  and audio enhancement.
- **Reference machine:** the development laptop (Intel Raptor Lake, 12 threads, Intel iGPU). AI-tier
  targets are verified on a discrete GPU via CI or the owner's hardware.
