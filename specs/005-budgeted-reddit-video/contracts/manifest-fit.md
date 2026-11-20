# Manifest contract: `fit`

This extends the JSON and CSV manifest formats (`docs/reference/file-formats.md`). `fit` can be
set at every level (`defaults`, a video, a clip), and each level overrides the one before it, as
`gif` and `upscale` do.

## JSON

```json
{
  "version": 1,
  "defaults": { "fit": { "target": "reddit-video" } },
  "videos": [
    {
      "input": "talk.mp4",
      "clips": [
        { "start": "00:10", "end": "03:10" },
        { "start": "05:00", "end": "25:00", "options": { "fit": { "allow_trim": true } } },
        { "start": "30:00", "end": "31:00", "options": { "fit": { "target": "sub-50mb", "max_size": "40MB" } } },
        { "start": "40:00", "end": "41:00", "options": { "fit": { "enabled": false } } }
      ]
    },
    { "input": "short.mp4" }
  ]
}
```

A video without `clips` is one whole-video job when a fit (or upscaling, or GIF export) is on.

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
`fit_allow_trim` (`true`/`false`). An empty cell inherits (from the command line's `--fit` flags),
`none` turns the fit off and cannot be combined with the other `fit_*` cells, and a row with a fit
and no times is the whole video. The writer adds the four columns only when some row has a fit. A
CSV and its equivalent JSON give the same requests (a test checks it).

`batch --fit TARGET` (with `--fit-max-size`, `--fit-max-length`, `--fit-allow-trim`) fits every
clip the manifest does not turn the fit off for; the manifest's own `fit` values win.

## Round trip

A manifest exported by the desktop app keeps only the values that differ from the defaults
(`PartialFit::diff`), and reads back to the same plan (FR-018).
