# JSON Output additions: GIF export

This extends `specs/001-video-clipping/contracts/json-output.md` and
`specs/002-mobile-upscale/contracts/json-output.md`. Every document keeps `"schema": 1`. All
changes are additive.

## `--dry-run --json`: gif jobs

A GIF job has the shared job fields, plus `"kind":"gif"` and a `gif` block. When framing is
smart, it also has 002's `reframe_analysis` block. For a chained job, the `upscale` block is
present too.

```json
{"id":1,"kind":"gif","source":"/v/game.mkv","output":"/v/gifs/game_reddit-post.gif",
 "status":"planned","requested":{"start":62.0,"end":69.0},
 "warnings":["15 fps is shown as 6–7 cs frame delays (≈ 15.0 fps)"],
 "notices":[{"code":"TIMING_APPROXIMATE","message":"15 fps is shown as 6–7 cs frame delays (≈ 15.0 fps)"}],
 "argv":["…final encode…"],"pipeline":[["…palette…"],["…encode…"],["…gifsicle…"]],
 "gif":{
   "target":"reddit-post","target_verified":false,"format":"gif",
   "engine":"builtin","engine_reason":"target engine: builtin","optimiser":"gifsicle 1.98",
   "settings":{"width":720,"height":405,"fps":15,"timing":{"exact":false,"avg_cs":6.67},
               "colours":256,"dither":{"bayer":4},"lossy":30},
   "floors":{"width":320,"fps":10},
   "budget":{"soft":8000000,"hard":20000000},
   "loop":"forever","allow_trim":false,
   "estimate":{"bytes":6100000,"from":"search","lowered":[],"settings":{"…":"…"},"mp4_bytes":820000},
   "notes":[]
 }}
```

- `warnings` stays an array of strings, exactly as in 001/002. GIF jobs add `notices`, which
  carry the same messages with their codes (research R16).
- `argv` is the final encode, as before. `pipeline` lists every process of the job, in order.
- `estimate.from`:
  - `"search"`: the predicted result of the budget search, with the settings it expects to lower;
  - `"sample"`: no budget, so one sample;
  - `"pending"` (`bytes: null`): `--no-sample`, or any `batch --dry-run` (batch never samples).
- Planning errors use the existing `issues` shape. New codes:
  - `TARGET_UNKNOWN`;
  - `BUDGET_UNREACHABLE` with `"detail":{"min_bytes":65536}`, for the static floor only.

## NDJSON events

- `JobStage.stage` values for GIF jobs: `analyze`, `search`, `palette`, `encode`, `optimise`,
  `verify`.
- `JobProgress.fraction` is weighted as described in [data-model.md](../data-model.md).
- `JobWarning` gains an optional `code` (additive): `OVER_RECOMMENDED`, `ENGINE_FALLBACK`,
  `TIMING_APPROXIMATE`, `LONG_RANGE`. Existing warnings have no code.

## Outcome (`JobFinished` and the run summary)

```json
{"type":"job_finished","job_id":1,"outcome":{"status":"succeeded",
  "output":"/v/gifs/game_reddit-post.gif","measure":"gif",
  "span":{"start":62.0,"end":69.0},"format":"gif","bytes":2911034,
  "width":612,"height":344,"fps":12,"timing":{"exact":false,"avg_cs":8.33},
  "colours":128,"dither":{"bayer":5},"lossy":90,"engine":"builtin","optimiser":true,
  "loop":"forever","deterministic":true,"trimmed":null,"mp4_estimate":410000,
  "search":{"encodes":6,"fit":"soft",
    "lowered":[{"setting":"lossy","from":30,"to":90},{"setting":"dither","from":{"bayer":4},"to":{"bayer":5}},
               {"setting":"colours","from":256,"to":128},{"setting":"fps","from":15,"to":12},
               {"setting":"width","from":720,"to":612}],
    "steps":[{"n":1,"kind":"sample","bytes":1210400,"estimate":9300000,"settings":{"…":"…"}},
             {"n":2,"kind":"post","bytes":1080000,"estimate":8100000,"settings":{"…":"…"}},
             {"n":8,"kind":"full","bytes":2911034,"estimate":2911034,"settings":{"…":"…"}}]}}}
```

- Without a budget, `search` is `null`.
- A run-time budget failure gives:
  `{"status":"failed","error":{"code":"BUDGET_UNREACHABLE","message":"…","detail":{"fits_secs":4.2,"best_bytes":9800000}}}`.
  `ErrorInfo.detail` is a new optional object (additive). Other errors don't have it.
- Nothing is written in that case.

## `doctor --json`

`DoctorReport` gains:

```json
"gif":{"gif":true,"mp4":true,"missing":[],
  "engines":[
    {"name":"gifsicle","path":"/opt/rmedia/rmedia-gifsicle","source":"bundled","version":"1.98","ok":true,"reason":null},
    {"name":"gifski","path":null,"source":null,"version":null,"ok":false,"reason":"not found"}],
  "target_problems":[]}
```

## `gif --list-targets --json`

`{"schema":1,"type":"targets","targets":[…GifTarget…],"problems":[{"file":"…","message":"…"}]}`
