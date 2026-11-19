# CLI contract: Budgeted Reddit Video

This extends `specs/001-video-clipping/contracts/cli.md` and 002/003's contracts. No existing flag
changes meaning, and without `--fit` every command behaves as before (FR-022).

## `rmedia clip`, `rmedia upscale` and `rmedia batch`

```text
--fit <TARGET>              Make each output an MP4 with audio that fits TARGET's size cap and
                            length limit (see --list-fit-targets). Re-encodes (accurate cut).
--fit-max-size <SIZE>       Lower the target's size cap for this run: 50MB, 8MiB, 1GB, or bytes.
--fit-max-length <DURATION> Lower the target's length limit (same syntax as -d).
--fit-allow-trim            Shorten a range that is too long, or that cannot fit the cap even at
                            the target's smallest size, instead of refusing it.
--list-fit-targets          (clip only) List the video targets and exit; with --json, as JSON.
```

- The `--fit-*` flags require `--fit` (exit 2 otherwise). `--fit` together with `--gif` is a
  usage error (exit 2). A size or length above the target's limit is refused (exit 2): an
  override may only lower a limit.
- `--fit` with `--container` other than `mp4` is refused (exit 3). Without `--container`, the
  output is `.mp4` whatever the source container is.
- **Modes:**
  - `--fit` with `--mode smart` prints the note "smart cut does not apply: the clip is
    re-encoded to fit its target" (`SMART_NOT_APPLIED`);
  - `--mode fast` (the default) is ignored without a note, as for upscale and GIF jobs;
  - in both cases the cut is accurate.
- **With `upscale` or `clip --upscale`:** the preset sets the shape, size and reframing, and the
  target sets the caps. The preset's own `limits` are not used (FR-015).
- **Default names** are unchanged (`{source}_{index:03}_{name-or-range}.mp4`). A name template
  can use `{target}`.

### Dry run (`--dry-run`), one line per fit job

```text
fit reddit-video (unverified): 1920x1080 30 fps · video 4,227 kb/s + audio 128 kb/s (budget) · 98.0 MB of 100 MB
fit reddit-video (unverified): 1280x720 quality · crf 20 ≤ 3,318 kb/s + 128 kb/s · ≤ 43.1 MB of 1 GB (a 100 s range)
fit sub-50mb: size lowered 1920x1080 → 1440x810 (budget below the minimum bitrate)
```

A refusal prints the issue instead:

```text
error: RANGE_TOO_LONG: the range (20:00) is longer than reddit-video allows (15:00); use --fit-allow-trim to keep the first 15:00
error: BUDGET_UNREACHABLE: 600 s cannot fit 8 MB even at 854x480 (needs at least 33.1 MB); raise --fit-max-size or use --fit-allow-trim
```

### Result line

```text
✓ talk_001_00-10-03-10.mp4  95.4 MB of 100 MB (95%) · 1920x1080 · 2 passes
✓ long_001_00-00-15-00.mp4  998 MB of 1 GB · trimmed from 20:00 to 15:00 · 2 passes, 1 retry
```

## Exit codes (the existing set)

| Situation | Code |
|---|---|
| Fit jobs that succeed, including lowered or trimmed ones | 0 |
| A run-time overshoot after 3 retries (`FIT_FAILED`) | 1 (job failed) |
| A misused flag (`--fit-*` without `--fit`, `--fit` with `--gif`, an override above the target) | 2 |
| `TARGET_UNKNOWN`, `RANGE_TOO_LONG`, `BUDGET_UNREACHABLE` (plan time), `--fit` with a non-mp4 container | 3 |
| No libx264 or no AAC encoder | 4 |
| Cancelled | 130 |

## `rmedia clip --list-fit-targets`

```text
reddit-video   Reddit video post   1 GB · 15:00 · 1920 px · unverified   built-in
sub-50mb       r/example rule      50 MB · 05:00 · 1920 px · unverified   user (/home/u/.config/rmedia/video-targets/sub-50mb.json)
```

With `--json`: `{"schema":1,"targets":[{...VideoTarget, "source":"builtin"|"user", "file":...}]}`.

## `rmedia doctor`

Adds the line `video targets: 2 (reddit-video, sub-50mb)`, and one line per problem file. The
JSON gains `"fit": {"targets": [ids], "problems": [{file, message}]}` (additive). Problems
never change `status` or the exit code.
