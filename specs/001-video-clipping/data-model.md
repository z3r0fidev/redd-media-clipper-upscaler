# Data Model: Video Clipping Base

All types live in `rmedia-core`, derive `serde` (and `ts-rs` for GUI-visible ones). Times are
`Ts` = signed 64-bit microseconds; serialized as seconds (f64, µs precision) in JSON.

## Ts (time value)
| Field | Type | Rules |
|---|---|---|
| µs | i64 | ≥ 0 for positions |

Parsing (FR-002): `90`, `90.5`, `1:30`, `01:30.5`, `00:01:30.500`, `00:01:30,500`, `1h2m3.5s`,
`500ms`, `end`; `+<any of the above>` = duration. Rejects negatives, minutes/seconds ≥ 60 in
non-leading fields, > 6 fractional digits, exponents. Leading field may exceed 59 (`90:00`).

## EndSpec
`At(Ts)` | `Duration(Ts)` | `EndOfMedia`

## Options (layered: global defaults < source overrides < clip overrides)
| Field | Type | Default |
|---|---|---|
| mode | `Fast` \| `Accurate` | Fast |
| output_dir | path? | `<source dir>/clips` |
| name_template | string | `{source}_{index:03}_{name-or-range}.{ext}` |
| container | ext? | source container (accurate-mode fallback → mp4) |
| on_exists | `Skip` \| `Overwrite` \| `Rename` | Skip |
| streams | StreamSelection | audio All, subtitles All |

`StreamSelection { audio: All | None | Indices[u32], subtitles: All | None }`

## ClipRequest
| Field | Type | Rules |
|---|---|---|
| start | Ts | < resolved end; < media duration |
| end | EndSpec | resolved end > start; clamped to duration with warning |
| name | string? | sanitized for filenames |
| overrides | PartialOptions | |
| origin | Origin | file + line, or file + JSON path, or `cli`/`gui` |

## SourceRequest
`{ path, clips: [ClipRequest], overrides: PartialOptions, origin }` — path resolved relative to
the defining file's directory.

## BatchRequest (single normalized input form)
`{ defaults: Options, sources: [SourceRequest] }` — produced by CLI flags, clip lists (× N sources),
JSON/CSV manifests, and the GUI queue. Export = serialize to manifest JSON/CSV.

## MediaInfo (probe result)
`{ path, size, mtime, container, format_name, duration, start_time, streams: [StreamInfo] }`;
`StreamInfo { index, kind: Video|Audio|Subtitle|Data|Attachment, codec, width?, height?,
avg_frame_rate?, is_attached_pic, language?, title? }`. A file is a **video** iff ≥ 1 Video stream
with `is_attached_pic == false`.

## ScanResult
`{ roots, videos: [MediaInfo], rejected: [{path, reason}], skipped: {hidden, temp, symlink,
excluded}, elapsed_ms }`; reasons: `unreadable | no_video_stream | probe_failed`.

## Plan / PlannedJob
| Field | Type | Notes |
|---|---|---|
| id | JobId (u32) | stable within plan |
| source | path | |
| output | path | never equals any source (canonical, case-insensitive on Windows) |
| disposition | `Run` \| `Skip(reason)` | Skip when exists & policy Skip |
| spec | JobSpec | `Clip(ClipSpec)`; future `Upscale`, `Gif` |
| requested | (Ts, Ts) | |
| predicted | (Ts, Ts)? | fast mode keyframe snap |
| warnings | [string] | dropped streams, clamped end, container fallback |
| argv | [string] | exact ffmpeg arguments (shown in dry-run) |

`Plan { run_id, jobs, issues: [Issue{severity: Error|Warning, code, origin, message}], cancelled }` — any Error issue ⇒
nothing runs (exit 3).

`ClipSpec { segment: Segment{input, start, duration, map: [stream index]}, mode, container,
encode: EncodeProfile? }`

## Job lifecycle (state machine)
```
Planned ──► Skipped (exists / policy)            [terminal]
Planned ──► Queued ──► Running ──► Verifying ──► Succeeded   [terminal]
                         │             │
                         ├──► Failed ◄─┘                      [terminal]
                         └──► Cancelled                       [terminal]
Queued ──► Cancelled
```
On Failed/Cancelled: process tree killed, `.part` removed, journal entry cleared.

## Event (core → CLI renderer / GUI Channel)
`RunStarted{run_id,total_jobs,total_media}`, `JobStarted{job_id}`,
`JobProgress{job_id,fraction,out_time,speed?,eta_ms?}` (≤ 10 Hz/job), `JobWarning{job_id,
message}`, `JobFinished{job_id,outcome}`, `RunProgress{done,total,fraction,eta_ms?}`,
`RunFinished{summary}`.

`JobOutcome = Succeeded{output, actual:{start,end}, adjusted} | Skipped{reason} |
Failed{error:{code,message,stderr_tail}} | Cancelled`

`RunSummary { succeeded, skipped, failed, cancelled, jobs: [..], exit_code }`

`ScanEvent = Progress{visited,candidates,probed} | Found{video: MediaInfo} |
Rejected{rejected: {path, reason, detail?}}` — the call returns the `ScanResult` (no `Done` event).
`ScanResult.skipped` counts `hidden, temp, symlink, excluded, cloud`; `excluded_named` lists folders
skipped by name.

## Settings (`settings.json`)
`{ ffmpeg_path?, ffprobe_path?, defaults: PartialOptions, jobs?, proxy_cache_max_mb: 5120 }`

## Journal (`state_dir/inflight/<run_id>-<pid>.json`)
A JSON array of temp paths, held under an OS file lock by the running process. At startup, journals
whose lock can be taken (owner gone) are purged: each listed `.part` is deleted only if it matches
the rmedia temp-name pattern, then the journal file is removed.
