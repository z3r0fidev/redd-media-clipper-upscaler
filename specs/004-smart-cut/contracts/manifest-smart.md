# Manifest contract: Smart Cut

Extends `specs/001-video-clipping/contracts/manifest.schema.json` and `manifest-csv.md`. Manifests
written for 0.3.0 are read exactly as before.

## JSON (`version: 1`)

- `mode` accepts `"smart"` in `defaults`, in a video's `options` and in a clip's `options`.
- `smart_fallback` (boolean, default `true`) is accepted at the same three levels. `false` means
  strict mode.

```json
{"version":1,
 "defaults":{"mode":"smart","output_dir":"clips"},
 "videos":[
   {"input":"/v/match.mp4","clips":[
     {"start":"3.32","end":"20.72"},
     {"start":"61","end":"+30","options":{"smart_fallback":false}}]},
   {"input":"/v/phone.mp4","options":{"mode":"accurate"},"clips":[{"start":0,"end":5}]}]}
```

## CSV

- The `mode` column accepts `smart` (case-insensitive). The error for an unknown value becomes
  `mode must be fast, accurate or smart, not "<value>"`.
- There is a new optional column, `smart_fallback`, holding `true`, `false` or empty (the
  default).
- The writer emits `smart_fallback` after `mode` only when some row sets it, so 0.3.0 exports and
  the existing header snapshot stay unchanged.

## Export from the desktop app

`export_manifest` keeps `mode` and `smart_fallback` per clip, and `rmedia batch` reproduces the
app's outputs (SC-009).
