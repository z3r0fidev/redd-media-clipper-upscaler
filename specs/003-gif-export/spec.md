# Feature Specification: GIF Export for Reddit

**Feature Branch**: `003-gif-export`

**Created**: 2026-10-22

**Status**: Draft

**Input**: User description: "GIF export for Reddit (feature 003) for rmedia (CLI + desktop, Linux +
Windows, shared core). Users turn a video, a clip made by 001, or an upscaled/reframed output of 002
into an animated GIF that looks clean and fits a file-size budget for Reddit posts and comments."
Direction from `research/05-gif-export.md`: the bundled engine's two-pass palette pipeline; a
bounded size-budget search that lowers the least visible settings first and trims duration only
with consent; presets as data (Reddit post, Reddit comment, max quality, flat/text/UI); a "Reddit
MP4 (silent loop)" output next to GIF; portrait via 002's reframe, never upscaling for GIF; the
001/002 conventions (sources untouched, dry run, progress, cancel, JSON, manifests, combination with
clipping); a desktop GIF view with an estimated size and a preview. Optional external optimisers
(gifsicle GPL-2.0, gifski AGPL-3.0) are a licensing decision. Owner decision (2026-10-22): include
both gifsicle and gifski as optional engines.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Make a GIF that fits a Reddit post (Priority: P1)

A user has a short video or a clip made with rmedia. They want an animated GIF they can upload to
a Reddit post. It should look clean (no blotchy colours, no flicker, crisp edges) and come in under
the size Reddit and most subreddits accept, without them having to know what palettes, dithering
or frame rates are.

**Why this priority**: This is the feature's core promise. On its own it delivers value: one
command (or one button) turns a video into a postable GIF.

**Independent Test**: Convert a 6 s, 1280x720 test video with the default "Reddit post" target.
The result is a looping GIF of at most 8 MB, at most 720 px wide, 15 frames per second, and every
frame delay is a whole number of hundredths of a second, at least 2.

**Acceptance Scenarios**:

1. **Given** a 6 s landscape video, **When** the user exports a GIF with default settings,
   **Then** a looping GIF is written next to the other outputs, at most 720 px wide, within the
   post budget, and the run reports its size, width, frame rate and duration.
2. **Given** a source that is already smaller than the target width, **When** the user exports,
   **Then** the GIF keeps the source width (no upscaling) and a note says so.
3. **Given** the user asks for a frame rate GIF cannot represent exactly (e.g. 30 fps), **When**
   they export, **Then** the run uses the nearest supported rate or the approximation, says which,
   and never writes a frame delay under 2 hundredths of a second.
4. **Given** a start and end time, **When** the user exports, **Then** only that range is in the
   GIF, within one frame.
5. **Given** a dry run, **When** the user asks for one, **Then** the planned output, settings and
   estimated size are shown and nothing is written.
6. **Given** the user cancels during encoding, **When** cancellation completes, **Then** within 2 s
   no partial output, scratch file or engine process is left.

---

### User Story 2 - Hit a size budget automatically (Priority: P2)

The user's GIF comes out too big for the budget (a long or busy clip, or a portrait clip). They
want the tool to find the best-looking settings that fit, rather than guess by hand, and to tell
them what it gave up.

**Why this priority**: Without it, many real clips miss the budget and the user is back to trial
and error. It builds directly on US1.

**Independent Test**: Export a busy 10 s, 1280x720 test video to a 3 MB budget. The result is at
most 3 MB, the report lists each setting that was lowered from the preset's default, and no more
than 10 full or sample encodes were run.

**Acceptance Scenarios**:

1. **Given** a clip whose default-settings GIF is over the budget, **When** the user exports with
   a budget, **Then** the result fits the budget, and the settings were lowered in this order of
   least visible damage: lossy compression strength (when the optimiser is available), dithering
   style, number of colours, frame rate, width (as FR-009).
2. **Given** a clip that cannot fit the budget even at the lowest allowed width and frame rate,
   **When** the user exports, **Then** nothing over the hard limit is written; the run explains
   that only a shorter duration would fit, says how long, and trims only if the user allowed it
   (a flag in the CLI, a confirmation in the desktop app).
3. **Given** a soft and a hard budget, **When** the result fits the hard but not the soft budget,
   **Then** the GIF is kept and a warning says it is over the recommended size.
4. **Given** the same input and options twice, **When** both runs finish, **Then** the outputs are
   byte-identical and the search took the same steps.
5. **Given** a budget search is running, **When** the user cancels, **Then** it stops as in US1.

---

### User Story 3 - A smaller, sharper alternative: Reddit MP4 (Priority: P3)

For a Reddit post, a silent looping MP4 is usually much smaller and sharper than a GIF, because
Reddit turns uploaded GIFs into video anyway. The user wants that as a one-step choice, with the
same trimming, framing and budget handling as GIF.

**Why this priority**: It gives Reddit post users a better result for the same effort, and it is
cheap once US1 exists. GIF remains necessary for comments and GIF-only communities.

**Independent Test**: Export the US1 test video as "Reddit MP4". The result has no audio track,
plays in a standard player, starts playing before the file has fully downloaded, and is smaller
than the GIF of the same clip.

**Acceptance Scenarios**:

1. **Given** a video, **When** the user chooses the Reddit MP4 output, **Then** a silent,
   web-streamable MP4 is written with the same range and framing a GIF would have had.
2. **Given** the "Reddit post" target, **When** the user exports a GIF, **Then** the report
   mentions that the MP4 output would be smaller (with its estimated size), once per run.
3. **Given** a budget, **When** the user exports an MP4, **Then** the MP4 fits it, lowering
   quality before resolution.

---

### User Story 4 - Portrait and phone-friendly GIFs via smart reframe (Priority: P4)

The user's source is landscape but they want a vertical or square GIF that keeps the subject in
view, the same way 002 reframes videos for phones.

**Why this priority**: Portrait and 4:5 shapes take more room in the mobile Reddit feed. It reuses
002's reframe, so the cost is mostly wiring and tests.

**Independent Test**: Export a landscape test video with a moving face as a 4:5 GIF using smart
reframe. The face stays inside the frame in at least 95% of frames, as in 002's SC-004.

**Acceptance Scenarios**:

1. **Given** a landscape source, **When** the user picks a portrait (9:16), feed (4:5) or square
   (1:1) shape with smart, centre or blur framing, **Then** the GIF has that shape and the framing
   behaves as in 002.
2. **Given** an output already upscaled by 002, **When** the user exports it as a GIF, **Then** it
   is downscaled to the GIF width, never upscaled further.
3. **Given** smart framing cannot find a subject, **When** the user exports, **Then** the
   fallback from 002 applies and is reported.

---

### User Story 5 - Many GIFs at once and straight from a clip (Priority: P5)

A user making several GIFs from one or more videos wants to describe them once (in a clip list or
manifest, or with the clip command) and get all GIFs in one run.

**Why this priority**: This is where volume work happens, and it reuses 001/002 inputs.

**Independent Test**: A manifest with two videos and three clips, one clip overriding the target
to "Reddit comment", produces three GIFs with the right budgets and no intermediate files left.

**Acceptance Scenarios**:

1. **Given** a manifest with GIF options at defaults, video and clip level, **When** the user runs
   it, **Then** each clip becomes a GIF with the layered options, and one failure does not stop
   the others.
2. **Given** a clip request with GIF output enabled, **When** it runs, **Then** the clip is cut and
   converted in one job with no intermediate video left behind.
3. **Given** a clip that is also upscaled/reframed by 002, **When** it runs, **Then** the GIF is
   made from the reframed result in the same job.

---

### User Story 6 - Preview and tune in the desktop app (Priority: P6)

The user wants to pick a range, a target and a shape in the desktop app, see an estimated size
before encoding, and look at the finished GIF before posting it.

**Why this priority**: Visual confidence and fewer retries. It depends on US1–US4.

**Independent Test**: Open a video in the GIF view, choose a range and "Reddit comment". An
estimated size appears before encoding; after encoding, the GIF plays in the app and its real size
is within the stated margin of the estimate.

**Acceptance Scenarios**:

1. **Given** a video in the GIF view, **When** the user picks a range, target, shape and output
   (GIF or MP4), **Then** an estimated size appears within 5 s and updates when a setting changes.
2. **Given** a finished export, **When** the user opens it, **Then** it plays looping in the app
   with its size, width, frame rate and any settings the budget search lowered.
3. **Given** a GIF job, **When** the user adds it to the queue, **Then** it runs with other jobs and
   an exported manifest reproduces the same output from the CLI.

---

### Edge Cases

- **Very long range:** above the target's recommended duration (post 15 s, comment 6 s), the run
  warns before encoding; the budget search still applies.
- **Very short range:** a range shorter than 2 frames at the chosen rate is rejected with a clear
  message.
- **Static or near-static video:** the GIF is valid and small; no frame is dropped in a way that
  changes timing.
- **Transparency:** the source's alpha channel (if any) is not preserved; GIF output is opaque.
- **HDR or wide-gamut source:** tone-mapped to standard range before colour reduction, as in 002.
- **Variable frame rate source:** output timing follows the source clock; total duration stays
  within one frame.
- **Rotated phone video:** the rotation is applied before scaling, as in 001/002.
- **Audio, subtitles, chapters, metadata:** never in a GIF; dropped from Reddit MP4 too, with no
  warning (the output is silent by definition).
- **Output already exists:** the 001 collision policy (skip by default, rename, overwrite).
- **Unwritable output folder, full disk:** the 001 failure classes and messages apply; nothing
  partial is left.
- **Engine lacks a needed filter or encoder:** a clear message names it; GIF stays available if
  only the MP4 encoder is missing, and vice versa.
- **Budget smaller than any possible output** (e.g. 10 KB): rejected before encoding with the
  smallest achievable size.

## Requirements *(mandatory)*

### Functional Requirements

**Output and quality**

- **FR-001**: Users MUST be able to export a video, or a time range of it, as a looping animated
  GIF. Loop count MUST be configurable (forever by default, or play N times).
- **FR-002**: Colour reduction MUST be computed from the final-size frames, so the palette fits
  what is shown.
- **FR-003**: Frame rates MUST be limited to what GIF timing can express; no frame delay below
  2 hundredths of a second may be written. Rates that GIF can show exactly (10, 20, 25, 50) MUST
  be marked as such; others MUST be reported as approximations.
- **FR-004**: The GIF width MUST NOT exceed the source's (or the reframed result's) width.
- **FR-005**: Users MUST be able to export a silent, web-streamable "Reddit MP4" with the same
  range, framing and budget options as GIF.

**Targets and presets**

- **FR-006**: Targets MUST be versioned data, not code branches, and users MUST be able to add
  their own. Shipped targets:
  - Reddit post: 8 MB recommended, 20 MB hard limit, 720 px wide (480 px for portrait), 15 fps.
  - Reddit comment: 3 MB recommended, 8 MB hard limit, 480 px wide (360 px for portrait), 12 fps.
  - Max quality: no budget, 960 px wide, 20 fps, best-looking dithering; uses the best-quality
    engine when it is available.
  - Flat / text / UI: crisp edges, few colours, no dithering, 15 fps.
- **FR-007**: The Reddit limits in the shipped targets MUST be labelled as unverified in the
  documentation and the target list until the owner confirms them.
- **FR-008**: Users MUST be able to override width, frame rate, colours, dithering style, loop
  count and budget per run.

**Budget search**

- **FR-009**: With a budget, the system MUST search for the best settings that fit, lowering them
  in this order: lossy compression strength (only when the optimiser is available), dithering
  style, number of colours, frame rate, width, with floors of 320 px (post) and 240 px (comment)
  width and 10 fps.
- **FR-010**: The search MUST be bounded (at most 10 encodes, counting samples) and MUST estimate
  from short samples of the clip before encoding the whole of it.
- **FR-011**: If no allowed setting fits, the system MUST NOT write an output over the hard limit;
  it MUST report the longest duration that would fit and trim to it only when the user allowed
  trimming.
- **FR-012**: The result MUST report the final size, width, frame rate, colours, dithering style
  and every setting lowered from the target's default.

**Framing**

- **FR-013**: Users MUST be able to choose an output shape (source, 9:16, 4:5, 1:1) and a framing
  (smart, centre, blur) that behave as in 002.
- **FR-014**: When the input or the chained step is an upscaled result, the GIF MUST be made by
  downscaling it.

**Inputs, runs and surfaces** (reuse 001/002 conventions)

- **FR-015**: GIF export MUST accept the inputs 001 accepts (files, folders, input lists,
  JSON/CSV manifests). Manifests gain GIF options at defaults, video and clip level with 001's
  layering order.
- **FR-016**: Clip requests MUST be able to produce a GIF or Reddit MP4 directly, optionally after
  002's upscale/reframe, in one job without leaving intermediate files.
- **FR-017**: Dry run, JSON output, progress with ETA, cancellation (within 2 s, no partial files,
  no stray processes), crash cleanup, output naming, collision policy and exit codes MUST behave
  as in 001.
- **FR-018**: Sources MUST never be modified; outputs MUST be written to a temporary name and
  committed only after verification (valid file, expected size, frame count within one frame).
- **FR-019**: Before a run, the system MUST show an estimated output size per job and warn when
  free disk space is insufficient.
- **FR-020**: The desktop app MUST offer a GIF view with range selection, target, shape, framing
  and output type, an estimated size before encoding, playback of the finished output, and adding
  jobs to the shared queue. Exported manifests MUST reproduce desktop results from the CLI.
- **FR-021**: Diagnostics MUST report whether GIF and Reddit MP4 export are available and, if not,
  what is missing.
**Optional engines** (owner decision 2026-10-22: both in scope)

- **FR-022**: The system MUST support an optional GIF optimiser (gifsicle) as a post-pass that
  shrinks a finished GIF losslessly and, at a chosen strength, lossily. It MUST be used
  automatically when available and the budget search needs it, and users MUST be able to turn it
  off.
- **FR-023**: The system MUST support an optional best-quality GIF engine (gifski) that users can
  choose per run or per target. When it is chosen but unavailable, the run MUST fall back to the
  built-in engine with a warning (never fail), unless the user asked for it strictly.
- **FR-024**: Both optional engines MUST run as separate programs, located like the bundled
  engine (explicit setting, bundled copy, then the system path), with their version reported in
  diagnostics. Without them every other requirement MUST still be met.
- **FR-025**: Their licences MUST be recorded in the third-party notices with a source offer, and
  any bundled copy MUST be pinned and verified like the other bundled downloads. The best-quality
  engine's licence (AGPL-3.0) MUST be resolved against the application's licence before a release
  bundles it.

### Key Entities

- **GIF Target**: a named, versioned preset: recommended and hard size limits, width for landscape
  and portrait, frame rate, colours, dithering style, recommended maximum duration, and whether
  its limits are verified. Shipped as data; users can add their own.
- **GIF Options**: target, output type (GIF or Reddit MP4), engine (built-in or best quality),
  optimiser on/off, range, shape, framing, loop count,
  overrides and whether trimming is allowed. They layer like 001 options.
- **GIF Engine**: built-in, optimiser post-pass or best-quality engine, each with an
  availability status, version and the reason it is unavailable.
- **Budget Search Record**: the steps tried (settings and estimated or real size), the chosen
  settings, and the lowered settings. Shown in reports, JSON and dry runs.
- **GIF Job**: a 001 job producing a GIF or Reddit MP4, standalone or chained after a clip and an
  optional 002 upscale/reframe. Its outcome reports size, settings and any trimming.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of outputs pass an automated check: valid looping GIF (or silent streamable
  MP4), expected width and shape, no frame delay under 2 hundredths of a second, duration within
  one frame of the request.
- **SC-002**: With a budget, 100% of outputs on the test set are at or under the hard limit, and at
  least 90% are at or under the recommended size when that is achievable at the floors.
- **SC-003**: Budget search uses at most 10 encodes, and a 10 s 720p clip to the post budget
  finishes in at most 3x the clip's duration on the reference machine (12-thread laptop CPU).
- **SC-004**: The estimate shown before encoding is within 25% of the final size for 90% of the
  test clips.
- **SC-005**: Measured against the source frames, the default "Reddit post" GIF scores at least as
  well on an objective perceptual-quality metric as a plain one-pass conversion of the same size,
  frame rate and width.
- **SC-006**: The Reddit MP4 of a clip is at least 5x smaller than its GIF at the same width and
  frame rate on the test set.
- **SC-007**: In the desktop app, a user exports a GIF of a chosen range in 3 interactions or
  fewer after opening a video, and the output is byte-identical to the CLI's.
- **SC-008**: Cancellation stops any GIF or MP4 job within 2 s with no partial files or stray
  processes, on Linux and Windows.
- **SC-009**: With the optimiser available, the default "Reddit post" GIF is at least 15% smaller
  than without it on the test set, with no drop on the perceptual-quality metric larger than
  1 point.
- **SC-010**: With the best-quality engine, "Max quality" scores higher on the perceptual-quality
  metric than the built-in engine at the same width and frame rate on the test set.

## Assumptions

- **Reddit limits are unverified:** Reddit's Help Center could not be fetched during research
  (`research/05` §0). The shipped budgets come from third-party guides; the owner confirms them
  in a browser before release, as for 002's owner checks.
- **Engine:** the bundled FFmpeg already includes everything needed for GIF and H.264 MP4. The
  optional engines (gifsicle GPL-2.0, gifski AGPL-3.0, `research/05` §3) are new third-party
  components. Owner decision (2026-10-23): installers and CLI archives bundle both, pinned and
  built from source; a release that bundles gifski is gated on the application licence (still a
  placeholder, see the roadmap and `plan.md`).
- **Animated WebP and AVIF are out of scope:** Reddit's acceptance of animated WebP is unconfirmed
  and AVIF is not accepted (`research/05` §5).
- **Out of scope:** uploading to Reddit, text/caption overlays, stickers, crossfade "perfect
  loops", reverse/boomerang, and per-frame editing.
- **Reuse:** the 001 job model and the 002 multi-stage jobs, weighted progress, scratch folders,
  reframe analysis and colour handling are reused, not rebuilt.
- **Reference machine:** the development laptop (Intel Raptor Lake, 12 threads), as in 002.
