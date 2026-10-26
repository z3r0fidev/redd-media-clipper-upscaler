# rmedia Domain Breakdown

**Status:** Current (0.1.0, commit `bd1eef8`). This page lists, for each domain, what it is
responsible for, its key types and files, and the invariants it upholds. For the system-level
view, see the [Architecture Overview](overview.md).

All paths are relative to the repository root. Core files are under `crates/rmedia-core/src/`.

## Time and Parsing

**Responsibility:** the timestamp grammar (FR-002) and time arithmetic.

| Item | Location |
|---|---|
| `Ts` — non-negative time in microseconds (`i64`) | `time.rs` |
| `EndSpec` — `At(Ts)` / `Duration(Ts)` / `EndOfMedia` | `time.rs` |
| `TimeError` — `invalid time "<input>": <reason>` | `time.rs` |

**Invariants**

- Accepted positions: plain seconds (`90`, `90.5`), clock form (`1:30`, `00:01:30.500`,
  `00:01:30,500`), and unit form (`1h2m3.5s`, `500ms`). `end` and `+<duration>` are handled by
  `EndSpec::parse`. Full grammar: [File Formats › Time grammar](../api/file-formats.md#time-grammar).
- Rejected: negatives, exponents, more than 6 fractional digits, non-leading fields of 60 or more,
  and units out of order or repeated.
- JSON serialization is always seconds as a number. Deserialization accepts a number or a grammar
  string.
- Display formats: `HH:MM:SS.mmm` (`display`), `HH-MM-SS.mmm` (`filename`, safe on every OS), and
  `S.ffffff` (`ffmpeg`).

## Request Model

**Responsibility:** the single normalized input form for planning (data-model.md).

| Item | Location |
|---|---|
| `Options`, `PartialOptions` (`apply`, `diff`), `Mode`, `OnExists`, `AudioSel`, `SubSel`, `StreamSelection` | `request.rs` |
| `ClipRequest`, `SourceRequest`, `BatchRequest`, `Origin` (`Cli`, `Gui`, `Line`, `JsonPath`) | `request.rs` |
| `DEFAULT_NAME_TEMPLATE` = `{source}_{index:03}_{name-or-range}.{ext}`; `MULTI_SOURCE_NAME_TEMPLATE` = `{source}/{source}_{index:03}_{name-or-range}.{ext}` | `request.rs` |

**Invariants:** options layer as defaults < source < clip (`PartialOptions::apply`).
`PartialOptions` rejects unknown fields. `diff` is the inverse of `apply` (unit-tested).

## Input Formats

**Responsibility:** turn user files into `BatchRequest`s and report every problem at once
(FR-003, FR-005, SC-006).

| Parser | File | Errors located by |
|---|---|---|
| Clip list (`START END|+DUR [NAME]`) | `input/cliplist.rs` | file:line |
| JSON manifest v1 (strict) + writer | `input/manifest_json.rs` | Syntax/type: line (message includes column). Semantic: JSON path such as `videos[0].clips[1].start`. |
| CSV manifest + writer | `input/manifest_csv.rs` | file:line (comment lines accounted for) |
| Path list (`--inputs-from`) | `input/pathlist.rs` | (no errors; verbatim lines) |

**Invariants**

- UTF-8 only. A leading BOM is tolerated. Invalid UTF-8 is reported with the offending line.
- Relative inputs and output folders resolve against the defining file's folder (FR-008).
- JSON and CSV that express the same content produce identical plans (FR-007, tested).
- The JSON writer emits `HH:MM:SS.mmm` strings when a time is exact to the millisecond, otherwise
  seconds, so write → parse round-trips losslessly. The CSV writer folds options per row and drops
  options CSV cannot express (container, name template, streams).

## Engine Location and Diagnostics

**Responsibility:** find ffmpeg/ffprobe, check the version, and detect capabilities (Principle VI,
FR-026).

| Item | Location |
|---|---|
| `tools::locate` — explicit path → `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` → bundled `rmedia-ffmpeg[.exe]`/`rmedia-ffprobe[.exe]` beside the executable → `PATH` | `tools.rs` |
| `ToolSource` = `config` (flag, settings or env var) / `bundled` / `path` | `tools.rs` |
| `MIN_VERSION` = 6.0. Git/nightly version strings count as supported. | `tools.rs` |
| `doctor::run` → `DoctorReport` + `(Tools, Caps)`; `Caps` (encoders, filters, models, face detector, capability record) | `doctor.rs` |
| `doctor::check_capability` (async): adds the machine capability record (tiers, GPUs) to a report and to `Caps` | `doctor.rs`, `upscale/capability.rs` |

**Invariants:** the doctor never fails, it reports problems instead. Accurate-mode containers are
listed only when both their video and audio encoders exist. Explicit paths are canonicalized so
spawning never depends on the working directory.

## Discovery

**Responsibility:** find real videos in folder trees (US4, FR-010–FR-012, research.md R6).

| Item | Location |
|---|---|
| `discover::scan(tools, roots, ScanOptions, on_event, cancel)` → `ScanResult` | `discover.rs` |
| `ScanOptions` (`no_recursive`, `max_depth`, `include`, `exclude`, `hidden`, `follow_symlinks`, `exclude_dirs`, `exclude_dir_names`) | `discover.rs` |
| `ScanEvent` = `progress` / `found` / `rejected`; `RejectReason` = `unreadable` / `no_video_stream` / `image` / `inaccessible` | `discover.rs` |
| `VIDEO_EXTENSIONS` (22): mp4 m4v mov mkv webm avi divx wmv asf flv f4v mpg mpeg m2ts mts ts vob 3gp 3g2 ogv mxf qt | `discover.rs` |
| `path_key` — identity key: exact bytes on Unix, lower-cased on Windows | `discover.rs` |

**Invariants**

- The walk runs in `spawn_blocking`. Probing runs on runtime workers in a fixed pool of
  min(parallelism, 16), which keeps it PDEATHSIG-safe.
- Symlinks are not followed by default, and a file swapped for a symlink after the walk is
  rejected.
- Hidden entries (dot-names, plus Hidden/System attributes on Windows), system folders
  (`$RECYCLE.BIN`, `System Volume Information`, `@eaDir`, `.git`, `.Trashes`, `.thumbnails`,
  `lost+found`, `.Trash-*`) and temp names (`.part`, `.crdownload`, `.tmp`, `.ytdl`, `.download`,
  `.!qb`, `.partial`, `._*`) are skipped and counted. On Windows, online-only cloud placeholders
  are skipped and counted.
- A file is a video only if ffprobe finds at least one video stream that is not attached-picture
  cover art. Image formats (`image2`, `*_pipe`) are rejected. Archives are never opened.
- A file given explicitly as a root bypasses the extension and include/exclude checks.
- Results are naturally sorted (`clip2` < `clip10`).

## Probing

**Responsibility:** ffprobe invocation and parsing into `MediaInfo`.

| Item | Location |
|---|---|
| `probe::probe` → `MediaInfo { path, size, container (extension), format_name, duration?, start_time, streams }` | `probe.rs` |
| `StreamInfo` (index, kind, codec, width/height, frame_rate, pix_fmt, is_attached_pic, language, start_time, duration) | `probe.rs` |
| `keyframe_at_or_before` (packet scan, 12 s then 65 s windows) → `Keyframe { at, previous }` | `probe.rs` |
| `file_arg` (`file:` prefix), `INPUT_PROTOCOLS = "file,pipe"` | `probe.rs` |

**Invariants:** a 60 s timeout per call; at most 16 MiB of captured stdout. Matroska stream
durations come from the `DURATION` tag. `has_indexed_seeking` uses `format_name`, never the
extension.

## Planning

**Responsibility:** turn a `BatchRequest` into validated `PlannedJob`s, writing nothing (FR-003,
FR-016–FR-019).

| Item | Location |
|---|---|
| `plan::plan` → `Plan { run_id, jobs, issues, cancelled }`; `Planner::plan_clip` | `plan/mod.rs` |
| `PlannedJob`; `Disposition` = `run` / `skip{reason}`; `JobSpec::Clip(ClipSpec)`, `JobSpec::Upscale(UpscaleSpec)` or `JobSpec::Gif(GifSpec)` (0.3.0). Since the 002 refactor A1, clip-only fields (`mode`, `requested`, `predicted`) live in `ClipSpec`. | `plan/mod.rs` |
| `Issue { severity, code, origin, message }`; `Plan::has_errors`, `engine_errors_only` | `plan/mod.rs` |
| Name templates and file name sanitizing | `plan/naming.rs` |
| `Container`, `EncodeProfile` (`X264_AAC`, `VP9_OPUS`), `SubtitlePolicy`, `select_streams` | `plan/streams.rs` |
| `ClipSpec`, `Trim`, `common_args`, `clip_args` (pure; insta snapshot-tested) | `plan/ffargs.rs` |

**Invariants**

- Every source is probed once (results cached per path). Playlist-like inputs (`m3u8`, `m3u`,
  `ffconcat`, `sdp`) are refused.
- `start` must be before the media duration. `end` must be after `start`. A zero duration is
  rejected. An end past the media is clamped to the duration with a warning.
- An output never equals a canonical source path (the nearest existing ancestor is canonicalized,
  then the remaining parts appended). Two clips cannot claim the same output unless
  `on_exists = rename`, which picks `name_1.ext`, `name_2.ext`, …
- An existing output means: `skip` → `Disposition::Skip`; `overwrite` → a warning; `rename` → a
  free name.
- Name templates must produce a relative path with no empty, `.` or `..` parts. Each component is
  sanitized (Windows-reserved characters, control characters and reserved device names such as
  `CON` or `COM¹`) and limited to 240 bytes.
- Streams: every real video stream; audio all/none/1-based tracks; subtitles copied (MKV),
  converted to `mov_text` (MP4/M4V/MOV) or `webvtt` (WebM) when text-based, otherwise dropped with
  a warning. Attachments are kept only in MKV. Cover art and data streams are never mapped.
- Accurate mode: a missing encoder is an `ENGINE_MISSING` issue. High-bit-depth sources get a
  warning that output is 8-bit 4:2:0.
- `run_id` is 64 bits from a `RandomState` hasher keyed by the OS RNG, so temp names are not
  predictable.
- Every ffmpeg call starts with `-hide_banner -nostdin -n -loglevel warning -progress pipe:1
  -nostats -stats_period 0.25`. Fast mode adds `-c copy -avoid_negative_ts make_zero`.
  MP4/M4V/MOV/3GP add `-movflags +faststart`. Chapters are dropped (`-map_chapters -1`) and
  metadata is kept (`-map_metadata 0`).

## Execution

**Responsibility:** run a plan with concurrency, progress, cancellation, verification, commit and
crash cleanup (FR-020–FR-022).

| Sub-domain | Key items | File |
|---|---|---|
| Scheduler | `exec::start` → `RunHandle` (`cancel`, `cancel_job`, `control()` → `RunControl`, `wait`); `RunConfig { jobs, state_dir }`; `default_jobs()`; `JobCtx` | `exec/mod.rs` |
| Clip job | `clip::execute` (run ffmpeg, re-probe, verify, measure); `classify_failure` (stderr → `DISK_FULL` / `OUTPUT_UNWRITABLE` / `ENGINE_FAILED`) | `exec/clip.rs` |
| Process | `run_capture`, `run_lines`; process group / PDEATHSIG / Job Object / kill-on-drop | `exec/process.rs` |
| Progress | `ProgressParser` (`out_time_us`, falling back to `out_time_ms` and `out_time`; `speed`; `progress=end`); `Throttle` | `exec/progress.rs` |
| Commit | `temp_path` → `<dir>/.<stem>.rmedia-<run>-<job>.part`; `commit(temp, output, OnExists)`; `remove_with_retry` (10 tries, 100 ms apart) | `exec/commit.rs` |
| Journal | `Journal::open/add/remove`, `Drop` cleans up; `purge_stale` | `exec/journal.rs` |
| Events | `Event`, `JobOutcome`, `JobResult`, `RunSummary`, `EventSink` (closures implement it) | `event.rs` |
| Errors | `Error` + stable `code()`; `ErrorInfo { code, message, stderr_tail? }` | `error.rs` |

**Invariants**

- Job states: queued → running → (verify) → succeeded / failed / cancelled; skipped jobs never
  start.
- Verification before commit: the stream count equals the mapped count, the duration is
  positive, and accurate clips with an explicit end inside the video stream must match the
  expected video length within 3 frames + 100 ms (else `VERIFY_FAILED`).
- Commit fsyncs the temp file (opened for writing, which Windows `FlushFileBuffers` requires),
  then:
  - `skip`: `hard_link(temp, output)`, which fails if the output exists, then remove temp. On
    filesystems without hard links, reserve the name with `create_new`, then rename over it.
  - `rename`: the same, trying `_1` … `_9999`.
  - `overwrite`: `rename`.
- The temp file is always removed after the job, whether it succeeded, failed or was cancelled.
- Journal: each run holds an exclusive OS file lock on its journal. `purge_stale` treats a
  lockable journal as belonging to a dead process. It deletes only entries that are absolute,
  named exactly `.<stem>.rmedia-<run_id>-<digits>.part` for that journal's run id, and regular
  files (never symlinks). Fresh empty journals (under 60 s old) are left alone.
- Exit code in `RunSummary`: 130 if the run was cancelled; otherwise 1 if any job failed or was
  individually cancelled; otherwise 0.

## Preview and Media Server

**Responsibility:** make any source viewable in a webview (research.md R8, FR-025).

| Item | Location |
|---|---|
| `preview::time_offset`, `frame_at` (one MJPEG frame ≤ `max_height`), `ensure_proxy` (480p H.264/AAC MP4 or VP8/Opus WebM, `-fps_mode passthrough`, `-g 15`), `thumbnails` (n clamped 1–60, 90 px tall JPEGs), `PreviewCache` (SHA-256 key, LRU `evict`) | `crates/rmedia-core/src/preview.rs` |
| `MediaServer::start`, `register(path) → URL`, `revoke_all` | `apps/desktop/src-tauri/src/media_server.rs` |

**Invariants:** proxy timestamps equal source timestamps. The server serves only registered,
canonical, regular files by opaque id, over GET/HEAD with a single byte range, 128 KiB chunks,
at most 8 connections, and one request per connection. Full list:
[Security Model › Media server](../security/security-model.md#media-server).

## Desktop App

### Backend (`apps/desktop/src-tauri/src/`)

| Item | Responsibility |
|---|---|
| `lib.rs` | Builder: dialog plugin, `AppState` setup (settings, cache dir, media server), command-line paths (allowed + handed to the page once), drag-drop paths registered before the page is told (`rmedia://dropped` event), `CloseRequested` → `AppState::shutdown` |
| `commands.rs` | 23 `#[tauri::command]`s; argument plumbing; native dialogs run in Rust |
| `ops.rs` | `AppState` with `Allowed` (files, dirs, save targets, lists), engine cache, plans by run id, run controls, scan tokens |

**Path policy invariants (`ops::Allowed`)**

- The page may only use paths the user chose: dialogs, drag-drop, command line, picked folders and
  their contents, or video files named by a manifest the user opened.
- Clip lists and manifests must have been picked as such.
- Exports may write only to the path chosen in the Save dialog.
- Planned outputs must be inside a picked folder or under the source's folder. Anything else adds
  an `INVALID_INPUT` issue.
- `set_settings` ignores engine paths. Only `pick_engine` (a native picker) changes them.
- `set_settings` accepts a changed default output folder only if it is already allowed (picked);
  `AppState::new` re-allows the saved folder at startup.
- A `.json` export folds the Settings `defaults` into the manifest's `defaults` (queue values win,
  `with_settings_defaults`), so `rmedia batch` reproduces the app's run.

### Frontend (`apps/desktop/src/`)

| Item | Responsibility |
|---|---|
| `lib/api.ts` | Typed `invoke` wrappers; `Channel` setup |
| `lib/bindings/*.ts` | ts-rs types generated from core (`cargo xtask gen-bindings`); CI fails on drift |
| `lib/manifest.ts` | `Manifest` / `VideoEntry` / `ClipEntry` — the queue is a v1 manifest |
| `lib/stores/queue.ts` | Pure, unit-tested queue operations (`addClip`, `updateClip` keeps end/duration exclusive, `merge`, `runnable`) |
| `lib/stores/run.ts` | Pure event → `RunView` reducer (`applyEvent`; progress never moves backwards) |
| `lib/stores/app.svelte.ts`, `sources.svelte.ts` | Svelte 5 rune state; add/scan/open actions |
| `lib/components/` | `Sources`, `Player` (direct → H.264 → VP8 → stills; keys), `Queue` (editable times via core parsing, import/export, plan/run), `RunPanel` (per-job and overall progress, cancel), `Settings` |

**Invariant:** all time parsing goes through the core (`parse_time` / `parse_end`), giving one
grammar everywhere (`lib/time.ts`).

## Upscaling (feature 002)

**Responsibility:** make a video phone-ready on the CPU, GPU or AI tier, driven by a device
preset: from `rmedia upscale`, from `rmedia clip --upscale` and batch manifests (an `upscale`
block), and from the desktop upscale view.
Code is in `crates/rmedia-core/src/{upscale,preset,models.rs}`, `exec/upscale.rs` and
`exec/pipeline.rs`, plus the AI worker crate `crates/rmedia-sr`. Added in 0.2.0 (unreleased).

| Item | Location |
|---|---|
| `UpscaleOptions` (preset, reframe, tier, hdr, motion, crf), `PartialUpscale`, `UpscaleSpec`, `UpscalePlan` | `upscale/mod.rs` |
| Displayed size, target fit, crop window, no-enlarge rule | `upscale/geometry.rs` |
| Single-pass `-filter_complex` builder: trim, pre-processing, reframe, scale, sharpen, tag | `upscale/graph.rs` |
| Preset to encoder, colour and tag arguments (`libx265` or `libx264`) | `upscale/encode.rs` |
| Output size and time estimates, platform limit warnings, free-space check | `upscale/estimate.rs` |
| Tier selection: `select(choice, record, size)`; auto takes the GPU only when it passed its self-test, is not slower than the CPU and has not failed since | `upscale/tier.rs` |
| Capability record: Vulkan listing, software-renderer rejection, fingerprint (versions, listing, driver manifests, `VK_*`), self-tests, `<state>/capability.json` under `capability.lock`, demotions | `upscale/capability.rs` |
| Presets: `PresetRegistry` (built-ins plus `<config>/presets/*.json`), JSON schema v1 | `preset/` |
| Model file lookup (`models_dir` setting, `models/` beside the executable, cache `models/`) | `models.rs` |
| Executor: analyze (smart), render, verify; emits `job_stage`; one CPU retry after an automatic GPU failure | `exec/upscale.rs` |
| Smart-reframe analysis: decode, YuNet, cuts, subject choice, crop path, sendcmd, cache | `upscale/reframe/*` |
| Manifest `upscale` block (JSON and CSV columns), `clip --upscale` (cut and upscale in one pass) | `input/manifest_json.rs`, `input/manifest_csv.rs`, `plan/mod.rs`, `plan/ffargs.rs` |
| Before/after preview frames (two PNGs; the standard scaler stands in for the AI tier) | `upscale/preview.rs` |
| AI tier: worker and model lookup with pinned SHA-256s (`PINNED`), scale choice, the `rmedia-sr` arguments | `upscale/ai.rs` |
| AI graphs: ffmpeg #1 frames the picture and writes rgb24; ffmpeg #2 scales to the target, de-flickers (`hqdn3d=0:0:3:3`), adds the blurred background and encodes | `upscale/graph.rs` (`ai_decode`, `ai_finish`) |
| The 3-process pipeline ffmpeg #1 → `rmedia-sr` → ffmpeg #2 over OS pipes: one process group on Unix (`killpg`), a Job Object per process on Windows; which stage to blame for a failure | `exec/pipeline.rs` |
| AI gate: `rmedia-sr --list-gpus` and `--selftest`; available when 60 s of 854x480 is predicted at 10 minutes or less | `upscale/capability.rs` (`ai_check`, `ai_gate`) |
| The worker: ncnn with Vulkan, frames on stdin/stdout, JSON lines on stderr, exit codes 2–6; `unsafe` only in `ffi.rs` | `crates/rmedia-sr` ([protocol](../../specs/002-mobile-upscale/contracts/rmedia-sr-protocol.md)) |
| Desktop: upscale options, preview, crop-path preview and queue commands | `apps/desktop/src-tauri/src/ops.rs`, `commands.rs` |

**Invariants**

- Upscale jobs always re-encode and always write `.mp4`; they never write next to the source
  outside an `upscaled/` folder (or `-o`).
- A source already at or above the target size is not enlarged (`no_upscale_needed`).
- `--reframe smart` follows faces (YuNet under tract, analysed at 576 px and 3 fps, cached in
  `<cache>/reframe/`); without a model, or where no face fits, it uses the blurred background,
  and the plan says so (`reframe_applied`, `fallback_ranges`).
- A forced tier the machine cannot run is a planning error (`TIER_UNAVAILABLE`); a forced tier
  never falls back. An automatic GPU job that fails is retried once on the CPU, and the GPU is
  demoted only when that retry succeeds.
- GPU jobs pin the Vulkan device (`-init_hw_device vulkan=vk:N -filter_hw_device vk`) and hold the
  single `Resource::Gpu` slot.
- Capability probes run through `exec::process` with per-probe and total time limits; a check
  cut short is never stored.
- The job runs in a scratch folder next to the output (`.<stem>.rmedia-<run>-<job>.d`) that is
  journaled and removed on success, failure and cancel.
- The CPU tier is deterministic for identical inputs, options and engine on one machine.
- The AI tier is never chosen automatically (it is suggested for sources under 720p); forcing it
  on a clip predicted at more than 10 times its length warns "too slow". The worker loads only
  the six pinned model files and checks the bytes it loads.
- rmedia copies no frame bytes on the AI tier: the three processes are joined by OS pipes, and a
  cancel stops all three within 2 seconds.

The 001 refactors A1 to A5 are in place: outcomes carry a `JobMeasure` (`clip` or `upscale`), jobs
can run several stages with weighted progress, and issue codes are a typed enum
(`IssueCode`).

## GIF Export (feature 003)

**Responsibility:** make an animated GIF or a silent "Reddit MP4" loop that fits a target's size
budget: from `rmedia gif`, `clip --gif`, `batch --gif` and manifest `gif` blocks, and from the
desktop GIF view. Code is in `crates/rmedia-core/src/gif/` and `exec/gif.rs`. Added in 0.3.0
(unreleased); design in `specs/003-gif-export/` (research R1–R16).

| Item | Location |
|---|---|
| `GifOptions` / `PartialGif` (target, format, engine, shape, framing, width, fps, colours, dither, lossy, loop, budget, allow_trim), `parse_size` | `gif/mod.rs` |
| GIF targets as data: `GifTargetRegistry` (four built-ins in `gif/builtin/*.json` plus `<config>/gif-targets/*.json`, schema 1), `Budget`, `Dither`, `GifskiParams`, `Mp4Params` | `gif/target.rs` |
| Frame timing: centisecond delays, the 50 fps cap, exact rates {10, 20, 25, 50}, the `TIMING_APPROXIMATE` note | `gif/timing.rs` |
| Planning: `plan_gif` builds `GifSpec { cut, frame: FramePlan, gif: GifPlan }`; never enlarges; engine choice and fallback; `FramePlan.smart` for follow-faces framing | `gif/plan.rs` |
| Graphs and argv: two-pass palette (`palettegen` to `p-<key>.png`, then `paletteuse` from a second input), gifski's y4m pipe, the MP4 (`libx264`, `+faststart`, no audio, square pixels), gifsicle `-O2 [--lossy]` | `gif/graph.rs` |
| Size estimate: three 1 s windows in one ffmpeg run, the size model, the MP4 hint, `prepare` (dry runs and the desktop estimate) | `gif/estimate.rs` |
| Budget search: a pure, deterministic ladder behind the `Sampler` trait (lossy → dither → colours → fps → width; CRF for the MP4; gifski `lossy_quality`), at most 9 encodes, `BUDGET_UNREACHABLE` with the seconds that fit, `--allow-trim` | `gif/budget.rs` |
| Optional engines: gifsicle and gifski lookup (flag/setting → env → bundled → `PATH`), `--version` banner check, refused paths | `gif/engines.rs` |
| Verify: every delay ≥ 2 cs, frame count, width, loop block (NETSCAPE), size under the limit; the MP4's codec, tags and fast start | `gif/verify.rs` |
| Executor: analyze (smart), search, palette, encode, optimise, verify; the gifski pipe runs through `exec/pipeline.rs` (ffmpeg → gifski) | `exec/gif.rs` |
| Manifests: JSON `gif` block, `gif_*` CSV columns; `clip --gif` / `batch --gif` | `input/manifest_json.rs`, `input/manifest_csv.rs`, `crates/rmedia-cli/src/commands/` |
| Desktop: `list_gif_targets`, `gif_estimate` / `cancel_estimate`, `open_output`, `pick_engine` with `kind` `gifsicle`/`gifski` (**Settings › GIF export**) | `apps/desktop/src-tauri/src/ops.rs` |

**Invariants**

- A GIF is never enlarged and never plays faster than 50 fps (no delay under 2 cs).
- Nothing is written above the hard limit: the verify stage checks the committed size, and a
  watchdog aborts an encode whose temp file passes 2× the limit.
- gifsicle and gifski are separate processes, never linked, found only outside input and scratch
  folders, and given only generated names inside scratch (never a source name).
- Without either engine every requirement still holds: gifski falls back to the built-in engine
  (`ENGINE_FALLBACK`, or `ENGINE_MISSING` with `--strict-engine`), and no post-pass runs.
- The search is deterministic for identical inputs, options and engines, and never runs more
  than 9 encodes; a retried full encode is kept only when it is smaller.
- Smart framing reuses 002's analysis and its cache; the crop window found for the start size is
  kept at every width the search tries.
- `clip --gif --upscale` is one job with no intermediate file; the upscale step is skipped
  (a GIF is never enlarged) and the plan says so.

## Tooling (`xtask`)

| Command | Effect |
|---|---|
| `cargo xtask fetch-tools [--target TRIPLE]` | Downloads the pinned BtbN build from `xtask/tools.lock.json`, verifies its SHA-256, and installs `binaries/rmedia-ffmpeg-<triple>` / `rmedia-ffprobe-<triple>`. A stamp file records the archive and binary hashes. |
| `cargo xtask pin-tools` | Pins the newest BtbN autobuild using GitHub asset digests |
| `cargo xtask gen-bindings` | Runs `cargo test -p rmedia-core --features ts` to export TypeScript into `apps/desktop/src/lib/bindings` |
| `cargo xtask stage-cli [--target TRIPLE]` | Builds the release CLI and the AI worker and copies them to `binaries/rmedia-<triple>[.exe]` and `binaries/rmedia-sr-<triple>[.exe]` (Tauri sidecar naming) |
| `cargo xtask fetch-ncnn [--target TRIPLE]` | Installs the pinned prebuilt ncnn into `third_party/ncnn/<triple>/` for the worker's build |
| `cargo xtask archive` / `mirror <tag>` / `check-pins` | Keep every pinned archive in `archives/`, upload missing ones to a mirror release and record the URLs, and check (in CI) that every pin is mirrored and the FFmpeg source release and notice are in place |
| `cargo xtask notices` | Writes `THIRD_PARTY_NOTICES.md` from cargo and npm metadata, including license texts |
| `cargo xtask fetch-tools` (GIF engines) | Also stages `binaries/rmedia-gifsicle-<triple>` and `rmedia-gifski-<triple>` from `xtask/gif-tools.lock.json`, verifying SHA-256s (`--no-gifski` leaves gifski out) |
| `cargo xtask pin-gif-tools <run-id>` | Pins the binaries and sources of a `gif-tools.yml` run into `xtask/gif-tools.lock.json` |
| `cargo xtask check-release-gate [--tree DIR]` | Fails when a release would bundle gifski (AGPL-3.0, `binaries/rmedia-gifski*` staged) unless the repository has a root `LICENSE` and an allowed `app_licence` in `[workspace.metadata.rmedia]` of the root `Cargo.toml` (an AGPL-3.0 or GPL-3.0 id, or `commercial-gifski-imagequant`; FR-025) |
| `cargo xtask fetch-models` | Downloads the models pinned in `xtask/models.lock.json` into `models/`, verifying SHA-256; verified files are kept. Models are never committed. |

## Testing

| Layer | Where | Notes |
|---|---|---|
| Unit (table-driven parsers, naming, commit, journal, progress, version parsing) | `#[cfg(test)]` in core modules | 54 core unit tests (validation.md) |
| Core integration | `crates/rmedia-core/tests/` (`us1_clip`, `us2_cliplist`, `us3_batch`, `us4_discover`, `preview`, `cancel`; for 002: `us1_upscale`, `us1_upscale_plan`, `quality`, `probe_002`, `engine_checks`, `scratch`) | Real media from `rmedia-testkit`; accuracy checked via ffprobe durations and first-frame PSNR |
| argv snapshots | `tests/snapshots_ffargs.rs` (insta) | 8 snapshots |
| Benches | `tests/bench.rs` (`--ignored bench`) | SC-001, SC-003 |
| CLI contract | `crates/rmedia-cli/tests/` (`clip_single`, `clip_list`, `scan`, `batch`, `contract_clip_json`, `upscale`) | Includes SIGINT / SIGKILL |
| Desktop backend | `apps/desktop/src-tauri/tests/{bridge,media_server}.rs` | Path policy, CLI/desktop app byte parity (SC-008), media server security |
| Frontend | `apps/desktop/src/lib/stores/*.test.ts` (vitest), `svelte-check` | 8 tests |

`rmedia-testkit` generates fixtures (testsrc2/sine, GOP 50, VFR, multi-audio + SRT, WebM,
no-audio, MPEG-TS with offset, cover-art m4a, fake and mislabelled files) into
`target/rmedia-fixtures/<hash>/`. No binary fixtures are committed (Principle III). Committed
ffprobe JSON samples live in `crates/rmedia-core/tests/data/probe/`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy (`-D warnings`) and tests against the bundled
engine on `ubuntu-22.04` and `windows-latest`; tests against distro FFmpeg 6.1 on `ubuntu-24.04`
(the minimum engine); a Windows type-check from Linux; and the desktop job (bindings drift,
frontend checks, backend tests on Linux).

## Related Documentation

- [Architecture Overview](overview.md)
- [Security Model](../security/security-model.md)
- [Glossary](../reference/glossary.md)
