# rmedia JSON Output Reference

This page documents the machine-readable output of `rmedia` with `--json` (FR-024, Principle II).
The shapes below were verified against the real binary (0.1.0, commit `bd1eef8`, bundled FFmpeg
n9.0.2) on generated test media. Long paths are shortened to `/work/...`, and `argv` arrays are
abbreviated where marked.

Code: `crates/rmedia-cli/src/render.rs`, `crates/rmedia-cli/src/commands/scan.rs`,
`crates/rmedia-core/src/{event.rs,error.rs,doctor.rs}`. Contract:
`specs/001-video-clipping/contracts/json-output.md`.

## Conventions

- Every top-level document has `"schema": 1` and a `"type"`.
- Times are seconds as JSON numbers with microsecond precision.
- Paths are strings. On Linux, bytes that are not valid UTF-8 are replaced with U+FFFD, so JSON
  output never fails on such names, but those names cannot be round-tripped.
- Key order is not significant. The CLI emits keys in alphabetical order, except the
  pretty-printed `doctor` document, which keeps field order (examples on this page are reordered
  for reading).
- With `--json`, stdout carries only JSON. Human-readable `error:` / `warning:` lines still go to
  stderr.

| Command | stdout |
|---|---|
| `doctor --json` | One pretty-printed document, `type: "doctor"` |
| `scan --json` | One document, `type: "scan"` |
| `clip`/`batch --dry-run --json` | One document, `type: "plan"` |
| `clip`/`batch --json` (run) | NDJSON: one event per line, ending with `type: "summary"` |
| `clip`, `batch` or `scan` failing before planning (engine missing, bad scan root, unreadable input) | One document, `type: "error"` (or `type: "plan"` with `issues` for clip list / manifest parse errors) |

## `doctor`

```json
{
  "schema": 1,
  "type": "doctor",
  "ffmpeg": { "path": "/work/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu", "source": "config" },
  "ffprobe": { "path": "/work/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu", "source": "config" },
  "version": "n9.0.2-22-g46d8f462ee-20261005",
  "encoders": { "aac": true, "libopus": true, "libvpx-vp9": true, "libx264": true },
  "accurate_containers": ["mp4", "m4v", "mov", "mkv", "ts", "webm"],
  "ok": true,
  "problems": []
}
```

| Field | Type | Notes |
|---|---|---|
| `ffmpeg`, `ffprobe` | `{path, source}` or `null` | `source`: `config` (flag, settings or `RMEDIA_*` env var), `bundled`, `path` |
| `version` | string or `null` | First token after `ffmpeg version` |
| `encoders` | object | These four keys: `libx264`, `aac`, `libvpx-vp9`, `libopus`. Empty (`{}`) when the engine was not found. |
| `accurate_containers` | string[] | Containers whose video and audio encoders both exist |
| `ok` | boolean | `false` when the engine was not found or its encoders could not be read (exit 4). `doctor --json` reports a missing engine in this document (`ffmpeg`/`ffprobe`/`version` `null`), not as a `type: "error"` document. |
| `problems` | string[] | Human-readable problems |

## `scan`

Real output (two videos and one fake `.mp4`):

```json
{"schema":1,"type":"scan",
 "roots":["/work/v"],
 "videos":[
  {"path":"/work/v/a.mp4","size":3230114,"duration":30.0,"width":640,"height":360,"fps":25.0,
   "video_codec":"h264","container":"mp4","audio_streams":1,"subtitle_streams":0},
  {"path":"/work/v/sub/b.mkv","size":735368,"duration":20.0,"width":320,"height":240,"fps":25.0,
   "video_codec":"h264","container":"mkv","audio_streams":0,"subtitle_streams":0}],
 "rejected":[{"path":"/work/v/fake.mp4","reason":"unreadable"}],
 "skipped":{"hidden":0,"temp":0,"symlink":0,"excluded":0,"cloud":0},
 "elapsed_ms":31}
```

| Field | Notes |
|---|---|
| `videos[].duration`, `width`, `height`, `fps`, `video_codec` | `null` when unknown. `fps` is rounded to 3 decimals. |
| `videos[].container` | Lower-case file extension, not the detected format |
| `rejected[].reason` | `unreadable` (ffprobe failed), `no_video_stream` (audio or cover art only), `image` (still image with a video extension), `inaccessible` (permission error, symlink loop, or replaced by a symlink during the scan) |
| `skipped.cloud` | Online-only cloud files (Windows); always 0 elsewhere |

The CLI document omits the rejection `detail` text; use `-v` without `--json` to see it.

## Plan (`--dry-run --json`, and plan errors)

Real dry run (one fast clip, `argv` shown in full):

```json
{"schema":1,"type":"plan","run_id":"0dde93ad7775daac",
 "jobs":[{"id":1,"source":"/work/v/a.mp4",
   "output":"/work/v/clips/a_001_00-00-10.500_00-00-15.500.mp4",
   "status":"planned","mode":"fast",
   "requested":{"start":10.5,"end":15.5},"predicted":{"start":10.0,"end":15.5},
   "warnings":[],
   "argv":["-hide_banner","-nostdin","-n","-loglevel","warning","-progress","pipe:1","-nostats",
           "-stats_period","0.25","-ss","10.500000","-protocol_whitelist","file,pipe",
           "-i","file:/work/v/a.mp4","-t","5.000000","-map","0:0","-map","0:1",
           "-map_metadata","0","-map_chapters","-1","-c","copy","-avoid_negative_ts","make_zero",
           "-movflags","+faststart","-f","mp4",
           "file:/work/v/clips/.a_001_00-00-10.500_00-00-15.500.rmedia-0dde93ad7775daac-1.part"]}],
 "issues":[],
 "summary":{"planned":1,"skipped":0,"errors":0}}
```

### `jobs[]`

| Field | Type | Notes |
|---|---|---|
| `id` | integer | 1-based, stable within the plan |
| `source`, `output` | string | Absolute paths |
| `status` | `"planned"` \| `"skip"` | `skip` adds `skip_reason` (for example `"output already exists"`) |
| `mode` | `"fast"` \| `"accurate"` | |
| `requested` | `{start, end}` | `end` is resolved: an end past the video, or `end`, becomes the duration |
| `predicted` | `{start, end}` | Fast mode only: the keyframe the clip will start on |
| `warnings` | string[] | For example `end 00:00:40.000 is past the end of the video; clip ends at 00:00:30.000`, `accurate mode cannot write .avi; writing .mp4 instead (choose --container to override)` |
| `argv` | string[] | The exact ffmpeg arguments, excluding the program; the last item is the temp file |

### `issues[]` and Errors

When any issue has severity `error`, nothing runs (exit 3, or 4 when every error is
`ENGINE_MISSING`). The plan document goes to stdout and each issue also goes to stderr.

```json
{"schema":1,"type":"plan","run_id":"","jobs":[],
 "issues":[
  {"severity":"error","code":"INVALID_INPUT","file":"/work/bad.txt","line":3,
   "message":"missing end after \"0:05\" (use END, +DURATION or end)"},
  {"severity":"error","code":"INVALID_INPUT","file":"/work/bad.txt","line":4,
   "message":"invalid time \"1:7x\": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s"}],
 "summary":{"planned":0,"skipped":0,"errors":2}}
```

| Field | Notes |
|---|---|
| `severity` | `error` \| `warning` |
| `code` | Error code (see [Error codes](#error-codes)) |
| `file` + `line` | Clip lists, CSV manifests, JSON syntax errors |
| `file` + `json_path` | JSON manifest semantic errors, for example `videos[0].clips[1]` |
| (no location) | Requests from the command line, for example `end (00:00:10.000) is not after start (00:00:20.000)` or `source not found: /work/v/nope.mp4` (`SOURCE_MISSING`) |

`run_id` is `""` when parsing failed before planning.

### `type: "error"`

Used for failures that are neither parse nor plan issues. Real example:

```json
{"schema":1,"type":"error","code":"ENGINE_MISSING","message":"/nonexistent does not exist"}
```

Codes used here: `ENGINE_MISSING` (exit 4) and `INVALID_INPUT` (exit 3).

## Run Events (NDJSON)

`clip`/`batch --json` without `--dry-run` streams one JSON object per line. Real run:

```text
{"schema":1,"type":"run_started","run_id":"d0ff2aee7763899e","total_jobs":1,"total_media":5.0}
{"schema":1,"type":"job_started","job_id":1}
{"schema":1,"type":"job_progress","job_id":1,"fraction":1.0,"out_time":5.08059,"speed":2390.0,"eta_ms":0}
{"schema":1,"type":"run_progress","done":0,"total":1,"fraction":1.0}
{"schema":1,"type":"job_finished","job_id":1,"outcome":{"status":"succeeded","output":"/work/v/clips/a_001_00-00-10.500_00-00-15.500.mp4","actual":{"start":10.0,"end":15.6},"adjusted":true}}
{"schema":1,"type":"run_progress","done":1,"total":1,"fraction":1.0}
{"schema":1,"type":"summary","succeeded":1,"skipped":0,"failed":0,"cancelled":0,"exit_code":0,"jobs":[{"id":1,"source":"/work/v/a.mp4","output":"/work/v/clips/a_001_00-00-10.500_00-00-15.500.mp4","requested":{"start":10.5,"end":15.5},"status":"succeeded","actual":{"start":10.0,"end":15.6},"adjusted":true}]}
```

| `type` | Fields | Notes |
|---|---|---|
| `run_started` | `run_id`, `total_jobs`, `total_media` | `total_media`: seconds of media to produce (jobs that will run) |
| `job_started` | `job_id` | |
| `job_progress` | `job_id`, `fraction` (0–1), `out_time`, `speed`?, `eta_ms`? | At most about 10 per second per job; the final block is always sent |
| `job_warning` | `job_id`, `message` | One per plan warning, at job start |
| `job_finished` | `job_id`, `outcome` | See below. Skipped jobs emit only this event. |
| `run_progress` | `done`, `total`, `fraction`, `eta_ms`? | Weighted by media length. Throttled to about 4 per second, plus one after each finished job. `eta_ms` only when 2% < fraction < 100%. |
| `summary` | `succeeded`, `skipped`, `failed`, `cancelled`, `exit_code`, `jobs[]` | **Always the last line**, also on cancel. This is the core's `run_finished` event, renamed by the CLI. |

### `outcome` (in `job_finished`, flattened into `summary.jobs[]`)

| `status` | Extra fields |
|---|---|
| `succeeded` | `output` (path actually written; may differ under `rename`), `actual` `{start,end}`, `adjusted` (true when start or end differ from the request by more than 50 ms) |
| `skipped` | `reason`: `output already exists`, or `output appeared while the clip was being made` |
| `failed` | `error` `{code, message, stderr_tail?}` |
| `cancelled` | — |

`summary.jobs[]` items have `id`, `source`, `output`, `requested`, plus the outcome fields.

In fast mode `actual.start` is the predicted keyframe and `actual.end` = start + measured output
duration. In the example above, the request was 10.5–15.5 and the clip was produced as
10.0–15.6.

## Error Codes

Stable codes from `Error::code()` (`crates/rmedia-core/src/error.rs`) plus `INTERNAL`
(`exec/mod.rs`):

| Code | Where it appears | Meaning |
|---|---|---|
| `INVALID_INPUT` | Issues, `error` documents | Malformed time, clip list, manifest, template or container; end not after start; output would overwrite a source; output collision; no keyframe found |
| `SOURCE_MISSING` | Issues | Source file does not exist |
| `PROBE_FAILED` | Issues | ffprobe could not read the source, or it has no video stream |
| `OUTPUT_EXISTS` | Internal | Surfaced as a `skipped` outcome, not as a failure |
| `OUTPUT_UNWRITABLE` | `failed` outcome | Output folder cannot be created, permission denied, read-only file system, fsync/commit failure |
| `DISK_FULL` | `failed` outcome | ffmpeg reported no space left |
| `ENGINE_FAILED` | `failed` outcome | ffmpeg exited with an error. The message is the most specific stderr line; `stderr_tail` holds up to 50 lines. |
| `ENGINE_MISSING` | Issues, `error` documents | Engine not found, too old, or encoder missing for accurate mode |
| `VERIFY_FAILED` | `failed` outcome | Output unreadable, wrong stream count, no duration, or accurate video length outside tolerance |
| `CANCELLED` | Internal | Surfaced as a `cancelled` outcome |
| `IO_ERROR` | `failed` outcome | Other I/O errors |
| `INTERNAL` | `failed` outcome | A job task crashed; reported, never silently dropped |

**Unverified:** the `failed`, `cancelled` and `INTERNAL` outcomes above are documented from code
and from the contract tests (`crates/rmedia-cli/tests/`, `crates/rmedia-core/tests/cancel.rs`,
`tests/us3_batch.rs`). They were not reproduced by hand for this page.
