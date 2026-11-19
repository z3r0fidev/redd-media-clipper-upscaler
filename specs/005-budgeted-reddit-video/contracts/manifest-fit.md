# Manifest contract: `fit`

This extends the JSON and CSV manifest formats (`docs/reference/file-formats.md`). `fit` can be
set at every level (`defaults`, a video, a clip), and each level overrides the one before it, as
`gif` and `upscale` do.

## JSON

```json
{
  "defaults": { "fit": { "target": "reddit-video" } },
  "videos": [
    {
      "path": "talk.mp4",
      "clips": [
        { "start": "00:10", "end": "03:10" },
        { "start": "05:00", "end": "25:00", "fit": { "allow_trim": true } },
        { "start": "30:00", "end": "31:00", "fit": { "target": "sub-50mb", "max_size": "40MB" } },
        { "start": "40:00", "end": "41:00", "fit": { "enabled": false } }
      ]
    }
  ]
}
```

| Key | Type | Notes |
|---|---|---|
| `enabled` | bool | any other `fit` key turns it on; `false` turns an inherited fit off |
| `target` | string | a video target id |
| `max_size` | string or integer | `"50MB"`, `"8MiB"`, or bytes; may only lower the target's cap |
| `max_length` | string or number | a duration (`"5:00"`, seconds); may only lower the limit |
| `allow_trim` | bool | |

- Unknown keys are refused (exit 3), as elsewhere.
- `fit` together with `gif` on the same clip is refused (exit 3).

## CSV

The new columns are `fit` (a target id, or `none`), `fit_max_size`, `fit_max_length` and
`fit_allow_trim` (`true`/`false`). An empty cell inherits. The defaults row and the row order work
as for the GIF columns, and a CSV and its equivalent JSON give identical plans (a test checks it).

## Round trip

A manifest exported by the desktop app keeps only the values that differ from the defaults
(`PartialFit::diff`), and reads back to the same plan (FR-018).
