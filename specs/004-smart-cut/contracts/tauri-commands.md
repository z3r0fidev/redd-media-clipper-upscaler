# Tauri commands: Smart Cut

Extends `specs/001-video-clipping/contracts/tauri-commands.md` and the 002/003 additions. Every
command calls `rmedia-core`; the page holds no media logic (Principle I). Paths must be allowed
sources (`AppState::require_allowed`).

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `keyframes` | `path`, `request_id` | `KeyframeIndex` | Whole-file index (research R1), cached per source (key: canonical path, size, mtime; the 32 most recent indexes). Capped and marked `truncated`. A new request for the same path is served from the cache. A cancelled scan fails with "the keyframe index was cancelled" and is not cached |
| `cancel_keyframes` | `request_id` | — | |
| `smart_plan` | `path`, `span: RangeArg`, `options: PartialOptions` | `SmartPlanResult { summary: SmartSummary \| null, notices: Notice[] }` | The same core planning as `rmedia clip --mode smart --dry-run`, for one range, over the settings' defaults. A newer call cancels the older one (it fails with "cancelled: a newer smart plan replaced this one"). With `smart_fallback: false` a source that cannot be smart cut is an error. The notices carry the fallback message (`SMART_FALLBACK`) |

Existing commands:
- `plan` and `run` accept `mode: "smart"` and `smart_fallback` in the queue manifest.
- The run's results carry `JobMeasure::Clip.smart`.
- `doctor` returns `smart.available`.

UI (`apps/desktop/src/lib/components`):
- **Player.svelte**: keyframe ticks on the timeline. With Smart selected, the selected range is
  drawn as head / copy / tail bands, with the copied share and any fallback reason under the
  timeline.
- **Queue.svelte**: a mode label "Smart"; a per-video "Only smart (no fallback)" option and a
  per-clip "Smart" / "Smart only" pill.
- **RunPanel.svelte**: shows "Smart · 92% copied" or "Accurate (smart fallback: <reason>)".
- **Settings.svelte**: Smart as a default mode choice; the doctor line for smart cut.
