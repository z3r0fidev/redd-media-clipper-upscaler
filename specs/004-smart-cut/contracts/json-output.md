# JSON output additions: Smart Cut

Extends the 001, 002 and 003 JSON contracts. `"schema": 1` stays, and every change is additive:
the new fields are absent unless a job requested smart mode.

## `--dry-run --json`: a smart clip job

```json
{"id":1,"kind":"clip","source":"/v/match.mp4","output":"/v/clips/match_00-03.320_00-20.720.mp4",
 "status":"planned","mode":"smart","requested":{"start":3.32,"end":20.72},
 "warnings":[],"notices":[],
 "argv":["…join…"],"pipeline":[["…head…"],["…copy…"],["…tail…"],["…audio…"],["…join…"]],
 "smart":{"requested":true,"mode":"smart","strict":false,
   "head":{"span":{"start":3.32,"end":4.0},"frames":17},
   "copy":{"span":{"start":4.0,"end":20.0},"frames":400},
   "tail":{"span":{"start":20.0,"end":20.72},"frames":18},
   "copied_share":0.9195,"fallback":null}}
```

A fallback known while planning:

```json
{"id":2,"kind":"clip","mode":"accurate","…":"…",
 "warnings":["smart cut not possible: the video has open GOPs; cutting in accurate mode"],
 "notices":[{"code":"SMART_FALLBACK","message":"smart cut not possible: the video has open GOPs; cutting in accurate mode","detail":{"reason":"open_gop"}}],
 "smart":{"requested":true,"mode":"accurate","strict":false,"head":null,"copy":null,"tail":null,
   "copied_share":null,"fallback":"open_gop"}}
```

- Under strict mode the job is listed in `issues` with code `SMART_UNAVAILABLE` and
  `"detail":{"reason":"open_gop"}`.
- The plan document gains
  `"smart":{"smart":3,"fallback":{"open_gop":1,"vfr":1},"not_applied":0}` when any job requested
  smart.
- `fallback` values: `encoder_missing`, `codec`, `container`, `profile`, `pixel_format`,
  `interlaced`, `vfr`, `rotation`, `no_full_gop`, `open_gop`, `params_changed`, `seam_check`.

## NDJSON events

- `JobStage.stage` values for smart jobs: `head`, `copy`, `tail`, `audio`, `join`, `verify`. A
  run-time fallback then emits accurate mode's stages.
- `JobWarning` with `code: "SMART_FALLBACK"` and `detail.reason` is sent when a seam-check or
  parameter failure turns a smart job into an accurate one.

## Outcome (`JobFinished` and the run summary)

```json
{"status":"succeeded","measure":"clip","actual":{"start":3.32,"end":20.72},"adjusted":false,
 "smart":{"mode_used":"smart","copied_frames":400,"reencoded_frames":35,"copied_share":0.9195,
          "fallback":null}}
```

On a fallback: `"mode_used":"accurate","copied_frames":0,"reencoded_frames":435,
"copied_share":0.0,"fallback":"seam_check"`.
