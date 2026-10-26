# Manifest additions: GIF options

This extends `specs/001-video-clipping/contracts/` (manifest formats) and
`specs/002-mobile-upscale/contracts/manifest-upscale.md`. Manifests stay `"version": 1`; the
`gif` key is additive.

## JSON

`gif` may appear in `defaults`, in a video's `options`, and in a clip's `options`. Layers are
applied defaults → video → clip, field by field (001's order). `"enabled": false` turns GIF off
for that layer and below. Unknown keys are rejected with the line number.

```json
{
  "version": 1,
  "defaults": { "gif": { "target": "reddit-post", "shape": "4:5", "framing": "smart" } },
  "videos": [
    { "input": "game.mkv",
      "clips": [
        { "start": "1:02", "end": "1:09" },
        { "start": "2:10", "end": "2:14", "options": { "gif": { "target": "reddit-comment", "allow_trim": true } } },
        { "start": "3:00", "end": "3:20", "options": { "gif": { "format": "mp4" } } }
      ] },
    { "input": "clip.mp4", "options": { "gif": { "budget": { "soft": 5000000 }, "loop": 3 } } }
  ]
}
```

| Key | Type | Notes |
|---|---|---|
| `enabled` | bool | |
| `target` | string | target id |
| `format` | `"gif"` \| `"mp4"` | |
| `engine` | `"auto"` \| `"builtin"` \| `"gifski"` | |
| `strict_engine`, `optimise`, `allow_trim` | bool | |
| `loop` | `"forever"` \| integer 1..=65535 | |
| `shape` | `"source"` \| `"9:16"` \| `"4:5"` \| `"1:1"` | |
| `framing` | `"smart"` \| `"center"` \| `"blur"` | |
| `width`, `fps`, `colours`, `lossy` | integer | ranges as in [cli.md](./cli.md); fps above 50 (GIF) is rejected |
| `dither` | `"none"` \| `"floyd_steinberg"` \| `"sierra2_4a"` \| `{"bayer": 0..5}` | |
| `budget` | `{"soft": bytes, "hard": bytes}` \| `null` | `null` = no search; `soft` alone sets `hard` = `soft`; `hard` alone keeps the target's `soft` (capped at `hard`) |

Other rules:
- A video with no `clips` and GIF on becomes a whole-video GIF job, as for upscale.
- `gif` together with `upscale` is one GIF job; the upscale is skipped with a note (see cli.md).
- The output `container` key is ignored for GIF jobs; `format` decides it.
- When the user exports the desktop queue as a manifest, it carries these keys, so the CLI
  reproduces the result (FR-020, SC-007).

## CSV

New optional columns: `gif_target`, `gif_format`, `gif_shape`, `gif_framing`, `gif_width`,
`gif_fps`, `gif_colours`, `gif_dither`, `gif_lossy`, `gif_loop`, `gif_budget`,
`gif_hard_limit`, `gif_allow_trim`, `gif_engine`, `gif_optimise`, `gif_strict_engine`.

- Empty cells inherit from the CLI flags and then the defaults.
- `gif_target=off` disables GIF for that row and cannot be combined with other `gif_*` cells.
- `gif_budget` and `gif_hard_limit` take the CLI size syntax (`3MB`), and `none` to disable.
- Booleans take `true`/`false`/`yes`/`no`/`1`/`0`.
- Enum errors read "gif_dither must be one of none, bayer, bayer:S, floyd-steinberg, sierra"
  and give the row number.
- If any `gif_*` header is present, or the CLI passed `--gif`, the `start` column becomes
  optional, as with `upscale_*`.
- The writer emits `gif_*` columns only when some job has GIF on.
