# Desktop IPC Contract (Tauri v2 commands)

Every command is a thin adapter over an `rmedia-core` function (Principle I): `src-tauri/src/commands.rs`
unwraps arguments and calls `src-tauri/src/ops.rs`, which is tested without a webview
(`tests/bridge.rs`). Types are generated into `apps/desktop/src/lib/bindings/` by
`cargo xtask gen-bindings` (ts-rs); CI fails if regeneration changes them. Streaming uses
`tauri::ipc::Channel<T>`.

**The queue is a v1 manifest** (contracts/manifest.schema.json): planning, export and import all
use the manifest parser/writer, so the app and `rmedia batch` produce identical files (SC-008,
tested).

## Path policy (security)
The page may only name files the user chose: native dialogs (run in Rust — the page has no fs,
shell or dialog permissions; capability `core:default` only), drag-and-drop (registered in Rust
before the page sees it), files/folders given on the command line, a picked folder and what
scanning it found, and the sources of a manifest the user opened. Exports may only write to the
path chosen in the Save dialog. Everything else is refused.

| Command | Args | Returns |
|---|---|---|
| `doctor` | — | `DoctorReport` |
| `take_opened` | — | paths given on the command line (once) |
| `pick_files` | `kind: "video" \| "cliplist" \| "manifest"` | chosen paths (now allowed) |
| `pick_folder` | — | chosen folder (now allowed) |
| `pick_save_path` | `defaultName` | chosen export path (the only allowed export target) |
| `probe` | `path` | `MediaInfo` |
| `scan` | `scanId, roots, options: ScanOptions, onEvent: Channel<ScanEvent>` | `ScanResult` |
| `cancel_scan` | `scanId` | — |
| `parse_time` | `text` | seconds |
| `parse_end` | `text` | seconds, `"end"` or `"+D"` |
| `load_cliplist` | `path, sources` | manifest |
| `load_manifest` | `path` | manifest (keeps its defaults; its sources become allowed) |
| `export_manifest` | `manifest, path` | — (`.json` as is, `.csv` with options folded per row) |
| `plan` | `manifest` | `Plan` (kept for `run`) |
| `run` | `runId, onEvent: Channel<Event>` | `RunSummary` |
| `cancel_run` | `runId, jobId?` | — |
| `preview_source` | `path` | `{info, direct_url?, time_offset}` |
| `ensure_proxy` | `path, codec: "h264" \| "vp8", onProgress: Channel<number>` | media URL |
| `frame_at` | `path, t, height` | JPEG bytes (`ipc::Response`) |
| `thumbnails` | `path, n` | `data:` URLs |
| `get_settings` / `set_settings` | `Settings` | |

## Media playback
`<video>` sources are `http://127.0.0.1:<port>/<token>/<id>` URLs from the in-process media server
(research.md §R8; WebKitGTK cannot play from the asset protocol, which is disabled). Clip time =
`currentTime − time_offset`. Fallback chain: direct → H.264 proxy → VP8 proxy → still frames.

Window close cancels all runs and scans (children are killed with their process group / Job Object).

## UI (FR-025)
Sources (add files/folder, drag-drop, scan progress + cancel, open manifest) · Player (direct/proxy/
still, thumbnail timeline, in/out with `I`/`O`, frame step `←`/`→`, add with `Enter`) · Queue
(editable start/end/name, per-source mode/audio track/subtitles, import clip list, export) · Run
panel (per-job and overall progress, ETA, cancel job/all, summary) · Settings (defaults, output
folder, collisions, naming, concurrency, cache, engine diagnostics).
