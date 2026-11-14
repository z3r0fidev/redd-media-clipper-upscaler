# CLI contract: Smart Cut

Extends `specs/001-video-clipping/contracts/cli.md`. Nothing existing changes meaning.

## `rmedia clip` and `rmedia batch`

```text
--mode <fast|accurate|smart>   Cutting mode (default: settings, then fast).
                               smart: frame-exact; copies whole GOPs and re-encodes only the
                               partial GOPs at each end. H.264 in mp4, mov, mkv or ts; other
                               sources are cut in accurate mode with a SMART_FALLBACK warning.
--no-smart-fallback            With --mode smart: fail a job that cannot be smart-cut instead of
                               cutting it in accurate mode.
```

- `--mode smart` together with `--upscale` or `--gif` runs as accurate, and prints the note
  "smart cut does not apply: the clip is re-encoded for upscaling/GIF" (`SMART_NOT_APPLIED`).
- `--dry-run` prints, per smart job, the line
  `smart: head 0.680 s (17 fr) re-encoded · copy 16.000 s (400 fr) · tail 0.720 s (18 fr) re-encoded · 92% copied`
  or `smart → accurate: <reason message>`. A batch dry run ends with
  `smart: 3 jobs; fallback: 2 (open_gop 1, vfr 1)`.
- The human result line for a smart job adds `smart, 92% copied`, or
  `accurate (smart fallback: <reason>)`.

## Exit codes (unchanged set)

| Situation | Code |
|---|---|
| Smart jobs that fall back and then succeed | 0 |
| `--no-smart-fallback` and a reason found while planning (`SMART_UNAVAILABLE`) | 3 (invalid input, like other planning errors) |
| `--no-smart-fallback` and a seam-check or parameter failure while running | 1 (job failed) |
| Cancelled | 130 |

## `rmedia doctor`

There is a new line `smart cut: available (libx264)` or
`smart cut: unavailable — <reason>`. The JSON gains `"smart": {"available": bool, "reason":
string|null}` (additive).

## Settings

`defaults.mode` accepts `"smart"`, and `defaults.smart_fallback` accepts a boolean. Settings files
from 0.3.0 load unchanged.
