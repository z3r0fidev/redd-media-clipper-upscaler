# Manifest additions: upscaling

The manifest stays at **version 1**: every addition is optional and additive. 001 manifests run
unchanged.

## JSON

Every `options` object (`defaults`, per-video `options`, per-clip `options`) accepts an `upscale`
object, which is added to the `options` definition in `manifest.schema.json`:

```json
"upscale": {
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "enabled": { "type": "boolean" },
    "preset":  { "type": "string" },
    "reframe": { "enum": ["smart", "center", "blur"] },
    "tier":    { "enum": ["auto", "cpu", "gpu", "ai"] },
    "hdr":     { "enum": ["sdr", "keep"] },
    "motion":  { "enum": ["calm", "normal", "fast"] },
    "crf":     { "type": ["integer", "null"], "minimum": 0, "maximum": 51 }
  }
}
```

**Rules:**
- **Layering:** defaults < video < clip, field by field (data-model.md › PartialUpscale).
- **Enabling:** any field other than `enabled: false` turns upscaling on, and `{}` also turns it on
  with defaults. `{"enabled": false}` turns it off at that level.
- **Whole-video entries:** `clips` may be omitted when upscaling is enabled for the video. The video
  is then processed whole, as if it had one clip `{ "start": 0, "end": "end" }`. If `clips` is
  missing and upscaling is not enabled, the error is reported at the video's JSON path, as in 001.
- **Ignored or forced options:** `mode` is ignored for upscale jobs. A `container` other than `mp4`
  set by the manifest on a job that is upscaled is an error at the `options` object that set it
  (`defaults.container`, `videos[i].options.container`, …). A `--container` given on the command
  line is only a warning (`upscaled outputs are always .mp4`), because it may suit the plain clips.
- **`crf: null`** resets a crf set at a higher level to the preset's own quality (so an exported
  manifest can express every change, FR-022).

**Example:**
```json
{ "version": 1,
  "defaults": { "upscale": { "preset": "phone-fill" }, "output_dir": "phone" },
  "videos": [
    { "input": "talk.mp4" },
    { "input": "game.mkv", "options": { "upscale": { "preset": "reddit-916", "reframe": "smart" } },
      "clips": [ { "start": "1:10", "end": "1:40", "name": "clutch" } ] },
    { "input": "slides.mp4", "options": { "upscale": { "reframe": "blur", "tier": "cpu" } } } ] }
```

## CSV

New optional columns: `upscale_preset`, `upscale_reframe`, `upscale_tier`, `upscale_hdr`,
`upscale_motion`, `upscale_crf`.

- **Turning upscaling on:** a non-empty upscale cell enables upscaling for that row (`batch
  --upscale` enables it for every row). A row with upscaling on and empty `start`/`end`/`duration`
  processes the whole video; the `start` column may then be left out of the header.
- **Turning it off:** `upscale_preset` set to `off` disables upscaling for that row; it cannot be
  combined with other `upscale_*` values in the same row.
- **Values:** the same words as JSON (case-insensitive), `upscale_crf` 0–51; errors name the line.
- **Unknown columns:** still rejected, as in 001.
- **Writing:** a CSV export adds the `upscale_*` columns only when some row is upscaled, and writes
  every field of such a row (and `off` for the others), so the file does not depend on the defaults
  it is later read with.

## Desktop export

The desktop queue exports jobs with an `upscale` object. Using `PartialOptions::diff`, it writes only
the fields that differ from the manifest `defaults`. Running the exported file with `rmedia batch`
reproduces the desktop outputs (FR-022).
