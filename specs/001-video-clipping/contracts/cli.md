# CLI Contract: `rmedia`

Output discipline: results → stdout; progress, logs, warnings → stderr. With `--json`, stdout
carries only JSON (see [json-output.md](./json-output.md)).

## Commands

### `rmedia clip`
```
rmedia clip <INPUT|DIR>... (-s START [-e END | -d +DURATION] | -l CLIPLIST) [OPTIONS]
rmedia clip --inputs-from <FILE|-> (-s ... | -l ...) [OPTIONS]
```
- One clip (`-s/-e/-d`) or a clip list (`-l`, see [cliplist-format.md](./cliplist-format.md))
  applied to every input. Directories are scanned with the scan flags below (FR-009).
- With > 1 source and `-o`, the default template becomes `{source}/{source}_{index:03}_{name-or-range}.{ext}`.
- When folders are scanned for `clip`, rmedia's own output location is excluded so re-runs never
  clip previous clips: the `-o` folder if given, otherwise every `clips/` folder.

### `rmedia batch <MANIFEST>`
- `MANIFEST` is `.json` ([manifest.schema.json](./manifest.schema.json)) or `.csv`
  ([manifest-csv.md](./manifest-csv.md)). `--continue-on-missing` runs remaining entries when some
  inputs are missing (default: abort with exit 3).

### `rmedia scan <DIR|FILE>...`
- `--no-recursive`, `--max-depth N`, `--include GLOB`*, `--exclude GLOB`*, `--hidden`,
  `--follow-symlinks`, `--paths` (one path per line; pipe into `--inputs-from -`).

Notes: globs are case-insensitive on every platform. `--paths` prints one path per line and skips
(with a warning) names containing line breaks or invalid UTF-8, so piping into
`--inputs-from -` can never inject extra entries. On Windows, online-only cloud files (OneDrive
etc.) are skipped and counted, because probing them would download them. When `clip` scans folders,
`-o` must not be one of the scanned folders.

### `rmedia doctor`
- Engine version/location (`bundled|config|path`), encoders/muxers, accurate-mode containers.

## Shared clip/batch options
| Flag | Values | Default |
|---|---|---|
| `--mode` | `fast` \| `accurate` | `fast` |
| `-o, --output-dir` | path | `<source dir>/clips` |
| `--name` | template (`{source} {index} {index:03} {name} {start} {end} {range} {name-or-start} {name-or-range} {ext}`) | see data-model |
| `--container` | ext | source's |
| `--on-exists` | `skip` \| `overwrite` \| `rename` | `skip` |
| `--no-audio`, `--audio 1,2`, `--no-subs` | | all streams |
| `-j, --jobs` | N ≥ 1 | clamp(ncpu/2,1,4) |
| `--dry-run` | | off — plan only, writes nothing |
| `--json` | | off |
| `--ffmpeg`, `--ffprobe` | path | locator |
| `-v` / `-q` | | |

## Exit codes
| Code | Meaning |
|---|---|
| 0 | all jobs succeeded or were skipped |
| 1 | ≥ 1 job failed (others completed) |
| 2 | usage error |
| 3 | input/validation error — nothing ran |
| 4 | engine missing / too old / required encoder absent |
| 130 | cancelled (Ctrl-C: first = graceful cancel + summary, second = immediate) |

## Examples
```
rmedia clip game.mkv -s 1:30 -e 2:00
rmedia clip game.mkv -s 90 -d +15 --mode accurate -o out/
rmedia clip game.mkv -l highlights.txt --dry-run
rmedia scan ~/Videos --include '*.mp4' --paths | rmedia clip --inputs-from - -l intro.txt
rmedia batch jobs.csv --json > run.ndjson
```
