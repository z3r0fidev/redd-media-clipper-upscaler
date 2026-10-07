# JSON Output additions: upscaling

This extends `specs/001-video-clipping/contracts/json-output.md`. Every document keeps
`"schema": 1`. All changes are additive: consumers should ignore keys they don't recognise.

## `--dry-run --json`: upscale jobs

An upscale job has the same fields as a clip job, plus `kind` and an `upscale` block. `mode` and
`predicted` are omitted. `requested` is the span being processed: the clip span, or 0 to the end of
the video.

```json
{"id":1,"kind":"upscale","source":"/v/game.mkv","output":"/v/upscaled/game_phone-fill.mp4",
 "status":"planned","requested":{"start":0.0,"end":62.4},"warnings":[],
 "upscale":{
   "preset":"phone-fill","size":[1080,2340],"reframe":"smart","motion":"normal",
   "tier":"gpu","tier_reason":"auto: Intel(R) Graphics (RPL-U) passed the self-test",
   "suggest":null,"hdr":"sdr","no_upscale_needed":false,
   "estimate":{"bytes":38500000,"secs":41.0},"limit_warnings":[],
   "reframe_analysis":{
     "status":"ready",
     "crop":[608,1080],"axis":"x","fps":30.0,
     "shots":[{"start":0.0,"end":31.2,"kind":"tracked"},{"start":31.2,"end":62.4,"kind":"mixed"}],
     "fallback_ranges":[{"start":40.0,"end":44.5}],
     "path":{"hz":10,"keyframes":[[0.0,656],[0.1,656],[0.2,662]]}}},
 "cwd":"/v/upscaled/.game_phone-fill.mp4.r1a2-1.d","argv":["-hide_banner","..."]}
```

**`upscale` fields:**
- `tier` is `cpu`, `gpu` or `ai`.
- `suggest` is `"ai"` when AI is available but was not chosen automatically (research R9).
- `reframe_analysis.status` is `not_needed` (the aspect already matches, or the reframe is `center`
  or `blur`), `pending` (`--no-analyze`, or the analysis did not run) or `ready`.
- `path.keyframes` are `[seconds, crop_x]` pairs at 10 Hz. `--crop-path FILE` writes the
  per-frame path: `{"schema":1,"fps":30.0,"crop":[608,1080],"x":[656,656,…]}`.

**New issue codes:** `TIER_UNAVAILABLE`, `PRESET_UNKNOWN` and `HDR_UNSUPPORTED` (errors), and
`LIMIT_EXCEEDED` (warning).

## NDJSON run events

**New event `job_stage`.** The `fraction` in `job_progress` covers the whole job, weighted across
stages.

```
{"schema":1,"type":"job_stage","job_id":1,"stage":"analyze","index":1,"of":3}
```

**Upscale success outcome:**
```json
{"status":"succeeded","output":"/v/upscaled/game_phone-fill.mp4",
 "measure":"upscale","span":{"start":0.0,"end":62.4},
 "tier_used":"cpu","tier_fallback":true,"fallback_ranges":[{"start":40.0,"end":44.5}],
 "no_upscale_needed":false}
```

- `measure` is `"clip"` or `"upscale"`.
- Clip outcomes keep `actual` and `adjusted` exactly as in 001, with `"measure":"clip"` added.
- When `tier_fallback` is true, the run has also emitted a `job_warning`:
  "GPU tier failed (…); retried on CPU".

## `rmedia upscale --list-presets --json`

```json
{"schema":1,"type":"presets","default":"phone-fill",
 "presets":[{"id":"phone-fill","name":"Phone fill","size":[1080,2340],"devices":["…"],
             "codec":"hevc","max_fps":60,"hdr_capable":true,"limits":null,"source":"builtin"}],
 "problems":[{"file":"/home/u/.config/rmedia/presets/bad.json","message":"…"}]}
```

## `doctor --json` additions

```json
{"tiers":{"cpu":{"available":true,"fps":96.0},
          "gpu":{"available":true,"device":0,"fps":69.0},
          "ai":{"available":false,"reason":"too slow on this GPU: 0.7 fps (needs ≥ 2.6 fps for a 60 s 480p clip in 10 min)"}},
 "gpus":[{"index":0,"name":"Intel(R) Graphics (RPL-U)","kind":"integrated","usable":true},
         {"index":1,"name":"llvmpipe (LLVM 20.1.2, 256 bits)","kind":"software","usable":false,"reason":"software renderer"}],
 "models":{"yunet":"ok","realesr-animevideov3":"missing"},
 "capability":{"checked_at":"2026-10-07T10:00:00Z","fingerprint":"…","demoted":[]}}
```
