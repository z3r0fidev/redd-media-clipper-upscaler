# Feature Specification: Video Clipping Base

**Feature Branch**: `001-video-clipping`

**Created**: 2026-10-06

**Status**: Draft

**Input**: User description: "Create a CLI and native application for Linux and Windows that can create
clips from a video input using manual timestamps, multiple clipping via text file, and batch clipping
via the use of an input file. The application surfaces should be capable of parsing a directory to
identify video archives if an input directory is provided."

**Decisions already made by the owner**:
- "Video archive" means the user's stored collection of video files. Compressed archives
  (zip/7z/rar/tar) are NOT opened or extracted.
- The video processing engine is bundled with the installer; users do not install it separately.
- Upscaling for mobile displays is a separate, later feature (002) and is out of scope here.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Cut a single clip with manual timestamps (Priority: P1)

A user has one video and wants one segment of it. They give the video, a start time and either an
end time or a duration, and get a new clip file. They can choose **fast** mode (near-instant, no
quality loss, but the cut may land slightly before the requested start because it snaps to the
nearest preceding keyframe) or **accurate** mode (frame-accurate, re-encoded, slower).

**Why this priority**: This is the smallest unit of value; every other story is built from it.

**Independent Test**: Run the CLI against a generated 60-second test video with
`start=00:00:10, end=00:00:20` and check that the output exists, plays, and lasts 10 s (±0.1 s in
accurate mode; within one keyframe interval in fast mode). Repeat in the GUI.

**Acceptance Scenarios**:

1. **Given** a valid video and start `00:01:00` / end `00:01:30`, **When** the user clips in accurate
   mode, **Then** a 30 s clip (±0.1 s) with the source's audio and video streams is written to the
   output location and the source file is byte-for-byte unchanged.
2. **Given** the same input in fast mode, **When** the clip completes, **Then** the clip contains no
   re-encoded frames, and the tool reports the actual start/end it produced if they differ from the
   request.
3. **Given** start `90` (seconds) and duration `+15`, **When** the user clips, **Then** the clip
   covers 90 s–105 s.
4. **Given** an end time beyond the video's length, **When** the user clips, **Then** the clip ends
   at the end of the video and a warning is shown.
5. **Given** an end time earlier than the start time, **When** the user submits, **Then** the request
   is rejected before any processing with a message naming the problem.

---

### User Story 2 - Multiple clips from one video via a clip list file (Priority: P2)

A user writes a plain text file listing several segments (one per line: start, end, optional name)
for a single video and gets one output file per line.

**Why this priority**: The most common real workflow (e.g. extracting highlights) and the user's
explicitly requested second mode.

**Independent Test**: Provide a 3-line clip list against a test video; verify three correctly named
outputs of the right durations.

**Acceptance Scenarios**:

1. **Given** a clip list with 3 valid lines and comment/blank lines, **When** run, **Then** exactly
   3 clips are produced, named from the optional name column or numbered automatically.
2. **Given** a clip list where line 2 is malformed, **When** the user runs a validation or dry run,
   **Then** the tool reports the file, line number and reason, and produces no files.
3. **Given** a clip list where one clip fails during processing, **When** run, **Then** the other
   clips still complete and the summary lists the failure.

---

### User Story 3 - Batch clipping across many videos via a manifest (Priority: P3)

A user prepares one input manifest that names multiple videos, each with one or more clips, plus
optional shared defaults (mode, output folder, naming pattern), and runs it as a single batch.

**Why this priority**: Enables bulk/archival workflows; builds on stories 1–2.

**Independent Test**: A manifest referencing 3 test videos with 2 clips each yields 6 correct
outputs and a summary report.

**Acceptance Scenarios**:

1. **Given** a valid manifest for 3 videos × 2 clips, **When** run, **Then** 6 clips are produced and
   a summary reports 6 succeeded, 0 failed.
2. **Given** a manifest with relative video paths, **When** run from any working directory,
   **Then** paths resolve relative to the manifest file's location.
3. **Given** a manifest referencing a missing video, **When** validated, **Then** the missing file
   is reported and (unless the user opts to continue) nothing runs.
4. **Given** a manifest in spreadsheet-style (comma-separated) form or structured form, **When**
   run, **Then** both produce the same jobs for equivalent content.

---

### User Story 4 - Discover videos in a directory (Priority: P2)

A user points the app at a folder (their video archive). The app recursively finds all video
files, ignoring non-video files, and lists them with basic properties (duration, resolution).
The discovered list can then be used as the input set for clipping (e.g. apply the same clip
list to every video, or pick videos in the GUI).

**Why this priority**: Explicitly required for every surface; also the GUI's main entry point.

**Independent Test**: A folder tree with 4 videos (some nested, one with a misleading extension,
plus text/image files) yields exactly the 4 real videos.

**Acceptance Scenarios**:

1. **Given** a nested folder with videos, images and text files, **When** scanned, **Then** only
   playable video files are listed, with path, duration, resolution and size.
2. **Given** a file named `.mp4` that is not actually a video, **When** scanned, **Then** it is
   excluded and reported as unreadable (in verbose/diagnostic output).
3. **Given** a folder containing a `.zip` that contains videos, **When** scanned, **Then** the zip is
   ignored (not opened).
4. **Given** a scan with a "non-recursive" option, **When** run, **Then** subfolders are not visited.
5. **Given** a folder of discovered videos and one clip list, **When** the user applies it to all,
   **Then** each video gets the listed clips in its own output subfolder.

---

### User Story 5 - Desktop application for the same workflows (Priority: P2)

A non-technical user opens the desktop app on Linux or Windows, adds a video (file picker or drag
and drop) or a folder, previews the video, marks in/out points, adds clips to a queue (or imports a
clip list / manifest), chooses an output folder and mode, and runs the queue while watching
progress per job and overall. They can cancel a single job or the whole queue.

**Why this priority**: Required native surface; depends on the core behaviour from stories 1–4.

**Independent Test**: In the GUI, add a test video, mark 2 segments, run, and confirm 2 outputs that
match the same request run via the CLI.

**Acceptance Scenarios**:

1. **Given** a loaded video, **When** the user sets in/out points with the player and adds the clip,
   **Then** the queue shows the clip with exact start/end/duration values that are also editable as
   text.
2. **Given** a running queue, **When** the user cancels it, **Then** processing stops within 2 s, no
   partial output files remain, and completed clips are kept.
3. **Given** the same inputs, **When** run in the GUI and the CLI, **Then** the outputs are identical.

---

### Edge Cases

- Paths with spaces, Unicode, very long paths, network shares and Windows drive letters.
- Output file already exists → default is to skip and report; overwrite or auto-rename only when
  chosen explicitly.
- Output folder is the same as the source folder → allowed, but the source is never overwritten
  (a generated name may never equal a source path).
- Overlapping clips in a list → allowed; each produces its own file.
- Zero-length or negative-length clips → rejected at validation.
- Videos with variable frame rate, no audio, multiple audio tracks, or subtitles → all selected
  streams that the output format supports are kept; unsupported ones are dropped with a warning.
- Very large directory trees (10,000+ files) → the scan shows progress and can be cancelled.
- Symbolic links / junctions → not followed by default, which also avoids infinite loops.
- Insufficient disk space or a read-only output folder → the job fails with a clear message and no
  partial file remains.
- The application is closed or killed mid-batch → no orphaned background processes remain;
  partial outputs are cleaned up on the next launch.
- Timestamps given as `1:30`, `90`, `90.5`, `00:01:30.500`, `00:01:30,500`, `1m30s` all mean the
  same instant.

## Requirements *(mandatory)*

### Functional Requirements

**Input & timestamps**
- **FR-001**: The system MUST accept a single video plus a start time and either an end time or a
  duration, and produce one clip.
- **FR-002**: The system MUST accept timestamps as seconds (with optional decimals),
  `[HH:]MM:SS[.mmm]` (period or comma decimal separator), and unit form (`1h2m3.5s`); durations are
  written with a leading `+`. `end` (or omitted end) means end of video.
- **FR-003**: The system MUST validate all clip requests (start < end, start within the video,
  positive duration) before any processing begins, and report every problem at once.

**Clip list (multi-clip) file**
- **FR-004**: The system MUST accept a plain text clip list for one video: one clip per line as
  `START END [NAME]` or `START +DURATION [NAME]`, with whitespace or comma separators, `#` comments
  and blank lines ignored.
- **FR-005**: The system MUST report malformed lines with file name, line number and reason.

**Batch manifest**
- **FR-006**: The system MUST accept a batch manifest that lists multiple videos, each with one or
  more clips, with optional global defaults (mode, output folder, naming pattern, overwrite policy)
  and per-video/per-clip overrides.
- **FR-007**: The manifest MUST be accepted in a structured form and in a comma-separated tabular
  form (one row per clip: input, start, end-or-duration, name, mode, output); equivalent content
  MUST yield identical jobs.
- **FR-008**: Relative paths inside clip lists and manifests MUST resolve against the file's own
  location.
- **FR-009**: The system MUST allow one clip list to be applied to every video in a set (a list of
  files or the result of a directory scan).

**Directory discovery**
- **FR-010**: Given a directory, the system MUST recursively (by default) find video files, confirm
  each is a readable video with at least one real video stream (cover art does not count), and list
  path, duration, resolution, frame rate, codec and size.
- **FR-011**: Discovery MUST NOT open compressed archives, MUST skip hidden and temporary/partial
  files by default, MUST NOT follow symbolic links by default, and MUST support include/exclude
  patterns and a non-recursive option.
- **FR-012**: Discovery MUST report progress and be cancellable; results MUST be exportable as a
  list usable as batch input.

**Clipping modes & output**
- **FR-013**: The system MUST offer **fast** mode (no re-encoding; snaps to keyframes) and
  **accurate** mode (frame-accurate; re-encoded at visually lossless default quality). Fast is the
  default for the CLI and GUI.
- **FR-014**: In fast mode the system MUST report the actual start and end of each produced clip
  when they differ from the request by more than 0.05 s.
- **FR-015**: The system MUST preserve all video, audio and subtitle streams by default and allow
  stream selection (e.g. drop audio, choose audio track).
- **FR-016**: Output names MUST follow a configurable pattern with placeholders (source name, clip
  index, clip name, start, end) and default to `{source}_{index:03}_{name-or-range}.{ext}` (name, or `START_END` when unnamed, so
  clips with the same start but different ends never collide) in the
  chosen output folder (default: a `clips` folder next to the source).
- **FR-017**: The default output container MUST match the source container; the user MAY choose
  another container. Exception: when accurate mode cannot re-encode into the source container
  (e.g. AVI, WMV, FLV, MPEG-PS, 3GP, OGV), the default is MP4 and the user is warned.
- **FR-018**: The system MUST NEVER modify, move or delete source files, and MUST NOT overwrite an
  existing file unless the user explicitly chose overwrite; the default collision policy is skip.

**Execution**
- **FR-019**: The system MUST provide a dry run that lists every planned output (and the conflicts it
  would hit) without writing anything.
- **FR-020**: The system MUST process jobs with a configurable concurrency limit (default: half the
  CPU cores, minimum 1, maximum 4), show per-job and overall progress with time remaining, and continue past
  individual job failures.
- **FR-021**: Users MUST be able to cancel a single job or the whole run; cancelled or failed jobs
  MUST leave no partial files, and no background processes may remain.
- **FR-022**: At the end of every run the system MUST present a summary (succeeded / skipped /
  failed with reasons) and, in the CLI, return a non-zero exit status when any job failed.

**Surfaces**
- **FR-023**: A command-line tool and a desktop application MUST both be provided for Linux and
  Windows, offering the same capabilities (FR-001 – FR-022).
- **FR-024**: The CLI MUST offer machine-readable output for scan results, plans and run summaries.
- **FR-025**: The desktop app MUST offer a video preview with scrubbing and setting in/out points,
  a clip queue editable as text values, import of clip lists and manifests, export of the queue as a
  manifest, drag-and-drop of files and folders, and per-job progress with cancel.
- **FR-026**: Both surfaces MUST provide a diagnostics view/command reporting the bundled video
  engine version and its available capabilities.
- **FR-027**: The application MUST work out of the box after installation with no separate install
  of the video processing engine; installers MUST include the licence notices required for bundled
  components.

### Key Entities

- **Source Video**: a discovered or user-supplied video file; attributes: path, duration, container,
  streams (video/audio/subtitle), resolution, frame rate, size, readable flag.
- **Clip Request**: start, end-or-duration, optional name, mode, stream selection, output overrides;
  belongs to one Source Video.
- **Clip List**: a text file of Clip Requests for one (or each of several) Source Videos.
- **Batch Manifest**: defaults + a set of (Source Video, Clip Requests) entries.
- **Job**: one Clip Request resolved to a concrete output path; status (planned, skipped, running,
  succeeded, failed, cancelled), progress, actual start/end, error.
- **Run**: a set of Jobs executed together, with a concurrency limit and a final summary.
- **Scan Result**: a set of Source Videos found under a directory, plus unreadable/skipped entries.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A fast-mode clip of any length from a 1080p source completes in under 5 seconds on a
  4-core machine with SSD storage, independent of clip length.
- **SC-002**: Accurate-mode clips start and end within one frame of the requested times in 100% of
  automated test cases.
- **SC-003**: A directory of 10,000 files containing 1,000 videos is scanned and listed in under
  60 seconds on a local SSD.
- **SC-004**: A batch of 100 clips across 20 videos runs unattended to completion with a correct
  summary; one injected failure does not stop the other 99.
- **SC-005**: A first-time user can install the desktop app and produce their first clip in under
  5 minutes without reading documentation.
- **SC-006**: 100% of malformed clip list / manifest lines are reported with a line number before any
  processing begins.
- **SC-007**: Cancelling a run stops all processing within 2 seconds and leaves zero partial output
  files and zero orphaned processes on both Linux and Windows.
- **SC-008**: The same request produces identical outputs from the CLI and the desktop app.

## Assumptions

- Target platforms are 64-bit Linux (mainstream desktop distributions) and 64-bit Windows 10/11;
  macOS is out of scope.
- Users run the app on their own machine against local or mounted storage; there are no accounts,
  network services, or telemetry.
- "Visually lossless" accurate-mode quality uses widely compatible codecs; the user can override.
- Smart/hybrid cutting (re-encoding only the clip boundaries) is a later enhancement; fast and
  accurate modes cover v1.
- Frame-number and SMPTE timecode input, chapter/EDL import, GIF and audio-only export are out of
  scope for v1 (candidates for later).
- Mobile-display upscaling, reframing and device presets are delivered in feature 002 and reuse this
  feature's job/queue model.
- The application's own licence permits shipping a GPL-licensed video engine build alongside it
  (separate executable, licence notices and source offer included); to be confirmed by the owner
  before the first public release.
