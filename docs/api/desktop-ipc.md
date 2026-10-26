# rmedia Desktop IPC Reference (Tauri 2)

This page documents the interface between the desktop app's Svelte page and its Rust backend:
Tauri commands, streaming channels, events, the path policy, and the media server URL scheme.

Code: `apps/desktop/src-tauri/src/{lib.rs,commands.rs,ops.rs,media_server.rs}`, the frontend
wrappers in `apps/desktop/src/lib/api.ts`, and generated types in `apps/desktop/src/lib/bindings/`.
Contract: `specs/001-video-clipping/contracts/tauri-commands.md`.

## Conventions

- Commands are called with `invoke(name, args)` from `@tauri-apps/api/core`. Argument names are
  camelCase on the JavaScript side (`scanId`, `onEvent`, `defaultName`), as Tauri converts them
  from the Rust snake_case parameters.
- Every command that can fail returns `Result<T, String>` (`OpResult`). A rejected promise
  carries a human-readable message string.
- Types named below (for example `MediaInfo` or `Plan`) are the ts-rs bindings generated from
  `rmedia-core` by `cargo xtask gen-bindings`. CI fails if regenerating them changes any file.
- Times are seconds (`number`).
- Every command is a thin adapter: `commands.rs` unwraps arguments and calls `ops.rs`, which calls
  `rmedia-core`. `ops.rs` is tested without a webview (`src-tauri/tests/bridge.rs`).

## Capabilities

`apps/desktop/src-tauri/capabilities/default.json` grants the `main` window only
`core:event:allow-listen` and `core:event:allow-unlisten`. The page has no filesystem, shell or
dialog permissions. App commands and channels need no core permissions, and dialogs run in Rust
through `tauri-plugin-dialog`. `withGlobalTauri` is `false` and the asset protocol is disabled
(`tauri.conf.json`).

## Path Policy

The page can name only files and folders the user chose (`ops::Allowed`). Each command that takes
a path checks it against this set first. A path is matched after canonicalization, and the
canonical path is what the command then uses.

| Granted by | Grants |
|---|---|
| `pick_files` with `kind: "video"` | Those files |
| `pick_files` with `kind: "cliplist"` or `"manifest"` | Permission to parse those files as lists (not to read them as media) |
| `pick_folder` | The folder and everything under it |
| Drag-and-drop onto the window | Dropped files and folders (registered in Rust before the page is notified) |
| Command line (`rmedia-desktop FILE|FOLDER…`) | Those paths |
| `load_manifest` | The manifest's sources that have a video extension and exist as files |
| `pick_save_path` | That exact path, as an export target only |
| App start (`AppState::new`) | The default output folder saved in Settings (`defaults.output_dir`), and everything under it. `set_settings` only accepts a new folder that is already allowed, so this re-grants a folder the user picked earlier. |

Further rules:

- Planned clip outputs must be inside a picked folder or under the source's own folder (for
  example the default `clips/` sub-folder). `plan` adds an `INVALID_INPUT` issue for any other
  output: `<output> is outside the folders you chose; pick the output folder in Settings`.
- A path that is not allowed is refused with:
  `<path> was not opened in rmedia (use Add files/folder or drag it in)`.

## Commands

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `doctor` | — | `DoctorReport` | Re-locates the engine and caches it |
| `take_opened` | — | `string[]` | Paths given on the command line; returns them once, then empty |
| `pick_files` | `kind: "video" \| "cliplist" \| "manifest"` | `string[]` | Native multi-file dialog. Filters: video extensions / `txt` / `json`+`csv`. |
| `pick_folder` | — | `string \| null` | Native folder dialog |
| `pick_save_path` | `defaultName: string` | `string \| null` | Native save dialog (filter `json`, `csv`) |
| `probe` | `path` | `MediaInfo` | Requires an allowed path |
| `scan` | `scanId: string`, `roots: string[]`, `options: ScanOptions`, `onEvent: Channel<ScanEvent>` | `ScanResult` | Roots must be allowed. Rejected with `"cancelled"` when cancelled. |
| `cancel_scan` | `scanId` | — | |
| `parse_time` | `text` | `number` | Core time grammar; positions only |
| `parse_end` | `text` | `number \| string` | Position → number; `"end"`; duration → `"+<seconds>"` |
| `load_cliplist` | `path`, `sources: string[]` | `Manifest` | The list must have been picked as `cliplist`, and the sources must be allowed. Applies the list to every source. |
| `load_manifest` | `path` | `Manifest` | Must have been picked as `manifest`; `.json` or `.csv`. Keeps the manifest's own `defaults`. Its video files become allowed. |
| `export_manifest` | `manifest: Manifest`, `path` | — | `path` must come from `pick_save_path`. `.csv` → CSV with options folded per row. Otherwise JSON: the Settings `defaults` are folded into the manifest's `defaults`, with the queue's own values winning (`ops::with_settings_defaults`), so `rmedia batch` reproduces the app's run. |
| `plan` | `manifest: Manifest` | `Plan` | Validates and plans the queue. The plan is stored under `plan.run_id` for `run`. |
| `run` | `runId`, `onEvent: Channel<Event>` | `RunSummary` | Runs the stored plan once (it is removed). Refused if the plan has errors. |
| `cancel_run` | `runId`, `jobId: number \| null` | — | Cancels one job, or the whole run when `jobId` is null |
| `preview_source` | `path` | `PreviewInfo` `{info: MediaInfo, direct_url: string \| null, time_offset: number}` | Registers the source with the media server when the webview can probably play it directly |
| `ensure_proxy` | `path`, `codec: "h264" \| "vp8"`, `onProgress: Channel<number>` | `string` (media URL) | Encodes or reuses a cached 480p proxy; progress 0–1 |
| `frame_at` | `path`, `t: number`, `height: number` | JPEG bytes (`ArrayBuffer`, via `ipc::Response`) | `height` clamped to 32–1080; `t` is clip time |
| `thumbnails` | `path`, `n: number` | `string[]` (`data:image/jpeg;base64,…`) | `n` clamped to 1–60; cached |
| `pick_engine` | `reset: boolean`, `kind?: "ffmpeg" \| "gifsicle" \| "gifski"` | `DoctorReport` | `reset: false` opens a native picker for that program (ffmpeg: ffprobe must sit next to it; gifsicle/gifski: a regular file, not a link, that identifies itself as version 1.x). `reset: true` returns to automatic lookup. Saved to settings. |
| `list_gif_targets` | — | `{default, targets: GifTarget[], problems}` | Built-in and user GIF targets (0.3.0). |
| `gif_estimate` | `estimateId` (1–64 chars), `path`, `span`, `options: GifOptions`, `onEvent: Channel<AnalysisEvent>` | `{estimate, notes, notices}` | Requires an allowed path. The predicted size of the GIF job (the budget search from samples, nothing encoded in full; the `estimate` object of the plan JSON), in a private folder under the app cache (`gif-estimates/`, removed on every path and at startup). One estimate at a time: a new one cancels the others; refused while a run encodes, and a run that starts cancels it. The core re-checks every option. |
| `cancel_estimate` | `estimateId` | — | |
| `open_output` | `runId`, `jobId` | `{url, kind: "image" \| "video", bytes, width, height}` | Registers a committed output of one of this app's last 16 runs with the media server. The page never sends a path; a file that is no longer the one the run wrote (a link swapped in) is refused. At most 48 output URLs stay live. |
| `doctor_recheck` | — | `DoctorReport` | Runs the machine check (tier self-tests) again; one at a time |
| `list_presets` | — | `{default, presets: Preset[], problems: {file, message}[]}` | Built-in and user presets (002 US5) |
| `analyze_reframe` | `analysisId` (1–64 chars), `path`, `span: {start, end\|null} \| null`, `options: UpscaleOptions`, `onEvent: Channel<AnalysisEvent>` | `{analysis: ReframeAnalysis \| null, note: string \| null}` | Requires an allowed path. Smart-reframe analysis for the crop overlay, through the same cache as the render and the CLI; runs with the CPU tier. A new analysis cancels the running one. `analysis` is null when there is nothing to analyse; `note` says why. |
| `cancel_analysis` | `analysisId` | — | |
| `upscale_preview` | `path`, `span`, `at: number`, `options: UpscaleOptions` | `{before, after, size, approximate, crop_x, in_fallback, classical}` | Requires an allowed path. Two PNG stills at the preset size (`classical`: AI tier, the "after" still uses the standard scaler), served by the media server (`image/png`); written under the app cache (`upscale-preview/`, newest 48 kept). A new preview cancels the running one. Times must be finite and 0–10,000,000 s. |
| `pick_models_dir` | `reset: boolean` | `Settings` | Native folder picker for the models folder (or back to the default search). Reloads the engine capabilities. |
| `get_settings` | — | `Settings` | |
| `set_settings` | `settings: Settings` | — | Saved atomically. `ffmpeg_path` / `ffprobe_path`, `gifsicle_path` / `gifski_path` and `models_dir` from the page are **ignored**; only `pick_engine` and `pick_models_dir` change them. `jobs` is clamped to 1–64. A changed `defaults.output_dir` must already be allowed (picked with `pick_folder`), otherwise: `choose the output folder with Choose…`. |

### The Queue Is a v1 Manifest

The page keeps its queue as a `Manifest` (`lib/manifest.ts`), the same document `rmedia batch`
runs ([File Formats › JSON manifest](file-formats.md#json-manifest-v1)). `plan` parses it with
`manifest_json::from_value`, with **Settings** defaults layered below the manifest's `defaults`.
Exporting the queue and running `rmedia batch` on the result produces byte-identical clips
(SC-008, `tests/bridge.rs::gui_and_cli_outputs_are_identical`).

### Settings

`Settings` (`crates/rmedia-core/src/settings.rs`, stored as `settings.json` in the per-user
config folder):

| Field | Type | Default |
|---|---|---|
| `ffmpeg_path`, `ffprobe_path` | string (optional) | unset (`RMEDIA_FFMPEG`/`RMEDIA_FFPROBE`, then bundled, then `PATH`) |
| `defaults` | `PartialOptions` | `{}` |
| `jobs` | number (optional) | unset (half the CPU cores, 1–4) |
| `proxy_cache_max_mb` | number | `5120` |
| `models_dir` | string (optional) | unset (`models/` next to rmedia, then `<cache>/models`); set only by `pick_models_dir` |
| `upscale_defaults` | `PartialUpscale` | `{}` (the upscale view's last choices; never turns upscaling on for clips) |
| `gifsicle_path`, `gifski_path` | string (optional) | unset (`RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI`, then bundled, then `PATH`); set only by `pick_engine` with `kind` `gifsicle`/`gifski` (0.3.0) |
| `gif_defaults` | `PartialGif` | `{}` (the GIF view's last choices, 0.3.0) |

Unknown fields are rejected.

## Channels and Events

Streaming uses `tauri::ipc::Channel<T>`:

| Channel | Payload type | Variants (`type` tag) |
|---|---|---|
| `scan` → `onEvent` | `ScanEvent` | `progress {visited, candidates, probed}`, `found {video: MediaInfo}`, `rejected {rejected: {path, reason, detail}}` |
| `run` → `onEvent` | `Event` | `run_started`, `job_started`, `job_progress`, `job_warning`, `job_stage`, `job_finished`, `run_progress`, `run_finished {summary}`. Field shapes match the [CLI NDJSON events](json-output.md#run-events-ndjson), but without `schema`, and the last event is `run_finished` (the CLI renames it `summary`). |
| `ensure_proxy` → `onProgress` | `number` | 0–1 |
| `analyze_reframe` → `onEvent` | `AnalysisEvent` | `progress {fraction}` (at most one per percent), then `done` |
| `gif_estimate` → `onEvent` | `AnalysisEvent` | `progress {fraction}`, then `done` (0.3.0) |

App event (Tauri event system, which is why the `listen` capability is granted):

| Event | Payload | Emitted when |
|---|---|---|
| `rmedia://dropped` | `string[]` | Files or folders were dropped on the window; already allowed |

**Window close** (`CloseRequested`) cancels every run and scan, which kills their child processes,
and revokes every media URL (`AppState::shutdown`).

## Media Server URL Scheme

The `<video>` element loads from an in-process HTTP server (`media_server.rs`):

```text
http://127.0.0.1:<port>/<token>/<id>
```

| Part | Value |
|---|---|
| `<port>` | Ephemeral, bound on `127.0.0.1` only, chosen at app start |
| `<token>` | 32 hex characters (128 bits from the OS RNG), one per app launch, compared in constant time |
| `<id>` | 32 hex characters, random per registered file. Re-registering the same canonical path returns the same URL. |

URLs are produced only by `preview_source` (`direct_url`) and `ensure_proxy`. They never contain
file paths.

| Request | Response |
|---|---|
| `Host` not exactly `127.0.0.1:<port>` | `403` |
| `Origin` present and not `tauri://localhost`, `http://tauri.localhost` or `https://tauri.localhost` (plus `http://localhost:1420` in debug builds) | `403` |
| Method other than `GET`/`HEAD` | `405` with `Allow: GET, HEAD` |
| Wrong path shape, wrong token, unknown id, file changed to a non-canonical path or symlink, not a regular file | `404` |
| No `Range` | `200` with the whole file |
| One valid `bytes=` range (`a-b`, `a-`, `-n`; the end is clamped) | `206` with `Content-Range` |
| Multiple ranges, malformed, or unsatisfiable | `416` with `Content-Range: bytes */<len>` |

Every response sets `Connection: close`, `Cache-Control: no-store` and
`X-Content-Type-Options: nosniff`. Successful responses add `Accept-Ranges: bytes`,
`Cross-Origin-Resource-Policy: cross-origin`, and a `Content-Type` from a fixed map: `video/mp4`,
`video/webm`, `video/quicktime`, `video/x-matroska`, `video/ogg`, else
`application/octet-stream`.

The page's Content Security Policy allows `media-src http://127.0.0.1:*`, and `connect-src` only
for IPC.

### Playback Time Mapping

`clip time = video.currentTime − time_offset`, where `time_offset` = first video frame − container
start (`preview::time_offset`). Proxies keep source timestamps, so the same mapping applies to
them.

**Fallback chain** (`lib/components/Player.svelte`): `direct_url`, if present → `ensure_proxy`
with `"h264"` → `ensure_proxy` with `"vp8"` → still frames from `frame_at`. Direct play is offered
only for MP4/M4V/MOV with H.264 video and AAC audio, or WebM with VP8/VP9/AV1 video and
Opus/Vorbis audio (`ops::direct_playable`).

## Keyboard (Player)

From `Player.svelte` › `onKey`. Keys are ignored while focus is in an input, select or textarea.

| Key | Action |
|---|---|
| `Space` | Play / pause |
| `I` / `O` | Set in / out at the current time |
| `Enter` or `A` | Add the marked clip to the queue |
| `←` / `→` (also `,` / `.`) | Previous / next frame; with `Shift`, 1 second |
| `Home` / `End` | Jump to start / end |
