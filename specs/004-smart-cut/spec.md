# Feature Specification: Smart Cut

**Feature Branch**: `004-smart-cut` (work happens on `main`; no feature branch)

**Created**: 2026-11-14

**Status**: Draft

**Input**: User description: "004 Smart cut: a hybrid clip mode, `--mode smart`. Frame-exact start
and end; the interior GOPs are stream-copied; the partial head and tail GOPs are re-encoded to
match the source's parameters, then the three parts are joined. First scope: H.264 in mp4, mov,
mkv and ts. Automatic fallback to accurate mode, with a typed reason code, for HEVC, interlaced,
VFR, open-GOP, mismatched parameters, 10-bit, or no full GOP inside the range. Seams are verified
after the join; the dry run reports how much is copied against re-encoded; the manifest (JSON and
CSV) gets a `mode` value of smart; the desktop app gets a Smart mode and keyframe markers on the
timeline. Out of scope: HEVC and AV1 smart cut, joining several clips, effects, any change to
upscale or GIF, multi-file input. Suggested success criteria: start and end within ±1 frame of the
request; at least 80% of frames stream-copied on long clips; no decode errors at the seams; every
risky source falls back with a code. Research: research/01-clipping-and-discovery.md §1.4.
Version 0.4.0 when done."

## Background

rmedia (001) cuts clips in two modes. **Fast** copies the source's compressed video untouched:
quick and lossless, but the clip can only start on a keyframe, so it may begin up to a few seconds
early. **Accurate** re-encodes the whole clip: it starts and ends on the exact frame, but it is
slow on long clips and loses some quality. **Smart** combines them: only the few frames between
the requested start and the next keyframe (the head) and between the last keyframe and the
requested end (the tail) are re-encoded; everything in between is copied untouched. The result is
frame-exact like accurate mode, and close to fast mode in speed and quality. The technique and its
pitfalls are described in `research/01-clipping-and-discovery.md` §1.4.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A frame-exact clip that keeps the original quality (Priority: P1)

A user wants a 2-minute highlight from a long H.264 recording, starting and ending on exact
frames. Accurate mode takes minutes and softens the picture; fast mode starts on the wrong frame.
With smart mode they get the exact range, almost all of it copied untouched, in a fraction of the
time accurate mode takes.

**Why this priority**: This is the feature. On its own it delivers value: one option on the
existing clip command gives frame-exact clips at near-original quality.

**Independent Test**: Cut a 60 s range whose start and end both fall mid-GOP from a generated
1080p H.264 test video (2 s GOPs) in mp4 with smart mode. The output starts and ends within one
frame of the request, at least 80% of its video frames are copied byte-for-byte from the source,
it decodes with no errors, and the run reports the copied and re-encoded shares.

**Acceptance Scenarios**:

1. **Given** an H.264 source in mp4, mov, mkv or ts and a range whose ends fall mid-GOP, **When**
   the user clips it with smart mode, **Then** the output's first and last frames are within one
   frame of the requested start and end, and the frames between the first and last keyframe of
   the range are copied untouched.
2. **Given** a range whose start falls exactly on a keyframe, **When** the user clips it with
   smart mode, **Then** no head is re-encoded (likewise for the end and the tail).
3. **Given** a source with audio, **When** the user clips it with smart mode, **Then** the audio
   covers exactly the clip's range and stays in sync with the video at the start, at each seam and
   at the end.
4. **Given** a finished smart clip, **When** the run reports it, **Then** it states the mode used
   (smart), the actual start and end, and how many frames (and what share of the duration) were
   copied and re-encoded.
5. **Given** the user cancels during a smart job, **When** cancellation completes, **Then** within
   2 s no partial output, part file, scratch file or tool process is left.

---

### User Story 2 - Risky sources fall back safely and say why (Priority: P2)

A user runs smart mode on whatever they have: phone recordings (variable frame rate), HEVC
footage, interlaced TV captures, 10-bit files. They never want a broken or glitching clip. When
smart mode cannot be trusted for a source, the clip is cut in accurate mode instead and the run
says which rule was hit.

**Why this priority**: Smart cutting fails in subtle ways (green frames, stutter, decoder errors
at the joins) on sources it does not handle. Safe fallback is what makes the mode usable at all.

**Independent Test**: Run smart mode on a generated set of risky sources (HEVC, interlaced,
variable frame rate, open-GOP, 10-bit, a range shorter than one GOP, a source whose parameters
change mid-stream). Every clip is produced in accurate mode, frame-exact, and each job reports a
distinct, stable reason code.

**Acceptance Scenarios**:

1. **Given** a source smart mode does not handle (FR-007), **When** the user clips it with smart
   mode, **Then** the job is cut in accurate mode, succeeds, and reports the fallback with its
   reason code and a plain-language message.
2. **Given** a range that contains no complete GOP, **When** the user clips it with smart mode,
   **Then** the job falls back to accurate mode with the "no full GOP" reason.
3. **Given** a smart clip whose seam check fails after the join (FR-010), **When** the job
   finishes, **Then** the failed result is discarded, the clip is cut again in accurate mode, and
   the fallback is reported with the seam-check reason.
4. **Given** the user asked for no fallback, **When** smart mode cannot be used, **Then** the job
   fails with the reason code and a documented exit code, and nothing is written.
5. **Given** a dry run, **When** a job would fall back, **Then** the plan already shows the
   fallback and its reason for every rule that can be checked before encoding.

---

### User Story 3 - Know before running what will be copied (Priority: P3)

Before committing to a batch, the user wants to know which clips will really be smart-cut, how
much of each will be copied against re-encoded, and which will fall back.

**Why this priority**: It makes the mode predictable and lets users fix ranges (or accept the
fallback) before spending time on a large batch.

**Independent Test**: Dry-run a batch of five smart clips (three eligible, two falling back) and
check that the plan lists, per job, the mode that will be used, the head, copied and tail spans,
the copied share, and the fallback reasons, with nothing written.

**Acceptance Scenarios**:

1. **Given** smart jobs in a dry run, **When** the plan is shown, **Then** each job lists its head,
   copied and tail spans (start, end, frame count), the share of the clip that will be copied, and
   the mode that will actually run, in both the human and the JSON output.
2. **Given** a batch, **When** the plan is shown, **Then** a summary gives how many jobs will be
   smart-cut and how many will fall back, grouped by reason.

---

### User Story 4 - Smart mode in batches and manifests (Priority: P4)

A user who keeps clip lists and manifests (001) wants to choose smart mode per run, per video or
per clip, in JSON and CSV manifests alike, and get the same result from the CLI and the desktop
app.

**Why this priority**: Most real use is batched; the mode has to travel with the work.

**Independent Test**: A JSON manifest and an equivalent CSV manifest, each with `mode` set to
smart at different levels, produce identical outputs from the CLI and from the desktop app, and an
exported manifest round-trips the mode.

**Acceptance Scenarios**:

1. **Given** a manifest with `mode: smart` in its defaults, on a video or on a single clip,
   **When** the batch runs, **Then** the most specific setting wins, as for the other modes.
2. **Given** an older manifest without smart, **When** it is loaded, **Then** it behaves exactly as
   before.
3. **Given** a manifest exported from the desktop app with smart clips, **When** it is run by the
   CLI, **Then** the outputs are identical to the app's.

---

### User Story 5 - Smart mode in the desktop app with keyframe markers (Priority: P5)

In the desktop app the user picks Smart next to Fast and Accurate, sees the source's keyframes as
markers on the timeline, and sees, for the selected range, which parts will be copied and which
re-encoded, before running.

**Why this priority**: The markers make the mode understandable (why a short range falls back, why
moving the start a little copies more), but the CLI already delivers the core value.

**Independent Test**: Open an H.264 video, choose Smart, select a range across several keyframes,
and check that the markers match the source's keyframes, the head and tail spans are shown, and
the run's report matches what was shown.

**Acceptance Scenarios**:

1. **Given** an opened video, **When** the timeline is shown, **Then** keyframe markers appear at
   the source's keyframe times (within one frame) without blocking the view while they load.
2. **Given** Smart is selected and a range is set, **When** the range changes, **Then** the
   copied and re-encoded spans and the copied share update, and a source or range that would fall
   back says so with the reason before the run.
3. **Given** a finished smart run, **When** the results are shown, **Then** each clip shows the
   mode used and, for a fallback, the reason.

---

### Edge Cases

- The start is on a keyframe and the end is on a keyframe: nothing is re-encoded; the clip is a
  pure copy and still frame-exact.
- The range ends after the source's last keyframe: the tail runs from that keyframe to the end.
- The end is "end of media": the tail is not re-encoded (the copy runs to the last frame).
- The range is shorter than one GOP or contains no complete GOP: fall back (no full GOP).
- The source has B-frames: frames after the first copied keyframe that depend on frames before it
  must not appear broken; if that cannot be guaranteed, the source counts as open-GOP and falls
  back.
- The keyframes are non-IDR recovery points rather than IDR frames: treated as open-GOP (fall
  back) unless the plan proves a clean boundary.
- Stream parameters change inside the range (resolution, profile, frame rate, colour settings):
  fall back (mismatched parameters).
- The source has rotation metadata, a non-square pixel aspect ratio or colour tags: the output
  keeps them, and the re-encoded parts match them; if they cannot be matched, fall back.
- No suitable H.264 encoder is available in the engine: smart jobs fall back with the
  "encoder missing" reason, and diagnostics say smart mode is unavailable.
- Several audio tracks or subtitle tracks: the same streams are kept as in accurate mode; text
  subtitles are cut by time.
- Cancelling between the head, middle, tail and join steps: all intermediate parts are removed.
- Disk full or an unwritable output folder during a part: the job fails cleanly, no partial
  output.
- The clip also asks for upscaling or a GIF (002, 003): those re-encode the whole clip anyway, so
  the job runs as accurate and the report says smart mode did not apply.
- Smart mode is chosen for a container other than mp4, mov, mkv or ts: fall back (container).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Users MUST be able to choose a third clip mode, smart, wherever fast and accurate can
  be chosen today (CLI flag, settings default, clip list, JSON and CSV manifest at run, video and
  clip level, and the desktop app). Fast stays the default.
- **FR-002**: A smart clip MUST start and end within one frame of the requested start and end
  (the same precision as accurate mode).
- **FR-003**: In a smart clip, every video frame from the first keyframe at or after the start up
  to the last keyframe the copy can end on MUST be copied unchanged from the source; only the head
  (start to that first keyframe) and the tail (from the copy's end to the requested end) are
  re-encoded.
- **FR-004**: The re-encoded head and tail MUST match the copied part's stream parameters
  (codec, profile and level compatibility, resolution, pixel aspect ratio, pixel format and bit
  depth, frame rate and timebase, colour settings, field order), so that the joined clip decodes
  as one continuous stream; their visual quality MUST be at least that of accurate mode at its
  default settings.
- **FR-005**: Audio MUST cover exactly the clip's range and stay in sync with the video; it MAY be
  re-encoded for the whole clip (audio is cheap) rather than smart-cut. Stream selection
  (audio, subtitles, data) MUST be the same as in accurate mode.
- **FR-006**: Smart cutting MUST support H.264 video in mp4, mov, mkv and ts containers; the
  output container follows the same rules as the other modes.
- **FR-007**: Before encoding, the system MUST check the source and range and fall back to
  accurate mode, with a distinct stable reason code, for: a video codec other than H.264
  (HEVC and AV1 included); a container other than those of FR-006; interlaced video; variable
  frame rate; open-GOP or non-IDR boundaries; stream parameters that change within the range or
  cannot be matched (FR-004); an H.264 profile other than Baseline, Main or High; bit depth above
  8 bits or chroma other than 4:2:0; a display rotation the output cannot carry; no complete GOP
  inside the range; and no suitable encoder in the engine.
- **FR-008**: A fallback MUST NOT be an error: the job succeeds in accurate mode and reports the
  fallback, its reason code and a plain-language message, in the human and the JSON output.
- **FR-009**: Users MUST be able to refuse the fallback (per run, CLI and manifest); then a job
  that cannot be smart-cut fails with its reason code and a documented exit code, writing
  nothing.
- **FR-010**: After the join, and before the output is committed, the system MUST verify the
  result: it decodes without errors across both seams, its frame count and its start and end
  match the plan (FR-002), and its timestamps increase steadily across the seams. A clip that
  fails the check MUST be discarded and cut again in accurate mode, reported with the seam-check
  reason (or failed, under FR-009).
- **FR-011**: The plan (dry run and the run's report) MUST show per smart job: the mode that will
  run, the head, copied and tail spans (times and frame counts), the copied share, and any
  fallback reason that can be known before encoding; a batch summary MUST count smart jobs and
  fallbacks by reason.
- **FR-012**: The results MUST report per job the mode actually used, the actual start and end,
  the copied and re-encoded frame counts and shares, and, for a fallback, the reason.
- **FR-013**: When a clip also requests upscaling (002) or a GIF or Reddit MP4 (003), the job MUST
  run as accurate, and the report MUST say that smart mode did not apply; upscale and GIF
  behaviour MUST NOT change.
- **FR-014**: Manifests (JSON and CSV) MUST accept `mode` set to smart, and the no-fallback choice,
  at the same levels as the existing mode; manifests without them MUST behave exactly as in
  0.3.0; exported manifests MUST round-trip both.
- **FR-015**: The desktop app MUST offer Smart next to Fast and Accurate, show the source's
  keyframes as markers on the timeline, show the head, copied and tail spans and the copied share
  of the selected range, and say before the run when a range or source will fall back and why.
- **FR-016**: Keyframe discovery MUST be bounded in time and memory, cancellable, and MUST NOT
  block the desktop app while it runs; for long sources it MAY be limited to the area around the
  selected range.
- **FR-017**: Diagnostics MUST report whether smart mode is available (a suitable encoder and the
  features it needs) and, if not, why.
- **FR-018**: All 001 guarantees MUST hold for smart jobs: sources never modified; outputs written
  under a temporary name, verified and committed without clobbering; progress with ETA across the
  head, copy, tail, join and check steps; cancellation within 2 s with no partial files, scratch
  files or stray processes; JSON output; identical outputs for identical inputs and options.
- **FR-019**: CLI and desktop app MUST give identical outputs for the same source, range and
  options (feature parity).

### Key Entities

- **Clip mode**: fast, accurate or smart; set at run, video or clip level, the most specific
  winning.
- **Keyframe index**: the keyframe times of a source's video stream (whole source or around a
  range), with whether each is a clean (IDR) boundary.
- **Smart plan**: for one clip, the head, copied and tail spans with their times and frame
  counts, the copied share, the stream parameters to match, and the eligibility verdict (smart, or
  fallback with a reason).
- **Fallback reason**: a stable code and a plain-language message (encoder missing, codec,
  container, profile, pixel format, interlaced, variable frame rate, rotation, no full GOP, open
  GOP, mismatched parameters, seam check).
- **Smart result**: the mode actually used, actual start and end, copied and re-encoded frame
  counts, and the fallback reason if any.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the generated eligible test set (H.264 in mp4, mov, mkv and ts, with and without
  B-frames and audio), 100% of smart clips start and end within one frame of the request.
- **SC-002**: For clips at least ten GOPs long, at least 80% of video frames are copied unchanged
  from the source.
- **SC-003**: 0 decode errors and 0 timestamp discontinuities at the seams across the whole test
  set, and audio stays within one audio frame (about 20 ms) of the video at every seam.
- **SC-004**: 100% of the generated risky sources (FR-007) are cut in accurate mode, frame-exact,
  each reporting its expected reason code.
- **SC-005**: A 60 s range of a 1080p H.264 source is smart-cut in at most one third of the time
  accurate mode takes for the same range on the same machine.
- **SC-006**: The re-encoded head and tail score at least as well as accurate mode's output for
  the same frames on the perceptual-quality measure used in 002, and copied frames are identical
  to the source.
- **SC-007**: In the desktop app, a user makes a smart clip of a chosen range in 3 interactions or
  fewer after opening the video, and the keyframe markers appear within 2 s for a 10-minute
  source.
- **SC-008**: Cancellation stops a smart job within 2 s with no partial files or stray processes,
  on Linux and Windows.
- **SC-009**: Manifests from 0.3.0 produce identical outputs in 0.4.0, and JSON and CSV manifests
  with smart clips give identical outputs from the CLI and the desktop app.

## Assumptions

- Fast stays the default mode; smart is opt-in, since it is new and depends on encoder matching.
- Audio is re-encoded for the whole smart clip with accurate mode's audio settings, which keeps it
  sample-exact and in sync at little cost; copying audio is a possible later refinement.
- The bundled engine's H.264 encoder is used for the head and tail; matching the source's exact
  encoder settings is not required, only decode compatibility (FR-004), which the seam check
  enforces.
- "Long clips" in SC-002 means at least ten GOPs; short clips naturally have a larger re-encoded
  share.
- Only one video stream (the first) is smart-cut, as the other modes cut only one.
- Out of scope: HEVC and AV1 smart cut, joining several clips, effects or filters, any change to
  upscale or GIF, multi-file input.
- The fallback reason codes are stable once released and are documented with the other issue
  codes.
- Version 0.4.0 ships when this feature is done; 005 (budgeted Reddit video) follows.
