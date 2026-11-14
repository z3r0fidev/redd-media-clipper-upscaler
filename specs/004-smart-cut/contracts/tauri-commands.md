# Tauri commands: Smart Cut

Extends `specs/001-video-clipping/contracts/tauri-commands.md` and the 002/003 additions. Every
command calls `rmedia-core`; the page holds no media logic (Principle I). Paths must be allowed
sources (`AppState::require_allowed`).

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `keyframes` | `path`, `request_id` | `KeyframeIndex` | Whole-file index (research R1), cached per source (key: path, size, mtime). Capped and marked `truncated`. A new request for the same path is served from the cache |
| `cancel_keyframes` | `request_id` | — | |
| `smart_plan` | `path`, `start: f64`, `end: EndArg`, `options: PartialOptions` | `SmartSummary` | The same core planning as `rmedia clip --mode smart --dry-run`, for one range. A newer call cancels the older one |

Existing commands:
- `plan` and `run` accept `mode: "smart"` and `smart_fallback` in the queue manifest.
- The run's results carry `JobMeasure::Clip.smart`.
- `doctor` returns `smart.available`.

UI (`apps/desktop/src/lib/components`):
- **Player.svelte**: keyframe ticks on the timeline. With Smart selected, the selected range is
  drawn as head / copy / tail bands, with the copied share and any fallback reason under the
  timeline.
- **Queue.svelte**: a mode label "Smart"; a per-clip and per-video "Only smart (no fallback)"
  option.
- **RunPanel.svelte**: shows "Smart · 92% copied" or "Accurate (smart fallback: <reason>)".
- **Settings.svelte**: Smart as a default mode choice; the doctor line for smart cut.
