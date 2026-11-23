# Budgeted Reddit Video Owner Check Results (SC-008)

Results of the [owner runbook](fit-owner-checks.md) for 0.5.0 (feature 005, `--fit`).

| Field | Result |
|---|---|
| Date | 2026-11-23 |
| Reported by | the owner |
| Result | **pass**: `reddit-video` verified with real Reddit uploads |
| Reddit's limits | Maximum file size 1 GB, maximum length 15 minutes |
| Where video is allowed | Not in 18+ (NSFW, adult) or private communities |
| Post GIFs (003, roadmap item 3) | 100 MB hard limit (reported the same day); `reddit-post` updated |

The owner reported the outcome, not the per-file lines of the runbook's results sheet (result
lines, byte counts, playback on web and app). Those fields are not recorded here.

## What Changed

- `crates/rmedia-core/src/fit/builtin/reddit-video.json`: `verified: true`, version 2. The cap
  stays `max_bytes` 1073741824 (1 GiB) and `max_duration` 900 s.
- The docs note that Reddit doesn't allow video posts in 18+ or private communities. rmedia
  can't know where a file will be posted, so this is documentation, not a check.

## Open

- Reddit states the cap as "1 GB". rmedia's built-in uses 1 GiB (1,073,741,824 bytes). A file
  between 1,000,000,000 and 1,073,741,824 bytes was not part of the reported test. If Reddit
  turns out to mean 10^9 bytes, lower `max_bytes` to 1000000000.
- `reddit-comment` GIF limits: still unverified (a comment GIF upload).

## Related

[Runbook](fit-owner-checks.md) · [0.5.0 notes](../releases/0.5.0.md) ·
[Validation](../../specs/005-budgeted-reddit-video/checklists/validation.md) ·
[Research 10](../../research/10-reddit-limits-2026-11-13.md)
