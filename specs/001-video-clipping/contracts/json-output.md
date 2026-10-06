# JSON Output Contract (`--json`)

Every top-level document has `"schema": 1`. Times are seconds (number, µs precision). Paths are
strings (lossy UTF-8 on Linux for non-UTF-8 names, flagged `"path_lossy": true`).

## `scan --json` (single document)
```json
{"schema":1,"type":"scan","roots":["/v"],
 "videos":[{"path":"/v/a.mp4","size":1048576,"duration":60.0,"width":1920,"height":1080,
            "fps":29.97,"video_codec":"h264","container":"mp4","audio_streams":1,"subtitle_streams":0}],
 "rejected":[{"path":"/v/fake.mp4","reason":"unreadable"}],
 "skipped":{"hidden":3,"temp":1,"symlink":0,"excluded":0},"elapsed_ms":412}
```

## `doctor --json`
```json
{"schema":1,"type":"doctor","ffmpeg":{"path":"...","source":"bundled","version":"7.1"},
 "ffprobe":{"path":"...","source":"bundled","version":"7.1"},
 "encoders":{"libx264":true,"aac":true,"libvpx-vp9":true,"libopus":true},
 "accurate_containers":["mp4","mov","m4v","mkv","ts","webm"],"ok":true,"problems":[]}
```

## `--dry-run --json` (single document)
```json
{"schema":1,"type":"plan",
 "jobs":[{"id":1,"source":"/v/a.mp4","output":"/v/clips/a_001_00-01-30.000_00-02-00.000.mp4","status":"planned",
          "mode":"fast","requested":{"start":90.0,"end":120.0},"predicted":{"start":88.0,"end":120.0},
          "warnings":[],"argv":["-hide_banner","..."]}],
 "issues":[{"severity":"error","file":"clips.txt","line":2,"message":"invalid time \"1:7x\""}],
 "summary":{"planned":1,"skipped":0,"errors":1}}
```

## `clip|batch --json` (NDJSON stream; one event per line)
```
{"schema":1,"type":"run_started","run_id":"…","total_jobs":2,"total_media":40.0}
{"schema":1,"type":"job_started","job_id":1}
{"schema":1,"type":"job_progress","job_id":1,"fraction":0.42,"out_time":8.4,"speed":31.5,"eta_ms":380}
{"schema":1,"type":"job_finished","job_id":1,"outcome":{"status":"succeeded","output":"…","actual":{"start":88.0,"end":120.0},"adjusted":true}}
{"schema":1,"type":"run_progress","done":1,"total":2,"fraction":0.5,"eta_ms":900}
{"schema":1,"type":"summary","succeeded":2,"skipped":0,"failed":0,"cancelled":0,"exit_code":0,"jobs":[…]}
```
The final line is always `type: "summary"` (also on cancel). Failed outcome:
`{"status":"failed","error":{"code":"ENGINE_FAILED","message":"…","stderr_tail":"…"}}`.

Scan `rejected[].reason`: `unreadable` (ffprobe failed), `no_video_stream` (audio / cover art
only), `image` (still image with a video extension), `inaccessible` (permission error, symlink
loop).

Error codes (stable): `INVALID_INPUT`, `SOURCE_MISSING`, `PROBE_FAILED`, `OUTPUT_EXISTS`,
`OUTPUT_UNWRITABLE`, `DISK_FULL`, `ENGINE_FAILED`, `VERIFY_FAILED`, `CANCELLED`.
