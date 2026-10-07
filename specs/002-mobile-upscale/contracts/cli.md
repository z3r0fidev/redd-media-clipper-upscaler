# CLI Contract additions: upscaling

This extends `specs/001-video-clipping/contracts/cli.md`. Output discipline, `--json`, `--dry-run`,
Ctrl-C handling and exit codes are unchanged unless stated here.

## `rmedia upscale`

```
rmedia upscale <INPUT|DIR>... [UPSCALE OPTIONS] [SHARED OPTIONS]
rmedia upscale --inputs-from <FILE|-> [UPSCALE OPTIONS] [SHARED OPTIONS]
rmedia upscale --list-presets [--json]
```

- Each input video is processed whole: one job per video.
- Folders are scanned with 001's scan flags (`--no-recursive`, `--include`, …). When scanning, the
  default output folders (`upscaled/`) and the `-o` folder are excluded.
- To upscale part of a video, use `rmedia clip … --upscale` or a manifest.

### Upscale options

| Flag | Values | Default |
|---|---|---|
| `--preset ID` | a preset id (`--list-presets`) | `phone-fill` |
| `--reframe` | `smart` \| `center` \| `blur` | `smart` |
| `--tier` | `auto` \| `cpu` \| `gpu` \| `ai` | `auto` |
| `--hdr` | `sdr` \| `keep` | `sdr` |
| `--motion` | `calm` \| `normal` \| `fast` | `normal` |
| `--crf N` | override the preset's quality | preset value |
| `--crop-path FILE` | with smart reframe, write the full crop path as JSON (FR-016) | — |
| `--no-analyze` | with `--dry-run`, skip smart-reframe analysis (path shown as `pending`) | analyse |

**Shared options from 001 that apply:** `-o`, `--name`, `--on-exists`, `--no-audio`/`--audio`,
`--no-subs`, `-j`, `--dry-run`, `--json`, `--ffmpeg`/`--ffprobe`, `-v`, `-q`.

**Shared options that don't apply:**
- `--mode` is not accepted, because upscaling always re-encodes.
- `--container` accepts only `mp4`.

**Name variables:**
- Adds `{preset}`.
- The default template is `{source}_{preset}.{ext}`, written into `<source dir>/upscaled/`.
- With more than one source and `-o`, the default becomes `{source}_{preset}.{ext}`. Names don't
  collide because a source stem is unique within one folder; across folders, 001's collision rules
  apply.

**Human dry run** prints, per job:
- source and output;
- preset with its size, reframe mode and tier (with the reason);
- estimated size and time;
- any limit warnings;
- for smart reframe: shot count and the fallback ranges as `mm:ss–mm:ss`.

## `rmedia clip … --upscale`

`clip` accepts `--upscale` plus every upscale option above. Every clip is then cut and upscaled in
one pass (FR-018). Upscale options without `--upscale` are a usage error (exit 2).

## `rmedia batch`

Upscaling follows the manifest's `upscale` fields ([manifest-upscale.md](./manifest-upscale.md)).
`batch` also accepts the upscale flags, which act as command-line defaults; layering is unchanged
from 001.

## `rmedia doctor`

- Adds:
  - the tiers, each with availability, device, measured fps and the reason it is unavailable;
  - the GPUs found;
  - each model with its status;
  - the presets, with any invalid user preset files.
- `--recheck` forces the capability self-test (FR-009) and rewrites the record.

## Exit codes

These are unchanged.

| Code | Meaning in upscaling |
|---|---|
| 3 | Invalid input. Includes an unknown preset, `--hdr keep` with a preset that can't keep HDR, and a forced tier that is unavailable (`TIER_UNAVAILABLE`). Nothing runs. |
| 4 | The engine is missing or lacks a required encoder or filter (libx265, zscale, …). |
| 1 | One or more jobs failed, including after an automatic tier fallback that also failed. |

New issue codes:
- `TIER_UNAVAILABLE` (error);
- `PRESET_UNKNOWN` (error);
- `HDR_UNSUPPORTED` (error);
- `LIMIT_EXCEEDED` (warning).

## Determinism

- The CPU tier gives byte-identical output for identical inputs, options and engine on the same
  machine.
- GPU and AI tiers may differ slightly between devices or drivers.
