# Desktop IPC Contract (Tauri v2 commands)

Every command is a thin adapter over an `rmedia-core` function (Principle I). Types are generated to
`apps/desktop/src/lib/bindings/` via ts-rs; CI fails if regeneration produces a diff. Streaming uses
`tauri::ipc::Channel<T>`. Only plugin: `dialog`. No shell or fs plugin; the asset protocol scope is
the preview cache dir, plus individual files allowed at runtime for direct play.

| Command | Args | Returns | Core fn |
|---|---|---|---|
| `doctor` | — | `DoctorReport` | `doctor::run` |
| `probe` | `path` | `MediaInfo` | `probe::probe` |
| `scan` | `roots, opts, on_event: Channel<ScanEvent>` | `ScanId` | `discover::scan` |
| `cancel_scan` | `id` | — | token cancel |
| `parse_time` | `text` | `Ts` \| error | `time::parse` |
| `load_cliplist` | `path, sources[]` | `BatchRequest` \| `Issue[]` | `input::cliplist` |
| `load_manifest` | `path` | `BatchRequest` \| `Issue[]` | `input::manifest_*` |
| `export_manifest` | `req, path` | — | serialize JSON/CSV by extension |
| `plan` | `req` | `Plan` | `plan::plan` |
| `run` | `plan, jobs, on_event: Channel<Event>` | `RunId` | `exec::run` |
| `cancel_job` / `cancel_run` | `run, job?` | — | `RunHandle` |
| `preview_source` | `path` | `{direct_url?, info}` | `preview::source` |
| `ensure_proxy` | `path, on_progress: Channel` | `url` | `preview::proxy` |
| `thumbnails` | `path, n` | `url[]` | `preview::thumbnails` |
| `frame_at` | `path, t` | JPEG bytes (`ipc::Response`) | `preview::frame_at` |
| `get_settings` / `set_settings` | `Settings` | | `settings` |

Window close: cancel all runs/scans, wait ≤ 2 s for cleanup, then exit.

## UI views (FR-025)
Sources (file/folder pick, drag-drop, scan results table) · Player (video or still-frame, timeline,
thumbnail strip, I/O keys, frame step) · Queue (editable start/end/name rows, import clip list or
manifest, export manifest, shows predicted snap) · Run (per-job and overall progress, ETA,
cancel job/all, summary) · Settings & Diagnostics (doctor).
