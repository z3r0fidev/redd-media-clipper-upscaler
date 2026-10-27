# CLI Contract additions: GIF export

This extends `specs/001-video-clipping/contracts/cli.md` and
`specs/002-mobile-upscale/contracts/cli.md`. Output discipline, `--json`, `--dry-run`, Ctrl-C
handling and exit codes are unchanged unless stated here.

## `rmedia gif`

```
rmedia gif <INPUT|DIR>... [GIF OPTIONS] [SHARED OPTIONS]
rmedia gif --inputs-from <FILE|-> [GIF OPTIONS] [SHARED OPTIONS]
rmedia gif --list-targets [--json]
```

- One job per input video.
- `-s/--start`, `-e/--end` and `-d/--duration` limit the range, with 001's time syntax and
  flags. They apply to every input. They belong to `rmedia gif` only, not to `GifFlags`.
  For different ranges per video, use `rmedia clip … --gif` or a manifest.
- Folders are scanned with 001's scan flags. The default output folder (`gifs/`) and the `-o`
  folder are excluded from the scan.

### GIF options (the reusable `GifFlags` group)

| Flag | Values | Default |
|---|---|---|
| `--target ID` | a target id (`--list-targets`) | `reddit-post` |
| `--format` | `gif` \| `mp4` (the Reddit MP4, silent loop) | `gif` |
| `--width N` | 64..=1920; never above the source/reframed width | target |
| `--fps N` | 1..=50 for GIF, 1..=60 for MP4; above that is a usage error | target |
| `--colors N` (alias `--colours`) | 2..=256 (built-in engine only) | target |
| `--dither` | `none` \| `bayer` \| `bayer:S` (S 0..=5) \| `floyd-steinberg` \| `sierra` | target |
| `--lossy N` | 0..=200 (requires gifsicle) | target, or 0 |
| `--loop` | `forever` \| N plays (1..=65535) | `forever` |
| `--budget SIZE` | the size to aim for, e.g. `3MB`, `2.5MiB`, `800k`; `none` disables the search | target |
| `--hard-limit SIZE` | never write above this | the target's hard limit; **with `--budget` given, the budget** (research R5) |
| `--allow-trim` | allow shortening the range when no setting fits (FR-011) | off |
| `--engine` | `auto` \| `builtin` \| `gifski` | `auto` (the target's engine) |
| `--strict-engine` | fail (exit 4) instead of falling back when the engine is missing | off |
| `--no-optimise` | never run gifsicle | optimise when present |
| `--shape` | `source` \| `9:16` \| `4:5` \| `1:1` | `source` |
| `--framing` | `smart` \| `center` \| `blur` | `smart` |
| `--no-sample` | with `--dry-run`: skip or run the sample encodes (estimate `pending` when skipped) | sample for `gif` and `clip`; **no sample for `batch`** |
| `--crop-path FILE` | with smart framing, as in 002 | — |

Sizes are decimal (`MB` = 10^6) unless a binary suffix (`MiB`) is used. They must be positive and
at most 1 GiB.

With `--engine gifski`, `--colors`, `--dither` and `--lossy` are ignored with a note; the target's
`gifski` parameters apply (research R14).

**Shared options from 001 that apply:** `-o`, `--name`, `--on-exists`, `-j`, `--dry-run`,
`--json`, `--ffmpeg`/`--ffprobe`, `-v`, `-q`.

**Shared options that don't apply:**
- `--mode` is not accepted (GIF export always re-encodes).
- `--container` is not accepted: `--format` picks the container.
- `--audio`/`--no-audio`/`--no-subs`: the output is always silent and subtitle-free.

**Engine paths:** `--gifsicle PATH` and `--gifski PATH`. Lookup order (research R7): flag or
setting, then `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI`, then the bundled `rmedia-gifsicle`/`rmedia-gifski`
beside the executable, then `gifsicle`/`gifski` on `PATH` (only when no bundled copy exists). A
given path that is missing or fails the version check makes that engine unavailable, with a
reason. It is not an error, except with `--strict-engine`.

**Name variables:** adds `{target}` and `{format}`. `{ext}` is `gif` or `mp4`. Outputs go into
`<source dir>/gifs/`. The default templates (research R16) are:
- whole video: `{source}_{target}.{ext}`;
- clips: `{source}_{index:03}_{name-or-range}_{target}.{ext}`;
- many sources into one `-o` folder: the clip template under `{source}/`, as for 001.

**Dry run writes:** no outputs. Unless `--no-sample` is given, it **runs ffmpeg (and gifsicle or
gifski) on the input**: the estimate is the predicted result of the budget search (research R4),
encoded as samples in a temporary scratch folder that is then removed. It may update the reframe
cache. The help text says this.

**Human dry run**, per job:
- source → output;
- target, with "(limits unverified)" when the target is not verified;
- format and engine (with the fallback reason);
- width × height, fps (exact, or "≈ 6.67 cs, approximate"), colours, dither;
- budget soft/hard;
- the estimate (the predicted result of the budget search) and the settings it expects to lower,
  plus for `reddit-post` GIFs "Reddit MP4 would be ≈ N MB";
- warnings (long range, approximate timing, engine fallback);
- for smart framing, the reframe summary as in 002.

**Human run summary**, per job: the output, size, width × height, fps, colours, dither, engine,
the lowered settings ("lowered: dither bayer4→bayer5, width 720→612"), and any trim.

### Exit codes

Unchanged:

| Code | Meaning |
|---|---|
| 0 | all jobs succeeded or were skipped |
| 1 | at least one job failed, including `BUDGET_UNREACHABLE` at run time |
| 2 | usage error |
| 3 | invalid input, including `TARGET_UNKNOWN` and a budget below the smallest achievable size |
| 4 | engine missing (no gif muxer/palette filters for GIF, no libx264 for MP4, or the `--strict-engine` engine) |

`BUDGET_UNREACHABLE` at plan time is only the static check: below 64 KiB without `--allow-trim`.
| 130 | cancelled |

## `rmedia clip … --gif` and `rmedia batch … --gif`

- `--gif` turns GIF export on for every job and accepts all `GifFlags`. Each clip becomes a GIF
  or MP4 in one job, with no intermediate video (FR-016).
- `clip`: a GIF flag without `--gif` is a usage error (exit 2): "error: --target needs --gif".
  `batch` (like its upscale flags in 002): GIF flags without `--gif` fill the clips the
  manifest makes GIFs, with a warning when it makes none. `--crop-path` and `--no-sample` do
  not apply to `batch`.
- `--gif` with `--upscale` stays one GIF job: the upscale is skipped, with the note "upscale
  skipped: a GIF is never larger than its source" (research R16; FR-004). The GIF's own shape
  and framing apply 002's reframe at the GIF size.
- `clip` keeps its own range flags (`-s`, `-e`, `-d`, `-l`), so `rmedia clip in.mp4 -s 0:01 -e 0:03 --gif`
  and `rmedia clip in.mp4 -l clips.txt --gif` both work.

## `rmedia gif --list-targets`

Human output is one line per target: id, name, budget (soft/hard), widths, fps, and `verified`,
then `[user]` or `[user override]` for a target from a user file.
`--json` gives
`{"schema":1,"type":"targets","targets":[GifTarget…],"problems":[…]}`.

## `rmedia doctor`

New lines:
```
gif: yes (palettegen, paletteuse, gif muxer)
reddit mp4: yes (libx264)
gifsicle: 1.98 (bundled /opt/rmedia/rmedia-gifsicle)
gifski: 1.34.0 (bundled /opt/rmedia/rmedia-gifski)
```
- A missing engine shows `no (not found: set gifsicle_path or RMEDIA_GIFSICLE)`.
- `status` keeps its 001/002 meaning. GIF/MP4 unavailability and missing optional engines are
  reported on their own lines and in `problems`, but do not make the status `NOT READY`
  (Principle VI).
- `--json` adds the `gif` block ([json-output.md](./json-output.md)).
