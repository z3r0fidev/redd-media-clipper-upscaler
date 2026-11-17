# Smart Cut Codes

Added in 0.4.0 (feature 004). These values are stable once released: scripts can match on them.
Source: `crates/rmedia-core/src/smart/mod.rs` (`FallbackReason`) and
`crates/rmedia-core/src/error.rs` (`IssueCode`).

## Issue Codes

| Code | Severity | When | Where it appears |
|---|---|---|---|
| `SMART_FALLBACK` | warning | A smart job runs in accurate mode instead, while planning or after a failed seam check | Plan `warnings` and `notices` (with `detail.reason`); `job_warning` events; the human `warning:` line; the desktop line under the timeline |
| `SMART_UNAVAILABLE` | error | Strict mode (`--no-smart-fallback`, `smart_fallback: false`, **Only smart**) and a fallback reason | Plan `issues` (exit 3, nothing runs) with `detail.reason`; or a `failed` outcome's `error` (exit 1) for `params_changed` and `seam_check` |
| `SMART_NOT_APPLIED` | warning (a note) | Smart was asked for a job that upscales or makes a GIF, which re-encodes every frame anyway | Plan `warnings` and `notices`; the plan's `smart.not_applied` count. The job succeeds as usual |

## Fallback Reasons

The first ten are found while planning, from the probe and the keyframe index, so a dry run
shows them. The last two are found only after the ends were encoded and joined; the job is then
redone in accurate mode.

| Code | Message (after `smart cut not possible: `) | Found |
|---|---|---|
| `encoder_missing` | the engine has no H.264 encoder (libx264) | planning |
| `codec` | the video is not H.264; smart cut handles H.264 only | planning |
| `container` | smart cut handles mp4, mov, mkv and ts files only | planning |
| `profile` | the video's H.264 profile is not Baseline, Main or High | planning |
| `pixel_format` | the video is not 8-bit 4:2:0 (for example 10-bit or 4:2:2) | planning |
| `interlaced` | the video is interlaced | planning |
| `vfr` | the video has a variable frame rate | planning |
| `rotation` | the video's rotation cannot be kept in the output | planning |
| `no_full_gop` | the range holds no complete group of pictures (it is too short) | planning |
| `open_gop` | the video has open GOPs (frames that depend on the previous GOP) | planning |
| `params_changed` | the video's size or pixel format changes at a cut point | after the join |
| `seam_check` | the joined clip failed the seam check | after the join, or when a head, copy, tail or join step fails |

What to do about each one: [Troubleshooting › Smart cut](../user/troubleshooting.md#smart-cut).
The JSON shapes: [JSON Output › Smart cut](../api/json-output.md#smart-cut).
