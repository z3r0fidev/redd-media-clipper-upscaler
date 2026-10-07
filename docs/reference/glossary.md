# rmedia Glossary

These are the project terms used across the code, specs and documentation. Each entry names
where the term comes from. Use these terms consistently; the "Do not use" column lists synonyms
to avoid.

| Term | Definition | Source | Do not use |
|---|---|---|---|
| **accurate mode** | Clipping mode that re-encodes (libx264/AAC, or libvpx-vp9/libopus for WebM) and is frame-exact. The output length is verified within 3 frames + 100 ms. CLI: `--mode accurate`. | `request::Mode::Accurate`, research.md R3/R4 | precise mode, re-encode mode |
| **actual span** | The start/end a finished clip really covers: the measured output duration, starting at the predicted keyframe in fast mode. Reported as `actual`. | `JobOutcome::Succeeded.actual` | real times |
| **adjusted** | Flag set when the actual span differs from the requested span by more than 50 ms (FR-014) | `exec::ADJUST_TOLERANCE` | |
| **batch / batch manifest** | See *manifest*. `rmedia batch` runs one. | `commands/batch.rs` | job file |
| **BatchRequest** | The single normalized input form (defaults + sources × clips) that every surface builds before planning | `request.rs` | |
| **bundled engine** | The pinned FFmpeg build shipped as `rmedia-ffmpeg` / `rmedia-ffprobe` beside the executables; `doctor` reports source `bundled` | `tools.rs`, `xtask/tools.lock.json` | embedded ffmpeg |
| **clip** | One output file cut from one source between a start and an end | spec.md | segment (except in the planned `Segment` type), cut |
| **clip list** | A plain-text file with one clip per line (`START END|+DURATION [NAME]`), applied to one or more sources | `input/cliplist.rs` | cut list, EDL |
| **commit** | Moving a verified temp file to its final name according to the on-exists policy, atomically and without clobbering | `exec/commit.rs` | save, finalize |
| **container** | Output file format chosen by extension (`mp4`, `mkv`, `webm`, …). Defaults to the source's. | `plan/streams.rs::Container` | format (ambiguous) |
| **disposition** | Planned fate of a job: `run` or `skip{reason}` | `plan::Disposition` | |
| **doctor** | Engine diagnostics: location, version, encoders, accurate-mode containers. CLI `rmedia doctor`; app **Settings › Video engine**. | `doctor.rs` | |
| **dry run** | Planning without writing anything (`--dry-run`) | FR-019 | preview (reserved for the player) |
| **engine** | ffmpeg + ffprobe as used by rmedia. User guides call it the **video engine**, matching **Settings › Video engine**. | `tools::Tools` | backend |
| **fast mode** | Default clipping mode: stream copy (`-c copy`), no re-encoding. Starts on the keyframe at or before the requested start and keeps the requested end. | `request::Mode::Fast`, research.md R3 | quick mode, lossless mode |
| **indexed container / indexed seeking** | Formats whose demuxer seek is exact (ffprobe `format_name` `mov`, `mp4`, `matroska`, `webm`). Decides the seek strategy. | `MediaInfo::has_indexed_seeking` | |
| **issue** | A planning or parsing problem (`severity` error/warning, `code`, `origin`, `message`). Any error-severity issue means nothing runs. | `plan::Issue` | |
| **job** | One clip request resolved to a concrete output path, with state queued/running/succeeded/failed/cancelled/skipped | `plan::PlannedJob`, spec.md | task |
| **job kind** | The work a job does: `JobSpec::Clip` today; `Upscale` (002) and `Gif` (003) are planned | `plan::JobSpec` | |
| **journal (in-flight journal, crash journal)** | A per-run file under `<state dir>/inflight/` listing temp files, locked while the run lives. The next run deletes temp files of dead runs. | `exec/journal.rs` | lock file |
| **keyframe snap** | Fast mode starting at the last keyframe K ≤ requested start, found by scanning packets with ffprobe | `probe::keyframe_at_or_before` | |
| **manifest** | A batch file listing videos and their clips with layered options, in JSON (versioned, strict) or CSV (one row per clip). The desktop queue is a v1 manifest. | `input/manifest_json.rs`, `input/manifest_csv.rs` | job list |
| **media server** | The desktop app's loopback HTTP range server that feeds `<video>` | `media_server.rs` | asset server |
| **name template** | Output naming pattern with placeholders such as `{source}`, `{index:03}` and `{name-or-range}`. User guides call it the **file name pattern** (**File names** in Settings). | `plan/naming.rs` | |
| **on-exists policy** | What to do when an output exists: `skip` (default), `overwrite`, `rename` | `request::OnExists` | collision mode |
| **origin** | Where a request came from, for error messages: command line, queue, file:line, or file:JSON path | `request::Origin` | |
| **`.part` file / temp output** | `.<stem>.rmedia-<run_id>-<job>.part`, the hidden file ffmpeg writes before commit | `exec/commit.rs::temp_path` | partial |
| **plan** | The validated list of jobs (plus issues) for a request; nothing has been written yet | `plan::Plan` | |
| **predicted span** | Fast mode's expected start (the keyframe) and end, shown in dry runs | `PlannedJob.predicted` | |
| **probe** | Running ffprobe on a file to build `MediaInfo` | `probe.rs` | inspect |
| **proxy (preview copy)** | A cached 480p H.264/AAC MP4 or VP8/Opus WebM made only so the webview can play a source. Timestamps match the source; clips are never cut from it. | `preview::ensure_proxy` | transcode |
| **requested span** | The start/end the user asked for, with `end` resolved (clamped to the video's duration) | `PlannedJob.requested` | |
| **run** | One execution of a plan, with a concurrency limit, events and a final summary | spec.md, `exec::start` | batch run |
| **run id** | 64-bit random hex identifying a plan/run; part of temp names and the journal name | `plan::new_run_id` | |
| **scan** | Folder discovery: walk, filter by extension, confirm with ffprobe. `rmedia scan`. | `discover.rs` | crawl |
| **sidecar** | A program bundled next to the app via Tauri `externalBin` (`rmedia-ffmpeg`, `rmedia-ffprobe`, `rmedia`), staged in `binaries/` with target-triple suffixes | `tauri.conf.json`, `xtask stage-cli` | |
| **source (source video)** | An input video. Never modified (Principle V). | spec.md | input file (fine in CLI context) |
| **still frames** | The last preview fallback: exact JPEG frames from `frame_at` instead of playback | `preview::frame_at` | |
| **summary** | End-of-run totals (`succeeded`, `skipped`, `failed`, `cancelled`, `exit_code`, `jobs`); the final NDJSON line `type: "summary"` | `event::RunSummary` | report |
| **surface** | A user-facing front end over the core: the CLI or the desktop app | constitution Principle I | client |
| **time offset** | Seconds to subtract from a player's `currentTime` to get clip time (first video frame − container start) | `preview::time_offset` | |
| **zero point** | The absolute timestamp that user time 0 refers to: the first video frame (falls back to the container start) | `MediaInfo::zero_point` | |

## Acronyms

| Acronym | Expansion |
|---|---|
| CSP | Content Security Policy |
| GOP | Group of Pictures (the frames between keyframes) |
| IPC | Inter-Process Communication (here: Tauri `invoke` and channels) |
| NDJSON | Newline-Delimited JSON |
| PDEATHSIG | Linux "parent death signal" (`prctl(PR_SET_PDEATHSIG)`) |
| PSNR | Peak Signal-to-Noise Ratio (used to compare first frames in tests) |
| SC / FR | Success Criterion / Functional Requirement (spec.md) |
| VFR | Variable Frame Rate |
