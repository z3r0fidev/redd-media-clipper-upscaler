# rmedia File Formats

This page is the reference for every text format rmedia reads or writes: the time grammar, clip
lists, path lists, JSON and CSV manifests, and output name templates. It also covers how
options are layered.

Authoritative sources: `crates/rmedia-core/src/{time.rs,input/*.rs,plan/naming.rs,request.rs}`.
Contracts: `specs/001-video-clipping/contracts/{cliplist-format.md,manifest.schema.json,manifest-csv.md}`.
Where code and contract differ, this page follows the code and says so.

## Time Grammar

Implemented by `Ts::parse` and `EndSpec::parse` (`time.rs`). Leading and trailing whitespace is
ignored.

| Form | Examples | Rules |
|---|---|---|
| Seconds | `0`, `90`, `90.5`, `0.000001` | At most 12 integer digits and 6 fractional digits. `.` decimal separator only. |
| Clock `[HH:]MM:SS[.frac]` | `1:30`, `01:30.5`, `00:01:30.500`, `00:01:30,500`, `90:00`, `100:00:00` | 2 or 3 fields. The seconds field has 1–2 integer digits and an optional `.` or `,` fraction. Non-leading fields must be below 60 and at most 2 digits. The leading field may exceed 59. |
| Units | `1h`, `2m`, `90s`, `1m30s`, `1h2m3.5s`, `1.5m`, `500ms`, `1s500ms` | Units `h`, `m`, `s`, `ms` in descending order, each at most once. Values may have decimals. No spaces between number and unit. |
| End of media | `end` (case-insensitive) | Ends only |
| Duration | `+15`, `+1:00`, `+30s` | Ends only: `+` followed by any position form |

**Rejected** (from the unit tests): empty input, negative values (`-1`, `-0:30`), `1:7x`, `1:60`,
`01:60:00`, four fields, `:30`, `1:`, `1.2.3`, exponents (`1e3`), `90,5` (a comma decimal is only
valid in clock form), more than 6 fractional digits, hex, units out of order (`1m1h`) or repeated
(`1s1s`), `5 s`, `inf`, `NaN`, and non-ASCII digits.

Error text: `invalid time "<input>": <reason>`, for example
`invalid time "1:7x": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s`.

In JSON, times may also be numbers of seconds (≥ 0, finite).

## Clip Lists

A clip list holds one clip per line for one video. The same list is applied to every input of
`rmedia clip -l FILE` and of the desktop app's **Import list…** (`input/cliplist.rs`).

```text
START  END        [NAME...]
START  +DURATION  [NAME...]
```

- UTF-8; a BOM on the first line is tolerated; LF or CRLF line endings.
- Fields are separated by whitespace or a comma plus optional whitespace.
- **Decimal comma rule:** a comma directly between `:SS` and exactly three digits that end the
  field (`00:01:30,500`) is a decimal separator. Every other comma separates fields.
- `END` may be `end`. `NAME` is the rest of the line, trimmed; one leading comma separator is
  dropped. Names may contain spaces and commas (`1m +30s a, b` → name `a, b`).
- **Comments:** a line whose first non-blank character is `#`, or a ` # ` (a `#` with whitespace
  or the line end on both sides) up to the end of the line. A `#` inside a name such as `goal #3`
  is kept. Blank lines are ignored.
- A list with no clips is an error.

```text
# highlights for match 12
0:45          1:10   opening goal
00:03:15.250  +20    save
12:00,500     end
```

**Errors** are all reported before anything runs (exit 3), as `<file>:<line>: <reason>`. Real
example:

```text
error: /work/bad.txt:3: missing end after "0:05" (use END, +DURATION or end)
error: /work/bad.txt:4: invalid time "1:7x": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s
```

File-level problems (unreadable file, empty list) have no line number. Times beyond the video,
and end not after start, are reported at plan time with the same origin, for example
`<file>:<line>: end (…) is not after start (…)`.

## Path Lists (`--inputs-from`)

`input/pathlist.rs`:

- One path per line. Lines are used verbatim apart from a trailing `\r`, so leading and trailing
  spaces in names are kept.
- A line whose first non-blank character is `#` is a comment. To name a file starting with `#`,
  write `./#name`.
- Blank lines are ignored. A BOM on the first line is tolerated.
- Relative paths resolve against the list file's folder, or the working directory for stdin
  (`-`).

`rmedia scan --paths` produces this format and never prints names containing line breaks or
invalid UTF-8.

## JSON Manifest (v1)

Schema: `specs/001-video-clipping/contracts/manifest.schema.json`. Parser:
`input/manifest_json.rs`. The desktop queue uses the same document (`apps/desktop/src/lib/manifest.ts`).

```json
{
  "version": 1,
  "defaults": { "mode": "fast", "output_dir": "clips" },
  "videos": [
    { "input": "match12.mkv",
      "clips": [ { "start": "0:45", "end": "1:10", "name": "opening goal" } ] },
    { "input": "match13.mp4", "options": { "mode": "accurate" },
      "clips": [ { "start": 30, "duration": 12.5 },
                 { "start": "12:00", "end": "end", "options": { "container": "mkv" } } ] }
  ]
}
```

### Structure

| Path | Type | Required | Notes |
|---|---|---|---|
| `version` | integer | Yes | Must be `1` |
| `defaults` | options object | No | Manifest-wide defaults (see [Option layering](#option-layering)) |
| `videos` | array, ≥ 1 | Yes | |
| `videos[].input` | string | Yes | Relative paths resolve against the manifest's folder |
| `videos[].options` | options object | No | Per-video overrides |
| `videos[].clips` | array, ≥ 1 | Yes, unless upscaling is on for the video | Left out with upscaling on: the whole video is one job (see [Upscaling](#upscaling-in-manifests)) |
| `clips[].start` | time | Yes | Number of seconds or a grammar string. **No** `+`. |
| `clips[].end` | time | No | Number = position. String = `end`, `+DURATION` (a duration), or a position. |
| `clips[].duration` | time | No | Number or string; a leading `+` is optional. Not allowed together with `end`. |
| `clips[].name` | string | No | A blank name counts as no name |
| `clips[].options` | options object | No | Per-clip overrides |

A clip with neither `end` nor `duration` runs to the end of the video.

### Options Object

| Key | Values | Notes |
|---|---|---|
| `mode` | `"fast"` \| `"accurate"` | |
| `output_dir` | string | Relative folders resolve against the manifest's folder |
| `name_template` | string | See [Name templates](#name-templates) |
| `container` | string | Output extension; a leading `.` is ignored |
| `on_exists` | `"skip"` \| `"overwrite"` \| `"rename"` | |
| `audio` | `"all"` \| `"none"` \| array of 1-based track numbers | |
| `subtitles` | `"all"` \| `"none"` | |
| `upscale` | upscale object | Feature 002; see [Upscaling in manifests](#upscaling-in-manifests) |

### Upscaling in Manifests

Contract: `specs/002-mobile-upscale/contracts/manifest-upscale.md`. The manifest stays at
version 1; 001 manifests run unchanged.

```json
{ "version": 1, "defaults": { "upscale": {} },
  "videos": [ { "input": "low.mp4" },
              { "input": "land.mp4", "options": { "upscale": { "preset": "reddit-916" } },
                "clips": [ { "start": 1, "end": 5, "name": "pan" } ] },
              { "input": "slides.mp4", "options": { "upscale": { "reframe": "blur", "tier": "cpu" } } } ] }
```

This writes `upscaled/low_phone-fill.mp4` (1080x2340), `clips/land_001_pan_reddit-916.mp4`
(1080x1920) and `upscaled/slides_phone-fill.mp4` (1080x2340).

| Key | Values | Notes |
|---|---|---|
| `enabled` | `true` \| `false` | `false` turns upscaling off at this level |
| `preset` | preset id | See `rmedia upscale --list-presets` |
| `reframe` | `"smart"` \| `"center"` \| `"blur"` | |
| `tier` | `"auto"` \| `"cpu"` \| `"gpu"` \| `"ai"` | |
| `hdr` | `"sdr"` \| `"keep"` | |
| `motion` | `"calm"` \| `"normal"` \| `"fast"` | |
| `crf` | 0–51 or `null` | `null` resets a crf set at a higher level to the preset's |

- An `upscale` object turns upscaling on at its level (`{}` with the defaults), unless it says
  `"enabled": false`. Fields layer defaults < video < clip, one by one; unknown fields are
  rejected.
- A video without `clips` is valid only when upscaling is on for it; otherwise the error is at
  `videos[i]`: `no clips for this video (list clips, or turn upscaling on to process the whole video)`.
- `mode` does not apply to upscaled jobs. A `container` other than `mp4` that the manifest sets
  for an upscaled job is an error at the object that set it, for example
  `videos[3].options.container: upscaled outputs are always .mp4; container "mov" cannot be used`.
- `rmedia batch` upscale flags fill the fields of upscaled videos under these values; `--upscale`
  turns upscaling on for every video ([CLI reference](cli-reference.md#rmedia-batch)).

### Strictness and Errors

- Unknown fields are rejected at every level (`deny_unknown_fields`), for example
  ``unknown field `bogus`, expected one of `version`, `defaults`, `videos` ``.
- Syntax and type errors stop parsing. They are reported with the line number, and the message
  ends in `at line L column C`.
- Semantic errors (bad times, `end` and `duration` together, empty `clips`, wrong `version`) are
  all reported at once, located by JSON path. Real example:

  ```text
  error: /work/bad2.json:videos[0].clips[0].start: invalid time "1:7x": expected seconds, [HH:]MM:SS[.mmm] or 1h2m3s
  error: /work/bad2.json:videos[0].clips[1]: give either end or duration, not both
  ```

### Written by the Desktop App

The desktop **Export…** to `.json` writes the queue manifest with the app's **Settings** defaults
folded into `defaults`; values already in the queue's `defaults` win (`ops::with_settings_defaults`).
`rmedia batch` on the export therefore reproduces the app's run.

`manifest_json::write` (used for the queue built from clip lists and opened manifests) writes
`version`, `defaults` and `videos`. Times exact to the millisecond
are written as `HH:MM:SS.mmm` strings; other times as seconds. Empty per-video `options` are
omitted. A parse → write → parse round trip is lossless (unit-tested).

## CSV Manifest

Contract: `contracts/manifest-csv.md`. Parser and writer: `input/manifest_csv.rs`.

```csv
input,start,end,duration,name,mode,output
match12.mkv,0:45,1:10,,opening goal,,
match13.mp4,30,,12.5,,accurate,
match13.mp4,12:00,end,,,accurate,
```

- RFC 4180 CSV, UTF-8, BOM tolerated. Quoted fields may contain commas, `""` and newlines.
- A header row is **required**. Column names are case-insensitive and may come in any order.
  Unknown and duplicate columns are errors. `input` and `start` are required columns.
- Lines starting with `#` are comments, including above the header. Fields are trimmed.

| Column | Required | Meaning |
|---|---|---|
| `input` | Yes | Video path (relative → manifest's folder). `a.mp4` and `./a.mp4` are the same source. |
| `start` | Yes | Time; never `+` |
| `end` | No | Time, `end`, or `+DURATION` |
| `duration` | No | Time; a leading `+` is optional; not together with `end` |
| `name` | No | Clip name |
| `mode` | No | `fast` \| `accurate` (case-insensitive) |
| `output` | No | Output folder for this clip (relative → manifest's folder) |
| `upscale_preset` | No | Preset id, or `off` to turn upscaling off for the row (feature 002) |
| `upscale_reframe`, `upscale_tier`, `upscale_hdr`, `upscale_motion` | No | Same words as the JSON `upscale` object (case-insensitive) |
| `upscale_crf` | No | 0–51 |

- Without `end` and `duration`, a clip runs to the end of the video.
- Any non-empty `upscale_*` cell turns upscaling on for the row. A row with upscaling on and empty
  `start`, `end` and `duration` is the whole video; the `start` column may then be left out of the
  header. `upscale_preset=off` cannot be combined with other `upscale_*` values.
- Rows that share an `input` are grouped into one source, in order of first appearance.
- CSV cannot express container, name template, on-exists or stream selection. CLI flags supply
  those (`--container`, `--name`, `--on-exists`); stream selection is unavailable for CSV.
- Errors are reported as `<file>:<line>: <reason>`, all rows at once. Messages include
  `unknown column "x" (allowed: …)`, `column "x" appears more than once`,
  `missing required column "start"`, `input is empty`, `start: invalid time …`,
  `give either end or duration, not both`, `mode must be fast or accurate, not "slow"`, and
  `malformed row: …`.

**CSV export** from the desktop app resolves each clip's options (defaults < video < clip) and
writes `mode` and `output` on every row. When some row is upscaled, the six `upscale_*` columns are
added and filled in full (`off` for plain rows), so the file reads the same with any CLI defaults.
Other options are not written.

**Equivalence (FR-007):** a JSON and a CSV manifest with the same content produce the same jobs.
This was verified by running `rmedia batch jobs.json --dry-run --json` and the CSV equivalent;
both planned the same outputs, modes and spans.

## Name Templates

`plan/naming.rs`. The template renders a path relative to the output folder. A `/` (or `\`) in
the template creates sub-folders.

| Placeholder | Value |
|---|---|
| `{source}` | Source file name without extension |
| `{index}` | 1-based clip index within its source |
| `{index:0N}` | Index zero-padded to N digits, N = 1–9 (for example `{index:03}` → `007`) |
| `{name}` | Clip name (empty if none) |
| `{name-or-start}` | Name, or `{start}` when unnamed |
| `{name-or-range}` | Name, or `{start}_{end}` when unnamed |
| `{start}`, `{end}` | Requested start / resolved end as `HH-MM-SS.mmm` |
| `{range}` | `{start}_{end}` |
| `{ext}` | Output extension |

- Default: `{source}_{index:03}_{name-or-range}.{ext}`, for example
  `a_001_00-00-10.500_00-00-15.500.mp4` or `a_001_goal.mp4`.
- Multi-source default (more than one source with `-o`): `{source}/{source}_{index:03}_{name-or-range}.{ext}`.
- Errors: an unknown placeholder, an unclosed `{`, `{index:0}`, `{index:010}`, and templates that
  yield absolute paths or empty, `.` or `..` parts.
- Sanitizing:
  - Placeholder values cannot introduce `/` or `\` (replaced with `_`).
  - In each component, `< > : " / \ | ? *` and control characters become `_`.
  - Trailing dots and spaces and leading spaces are removed; an empty result becomes `_`.
  - Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM1`–`COM9`,
    `LPT1`–`LPT9`, `COM¹²³`, `LPT¹²³`) get a `_` prefix.
  - Components are cut to 240 bytes, keeping the extension.

## Device Preset Files (`presets/*.json`)

Unreleased (0.2.0-dev). `rmedia upscale` reads user presets from `*.json` files in the `presets`
folder of the config directory ([Configuration](../administration/configuration.md#file-locations)).
A user preset with the same `id` as a built-in replaces it. A file that fails validation is
skipped and reported (`warning: preset file … ignored: …`, and `problems` in
`--list-presets --json`). The format is the JSON Schema in
`specs/002-mobile-upscale/contracts/preset.schema.json` (schema version 1). Required keys:
`schema`, `id`, `version`, `name`, `devices`, `width`, `height`, `max_fps`, `video`, `audio`,
`est_bpp`. Optional keys include `description`, `hdr_capable` and `limits`.

```json
{
  "schema": 1, "id": "reddit-916", "version": 1, "name": "Reddit 9:16",
  "devices": ["Reddit full-screen player"],
  "width": 1080, "height": 1920, "max_fps": 60,
  "video": { "codec": "h264", "crf": 18, "preset": "medium", "profile": "high" },
  "audio": { "codec": "aac", "bitrate_k": 192, "copy_if_aac": true },
  "hdr_capable": false,
  "limits": { "max_bytes": 1073741824, "max_duration": 900 },
  "est_bpp": 0.12
}
```

This is the built-in `reddit-916` preset. `id` is lowercase letters, digits and hyphens (at most 40
characters). `video.codec` is `hevc` or `h264`. Manifest `upscale` fields
(`specs/002-mobile-upscale/contracts/manifest-upscale.md`) are planned (user story 4) and are not
accepted yet. The `{preset}` name-template placeholder is valid only for upscale jobs; the default
template for upscale is `{source}_{preset}.{ext}`.

## Option Layering

Each clip's options are resolved from the lowest to the highest priority:

```text
built-in defaults  <  CLI flags  <  manifest "defaults"  <  videos[].options  <  clips[].options
```

- Built-in defaults (`Options::default`): mode `fast`, output folder `<source folder>/clips`,
  template `{source}_{index:03}_{name-or-range}.{ext}`, container = source, on-exists `skip`,
  audio `all`, subtitles `all`.
- `rmedia batch` uses only the flags that were actually given as defaults; the manifest's own
  values win (unit test `cli_options_are_defaults_below_the_manifest`).
- For CSV, CLI flags are the defaults and each row may set `mode` and `output`.
- In the desktop app, **Settings** defaults sit where CLI flags do, below the queue's manifest
  `defaults` (`ops::queue_request`). A `.json` export writes the same merged `defaults`, so
  `rmedia batch` resolves identical options.
