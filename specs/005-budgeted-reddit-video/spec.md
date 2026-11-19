# Feature Specification: Budgeted Reddit Video

**Feature Branch**: `005-budgeted-reddit-video` (work happens on `main`; no feature branch)

**Created**: 2026-11-19

**Status**: Draft

**Input**: User description: "005 Budgeted Reddit video: export a clip (or a whole video) as a
Reddit-ready MP4 *with audio* that is guaranteed to fit a size budget and a length limit. The
budgets are data targets, like 003's GIF targets: a built-in `reddit-video` target with Reddit's
limits as of 2026-11-13 (`research/10-reddit-limits-2026-11-13.md`: 1 GB, 15 min, MP4, H.264, audio
optional, 1920×1080 recommended, any aspect ratio), plus user target files for stricter caps (for
example a subreddit's own size or length rule). The output must never exceed the hard size cap:
the encoder's bitrate is chosen from the length and the budget (video and audio shares), the
result is measured, and an over-budget result is re-encoded lower before anything is committed. A
range longer than the target's length limit is an error unless trimming is allowed. Resolution is
never enlarged and is lowered only when the budget needs it, down to a floor the target sets. The
dry run shows the predicted size, bitrate and settings. Works from the CLI, in JSON and CSV
manifests and in the desktop app, like the other features; reuses 002's device presets and
reframing (9:16 and 4:5 shapes) and 001's frame-accurate cut. Out of scope: Reddit Premium's 30 min
as a default (a user target can set it), uploading, the silent "Reddit MP4" of 003 (unchanged),
HEVC/AV1 output, and changes to GIF export. Suggested success criteria: 100% of outputs at or under
the hard cap; outputs use at least 90% of the budget when the budget is the limiting factor (no
needless quality loss); start and end within ±1 frame; audio kept in sync. Research:
`research/10-reddit-limits-2026-11-13.md`, `research/09-open-items-settled.md` §5,
`research/05-gif-export.md` (budget search). Version 0.5.0 when done."

## Background

rmedia already makes Reddit-shaped output, but none of it is a video with sound that is sure to
be accepted:

- **001 clips** keep the source's size, with no limit on the file size.
- **002's Reddit presets** (`reddit-916`, `reddit-feed-45`) carry Reddit's 1 GB / 15 min limits as
  data. The plan warns when an output may pass them, and the bitrate is capped "when possible",
  but nothing measures the result or guarantees it.
- **003's "Reddit MP4"** is a silent loop, made as a GIF alternative.

Reddit's limits as of 2026-11-13 (`research/10`) are 1 GB and 15 minutes for a video post, as MP4
or MOV with H.264, audio optional, and 1920×1080 recommended. Many subreddits set stricter rules of
their own (a size cap in megabytes, or a maximum length), which Reddit doesn't publish.

This feature adds a **budgeted video**: an MP4 with audio, cut frame-exact, that is guaranteed to
be at or under a size cap and a length limit taken from a **video target**. A video target is a
small data file, like 003's GIF targets. rmedia ships one, `reddit-video`, and users add their own.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A clip with sound that Reddit accepts (Priority: P1)

A user wants to post a 3-minute highlight, with its sound, from a long recording. Today they
guess an encoder setting, upload, and sometimes find the file too large. With a budgeted export
they name the range and the `reddit-video` target, and get an MP4 that is never over Reddit's
limit. It keeps as much quality as the limit allows, and its sound stays in sync.

**Why this priority**: This is the feature. A video that may be rejected is the problem
being solved, and every other story builds on this export.

**Independent Test**: Export a range of a test source with the `reddit-video` target, and with a
user target whose cap is small enough to force a low bitrate. In both cases the output is at or
under the cap, and starts and ends on the requested frames. It has audio, and the audio is in sync
with the picture.

**Acceptance Scenarios**:

1. **Given** a 1080p source with audio, **When** the user exports 00:10–03:10 with the
   `reddit-video` target, **Then** the result is an MP4 (H.264 video, AAC audio) of 3 minutes
   (±1 frame), at or under 1 GB, with the audio in sync.
2. **Given** a user target with a 50 MB cap, **When** the user exports a 5-minute range, **Then**
   the result is at or under 50 MB, and uses at least 90% of it.
3. **Given** any target, **When** the first encode comes out over the cap, **Then** rmedia
   re-encodes lower. Nothing over the cap is ever written to the output name.
4. **Given** a source without audio, **When** it is exported, **Then** the output has no audio
   track, and the whole budget goes to the picture.
5. **Given** a range shorter than the cap could ever fill, **When** it is exported, **Then** the
   output keeps the target's normal quality. It isn't inflated to fill the budget.

---

### User Story 2 - Length and resolution limits, with a clear answer when it can't fit (Priority: P2)

A user tries to post a 20-minute range, or to fit 10 minutes of 4K into 25 MB. rmedia tells them
before encoding what will happen. Either the picture is made smaller, down to a floor the target
sets, or the job is refused with a reason: too long for the target, or the budget can't be met
even at the floor. A user who allows trimming gets the longest part that fits instead.

**Why this priority**: Without this, Story 1's guarantee has silent failure modes. Refusing
clearly is part of "guaranteed to fit".

**Independent Test**: Plan ranges longer than a target's length limit, with trimming allowed and
not allowed. Plan budgets that need a smaller picture and budgets that can't be met at the
target's floor. Check each outcome and its code.

**Acceptance Scenarios**:

1. **Given** a target with a 15-minute limit, **When** a 20-minute range is planned, **Then** the
   job is refused with a "too long" reason naming both lengths, and nothing is encoded.
2. **Given** the same range **with trimming allowed**, **When** it runs, **Then** the output is
   the first 15 minutes of the range, and the result says it was trimmed.
3. **Given** a budget that would fall below the target's minimum video bitrate at full size,
   **When** it is planned, **Then** the plan lowers the resolution step by step, never below the
   target's floor, and says so.
4. **Given** a budget that can't be met even at the floor, **When** it is planned, **Then** the
   job is refused as "budget unreachable", with the smallest workable cap for that length, unless
   trimming is allowed.
5. **Given** a source smaller than the target's preferred size, **When** it is exported, **Then**
   it is never enlarged.

---

### User Story 3 - Know the size before running (Priority: P3)

Before a long encode, the user wants to see what they'll get: the target, its caps, the planned
resolution, the video and audio bitrates, and the predicted size against the cap. The CLI shows
this in its dry run, as text and JSON.

**Why this priority**: The dry run is how every rmedia feature is checked before spending time,
and scripts read its JSON. It depends on Stories 1–2's decisions.

**Independent Test**: Dry-run a budgeted export. Check that nothing is written, and that the
report lists the target, its limits, the planned settings and the predicted size.

**Acceptance Scenarios**:

1. **Given** a budgeted export, **When** it is dry-run, **Then** the report shows the target (and
   whether its limits are verified), the cap, the length limit, the resolution, the video and audio
   bitrates, and the predicted size. No file is written.
2. **Given** `--json`, **When** it is dry-run, **Then** the same facts are in a documented,
   versioned JSON block.
3. **Given** a plan that lowers the resolution or refuses the job, **When** it is dry-run, **Then**
   the report gives the reason with its code.

---

### User Story 4 - Device shapes and batches (Priority: P4)

A user exports a set of highlights from a manifest, each as a 9:16 vertical video for Reddit's
full-screen player, or as 4:5 for the feed, each within budget. They use 002's device presets and
reframing for the shape, and the video target for the budget, in JSON or CSV manifests and in
`batch`.

**Why this priority**: Reddit-bound clips are usually made in sets, and often vertical. It reuses
002 and the manifest formats, so it comes after the single export works.

**Independent Test**: Run a JSON and a CSV manifest with several clips. Use the target on some,
combine it with a 9:16 or 4:5 preset on others, and set it at different manifest levels. Check
that every output is within its cap and has the requested shape.

**Acceptance Scenarios**:

1. **Given** a manifest with the target set at the top level and overridden on one clip, **When**
   it runs, **Then** each clip uses its own target.
2. **Given** the `reddit-916` preset with the `reddit-video` target, **When** a clip is exported,
   **Then** the output is 1080×1920, reframed as 002 does it, and within the cap.
3. **Given** a CSV manifest with target columns, **When** it is read, **Then** the result matches
   the equivalent JSON manifest.
4. **Given** a budgeted clip in smart cut mode, **When** it is planned, **Then** it is cut in
   accurate mode (a budgeted export always re-encodes), with the "smart not applied" notice.

---

### User Story 5 - Budgeted video in the desktop app (Priority: P5)

A desktop user picks a range on the timeline, chooses "Reddit video" (or one of their own
targets), sees the predicted size against the cap, and adds the clip to the queue. The queue
shows each job's target and cap, and the finished size.

**Why this priority**: The desktop app is a first-class surface, but it sits on the same core
behaviour and comes last, as in 002–004.

**Independent Test**: In the desktop app, choose a video target for a range. Check the shown
prediction and the queue entry, run it, and check that the output and its reported size match
what the CLI makes for the same request.

**Acceptance Scenarios**:

1. **Given** an opened source, **When** the user picks a range and the "Reddit video" target,
   **Then** the panel shows the cap, the length limit and the predicted size before the clip is
   queued.
2. **Given** a range over the target's length limit, **When** it is chosen, **Then** the panel
   says so, with the trim option, before the clip can be queued.
3. **Given** the same request, **When** it is run from the desktop app and from the CLI, **Then**
   both make the same output settings.

---

### Edge Cases

- **Very short ranges** (under a second): the budget isn't the limit. The target's normal quality
  applies, and the size check still runs.
- **Audio-only or no-video sources**: refused (a video target needs video).
- **Several audio tracks**: the first (default) track is kept, as an MP4 for Reddit has one. The
  others are dropped, and the result says so.
- **Odd sizes and odd source dimensions**: the output always has even width and height (needed by
  4:2:0 H.264).
- **HDR or 10-bit sources**: tone-mapped to SDR 8-bit, as 002 does for H.264 presets, so every
  phone can play the file.
- **Variable frame rate and high frame rates**: the frame rate is kept up to the target's maximum
  (60 fps for `reddit-video`), and lowered to it when higher.
- **Very large caps** (1 GB on a 10-second clip): quality, not size, decides. The bitrate never
  goes above the target's maximum.
- **A re-encode that still overshoots**: at most a fixed number of retries, each lower. Then the
  job fails with a clear reason, and nothing is written.
- **Disk space**: the plan's free-space check counts the scratch encode and the output.
- **A user target that's invalid** (cap of 0, floor larger than the preferred size, unknown
  fields): reported in `doctor` and refused when used, like GIF targets.
- **Cancellation** during an encode or a retry: the partial files are removed, as for every job.

## Requirements *(mandatory)*

### Functional Requirements

**Targets**

- **FR-001**: rmedia MUST ship a built-in video target `reddit-video` with these defaults:
  - hard size cap 1 GB (1,073,741,824 bytes) and length limit 15 minutes;
  - MP4 container, H.264 video, AAC audio;
  - largest output 1920 pixels on the long side (never enlarged);
  - maximum 60 fps;
  - a resolution floor and a minimum video bitrate;
  - an audio bitrate with a lower floor.
- **FR-002**: Users MUST be able to add video targets as data files in the settings folder,
  overriding a built-in one by id or adding new ones. The targets use the same file layout and
  validation rules as 003's GIF targets.
- **FR-003**: A target MUST state whether its limits are verified. `reddit-video` ships as **not
  verified** until the owner's upload check passes (`research/10`), and the dry run and the
  results show the flag.
- **FR-004**: An invalid target file MUST be reported by `doctor`, and refused when named, with
  the reason; the other targets stay usable.

**Fitting the budget**

- **FR-005**: The output MUST NOT exceed the target's hard cap. Every output is measured before
  it's committed, and an output over the cap is re-encoded lower or the job fails. A file over the
  cap is never written to the output name.
- **FR-006**: The video and audio bitrates MUST be chosen from the range's length and the cap, and
  leave a margin for the container. When the budget, not quality, is the limit, the output MUST
  use at least 90% of the cap.
- **FR-007**: When the budget isn't the limit, the output MUST use the target's normal quality
  setting, capped so it can't pass the budget. It MUST NOT be inflated to fill the cap.
- **FR-008**: When the video bitrate would fall below the target's minimum, the resolution MUST be
  lowered step by step, keeping the aspect ratio, down to the target's floor.
- **FR-009**: When the budget can't be met at the floor, the job MUST be refused as
  `BUDGET_UNREACHABLE`, stating the smallest cap that would work for that length. With trimming
  allowed, rmedia MUST instead make the longest start of the range that fits.
- **FR-010**: Retries MUST be bounded (at most 3 re-encodes). After the last one, the job fails
  with the measured size and the cap.

**Length**

- **FR-011**: A range longer than the target's length limit MUST be refused, naming both lengths,
  unless trimming is allowed. With trimming allowed, the output is the first part of the range up
  to the limit, and the result says it was trimmed.

**Picture and sound**

- **FR-012**: The cut MUST be frame-accurate (start and end within ±1 frame of the request), using
  001's accurate cut. Smart cut doesn't apply to budgeted exports (`SMART_NOT_APPLIED`).
- **FR-013**: Audio MUST be kept when the source has it: the first audio track, in AAC, in sync
  with the picture (start offset within one audio frame). Sources without audio give an output
  without an audio track.
- **FR-014**: The output MUST NOT be enlarged beyond the source's picture size.
- **FR-015**: When a 002 device preset is given, its size, shape and reframing MUST be used. The
  target's caps still apply, and the preset's own limits are replaced by the target's.
- **FR-016**: The output MUST play on current phones and on the web: 8-bit 4:2:0 H.264, even
  dimensions, the MP4 index at the front of the file (fast start), and HDR tone-mapped to SDR.

**Surfaces**

- **FR-017**: The CLI MUST offer the budgeted export for single clips, clip lists, folders and
  `batch`. It names a target, and optionally overrides the cap and the length limit for one run,
  and allows trimming.
- **FR-018**: JSON and CSV manifests MUST accept the target and its overrides at every level
  (defaults, video, clip), with the same layering as the other options. A manifest written by the
  desktop app MUST read back unchanged.
- **FR-019**: The dry run MUST show:
  - the target and whether it is verified;
  - the cap and the length limit;
  - the planned resolution and frame rate;
  - the video and audio bitrates and the predicted size;
  - every lowered setting and refusal, with its code.

  The JSON form MUST be a documented, versioned block that only adds to the existing output.
- **FR-020**: Results (text and JSON) MUST report the final size against the cap, the bitrates
  used, how many encodes it took, and whether the range was trimmed.
- **FR-021**: The desktop app MUST let the user choose a video target for a clip, show the cap,
  the length limit and the predicted size before queueing, and show the finished size in the
  queue. For the same request it MUST make the same output settings as the CLI.

**Unchanged**

- **FR-022**: GIF export, 003's silent Reddit MP4, upscaling without a target, and the fast,
  accurate and smart clip modes MUST behave exactly as before when no video target is given.

### Key Entities

- **Video target**: a named set of limits and defaults for a destination. Fields:
  - id, name, and whether its limits are verified;
  - hard size cap and length limit;
  - container and codecs;
  - preferred long-side size and resolution floor;
  - maximum frame rate;
  - normal quality setting and minimum video bitrate;
  - audio bitrate and its floor.

  Built-in (`reddit-video`) or user-defined; a user file can override a built-in one.
- **Budget plan**: what the planner decides for one job:
  - the resolution and frame rate;
  - the video and audio bitrates;
  - the predicted size and the margin;
  - whether the size is set by the budget or by quality;
  - any lowered settings, trim or refusal, with its code.
- **Fit result**: what the run measured: the final size against the cap, the encodes it took
  (with each attempt's size), and whether the range was trimmed.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of budgeted outputs are at or under their hard cap, across the test matrix:
  - short, medium and 15-minute ranges;
  - caps from a few MB to 1 GB;
  - sources with and without audio, SDR and HDR, constant and variable frame rate.
- **SC-002**: When the budget is the limit, outputs use at least 90% of the cap. No output comes
  in under 90% of a cap it was constrained by.
- **SC-003**: Start and end are within ±1 frame of the request. Audio and picture stay in sync to
  within one audio frame at the start and the end.
- **SC-004**: At least 95% of budgeted jobs in the test matrix meet the cap on the first encode;
  the rest within the retry limit.
- **SC-005**: Every refused job (too long, budget unreachable, invalid target) is refused before
  any encode starts, and gives a reason code and the numbers involved.
- **SC-006**: The dry run's predicted size is within 15% of the final size for budget-limited
  jobs.
- **SC-007**: For the same request, the CLI and the desktop app make outputs with identical
  settings (resolution, frame rate, bitrates, attempt count).
- **SC-008**: Owner check: one `reddit-video` output with audio near a real cap (and one 9:16
  output) uploads to Reddit and plays with sound in the feed and the full-screen player.

## Assumptions

- **Defaults (adjustable as data):**
  - Reddit's limits as of 2026-11-13 from `research/10` are the built-in defaults. They come from
    a third-party summary that cites Reddit Help, so `reddit-video` ships unverified (FR-003) until
    the owner's upload (SC-008, roadmap item 3).
  - Audio defaults to AAC stereo at 128 kbit/s, lowered to no less than 64 kbit/s when the budget
    is tight. The audio's share is taken before the video's.
  - The resolution steps down by about 25% at a time; the target's floor defaults to 480 pixels on
    the short side.
  - The frame-rate cap of 60 fps follows the organic-post limits (none is stated); the 30 fps cap
    in Reddit's ad specs doesn't apply to organic posts.
  - A 2% container margin, and at most 3 re-encodes, are enough for H.264 in MP4 (to confirm in
    research).
- **Scope:**
  - Two-pass or capped single-pass encoding is a plan-time decision (research), not a user
    choice.
  - The budgeted export is an option on the existing clip workflow (and on upscaling with a
    device preset), not a separate program.
  - It reuses the existing scratch folder, verify-then-commit, cancellation and event model of
    001–004.
  - Uploading to Reddit, account handling and subreddit rule lookup are out of scope; a
    subreddit's rule is entered as a user target.
  - HEVC, AV1, MOV output, and Reddit Premium's 30 minutes as a default are out of scope; a user
    target can raise the length limit.
- **Depends on:**
  - 001 (accurate cut, manifests);
  - 002 (presets, reframing, tone-mapping);
  - 003 (the target data layout and validation, and the size-check pattern).
- **Version:** 0.5.0 when done.
