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
    "crf":     { "type": "integer", "minimum": 0, "maximum": 51 }
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
- **Ignored or forced options:** `mode` is ignored for upscale jobs, and `container` must be `mp4`
  or absent.

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

- **Turning upscaling on:** a non-empty upscale cell enables upscaling for that row. A row with only
  `input` and an upscale column, and empty `start`/`end`/`duration`, processes the whole video.
- **Turning it off:** `upscale_preset` set to `off` disables upscaling for that row.
- **Unknown columns:** still rejected, as in 001.

## Desktop export

The desktop queue exports jobs with an `upscale` object. Using `PartialOptions::diff`, it writes only
the fields that differ from the manifest `defaults`. Running the exported file with `rmedia batch`
reproduces the desktop outputs (FR-022).
