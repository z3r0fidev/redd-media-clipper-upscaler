# JSON contract: Budgeted Reddit Video

These additions to `docs/api/json-output.md` (schema 1) only add keys. Jobs without a fit have
no `fit` key.

## Dry run: planned job

```json
"fit": {
  "target": "reddit-video",
  "target_verified": false,
  "cap": 100000000,
  "max_length": 900.0,
  "len": 180.0,
  "trimmed_from": null,
  "mode": "budget",
  "size": [1920, 1080],
  "fps": 30.0,
  "video_kbps": 4227,
  "crf": null,
  "audio_kbps": 128,
  "predicted_bytes": 97987500,
  "lowered": [],
  "passes": 2
}
```

- `mode` is `budget` (two-pass, `crf: null`) or `quality` (one pass, `crf` set, and
  `video_kbps` is the ceiling).
- `audio_kbps` is `null` when the output has no audio.
- `lowered` lists `{setting, from, to}` (for example `{"setting": "size", "from": [1920, 1080],
  "to": [1440, 810]}`).
- `trimmed_from` is the requested length in seconds when the range was trimmed.

## Job outcome (`job_finished`, `summary.jobs[]`)

```json
"fit": {
  "bytes": 95410000,
  "cap": 100000000,
  "mode": "budget",
  "video_kbps": 4227,
  "audio_kbps": 128,
  "attempts": [{"video_kbps": 4227, "bytes": 95410000}],
  "trimmed": false,
  "size": [1920, 1080],
  "fps": 30.0
}
```

## Progress

A budget-mode job reports the stages `pass1` (weight 0.45), `pass2` (0.45) and `verify` (0.10).
A retry restarts `pass2` within its share (003's `restart_stage`). A quality-mode job reports
`encode` and `verify`.

## Issue codes (new)

| Code | Severity | When |
|---|---|---|
| `RANGE_TOO_LONG` | error (exit 3) | the range is longer than the target allows, with no `allow_trim`; `detail: {len, max_length}` |
| `FIT_LOWERED` | notice | the size or the audio bitrate was lowered to fit; the message names from → to |
| `FIT_TRIMMED` | notice | the range was shortened (length limit or budget); `detail: {from, to}` |
| `FIT_REENCODES` | notice | `--mode fast` with `--fit`: cut in accurate mode |
| `FIT_FAILED` | job error (exit 1) | still over the cap after 3 retries; `detail: {cap, attempts}` |

These existing codes also apply to fits:
- `TARGET_UNKNOWN` (unknown video target);
- `BUDGET_UNREACHABLE` (`detail.min_cap`: the smallest cap that would work);
- `SMART_NOT_APPLIED`.
