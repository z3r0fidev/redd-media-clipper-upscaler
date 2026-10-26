# rmedia Configuration

## Overview

This reference is for operators and support staff who configure rmedia or diagnose its environment.
It covers the desktop settings file, how rmedia selects its video engine (FFmpeg), the crash
journal, the preview cache, environment variables, and per-user file locations on Linux and
Windows.

rmedia has no server component, accounts, or network configuration. All configuration is per user
and local.

Sources: `crates/rmedia-core/src/settings.rs`, `tools.rs`, `exec/journal.rs`, `exec/mod.rs`,
`preview.rs`, `request.rs`, `apps/desktop/src-tauri/src/{lib.rs,ops.rs}`, and the `directories`
crate 6.0.0 path rules.

## Which Surface Reads What

| Setting source | CLI (`rmedia`) | Desktop app (`rmedia-desktop`) |
|---|---|---|
| `settings.json` | Read only for `gifsicle_path`, `gifski_path` and `models_dir` (through the core's `doctor::run`); every other field is ignored | Read at startup, written from **Settings** |
| `--ffmpeg` / `--ffprobe` flags | Yes | Not applicable |
| `--gifsicle` / `--gifski` flags | Yes | Not applicable |
| `RMEDIA_GIFSICLE` / `RMEDIA_GIFSKI` | Yes | Yes (when no engine path is set in settings) |
| `RMEDIA_FFMPEG` / `RMEDIA_FFPROBE` | Yes | Yes (when no engine path is set in settings) |
| `RMEDIA_STATE_DIR` | Yes | Yes |
| `RMEDIA_CONFIG_DIR` | Yes: device presets (`presets/`), GIF targets (`gif-targets/`) and the three `settings.json` fields above | Yes |
| `RMEDIA_CACHE_DIR` | Yes (cache root; used by upscale and models lookup) | Yes |

The CLI takes its clip, upscale and GIF options from flags and manifests only; it never reads
`defaults`, `jobs`, `ffmpeg_path` or `ffprobe_path` from `settings.json`. The core's engine check
(`crates/rmedia-core/src/doctor.rs`), which every CLI command runs, calls `Settings::load()` for the
GIF engine paths and the models folder only.

## Desktop Settings File (`settings.json`)

### Location

| OS | Path |
|---|---|
| Linux | `$XDG_CONFIG_HOME/rmedia/settings.json` (default `~/.config/rmedia/settings.json`) |
| Windows | `%APPDATA%\OpenLogic\rmedia\config\settings.json` |
| Any | `$RMEDIA_CONFIG_DIR/settings.json` when `RMEDIA_CONFIG_DIR` is set and not empty |

The file does not exist until you select **Save** in the desktop **Settings** dialog. rmedia writes
it atomically: it writes `settings.json.tmp` and then renames it over `settings.json`.

### Load behaviour

- A missing or unreadable file means all defaults are used.
- The file rejects unknown fields. **If the file contains any unknown field, or any value that fails
  to parse, the whole file is ignored silently and defaults are used.** No error is shown. The next
  **Save** from the desktop app then overwrites the file with the in-app values.

### Fields

All fields are optional. A file containing `{}` is equivalent to the defaults.

| Field | Type | Default | Description |
|---|---|---|---|
| `ffmpeg_path` | path string | not set | Explicit FFmpeg executable. Not set means the automatic order (see [Engine location order](#engine-location-order)). |
| `ffprobe_path` | path string | not set | Explicit ffprobe executable. Set together with `ffmpeg_path`. |
| `defaults` | object | `{}` | Defaults for new clips (see the next table). |
| `jobs` | integer (0 to 65535) | not set | Clips processed at once. Not set means half the CPU cores, between 1 and 4. The **Clips at once** field in the UI accepts 1 to 64. |
| `proxy_cache_max_mb` | integer | `5120` | Size cap of the preview cache in MB (multiplied by 1024 × 1024). The UI field has a minimum of 100. |
| `models_dir` | path string | not set | Folder searched first for model files (face detector, AI upscaler weights). Set in the desktop app with Settings › **Models folder** (a folder picker; the page cannot type a path). The AI tier uses a folder only when it holds all six Real-ESRGAN files (`realesr-animevideov3-x2/x3/x4` `.param` and `.bin`). |
| `gifsicle_path`, `gifski_path` | path string | not set | The optional GIF engines (feature 003). Read by the desktop app **and by the CLI** (after `--gifsicle`/`--gifski`, before `RMEDIA_GIFSICLE`/`RMEDIA_GIFSKI`). Set them in the desktop app with **Settings › GIF export › Choose gifsicle…/Choose gifski…** (a native file dialog that checks the program's `--version`); the page cannot type a path. |
| `gif_defaults` | object | `{}` | Last-used GIF choices (the JSON manifest `gif` keys). The desktop GIF view starts from these and saves each change here. |
| `upscale_defaults` | object | `{}` | Last-used upscale choices (`enabled`, `preset`, `reframe`, `tier`, `hdr`, …). The desktop upscale view starts from these and saves each change here; kept apart from `defaults` so clip runs never turn upscaling on. |

> **Unverified:** Setting `jobs` to `0` by editing the file is not rejected on load, and its effect
> was not tested. Use values from 1 to 64, the range the UI and CLI accept.

#### `defaults` object

Same fields as manifest options (`PartialOptions` in `crates/rmedia-core/src/request.rs`). Unknown
fields are rejected.

| Field | Values | Built-in default |
|---|---|---|
| `mode` | `"fast"` or `"accurate"` | `"fast"` |
| `output_dir` | path | not set: a `clips` folder next to each source |
| `name_template` | template string | `"{source}_{index:03}_{name-or-range}.{ext}"` |
| `container` | extension, for example `"mp4"` | not set: same as the source |
| `on_exists` | `"skip"`, `"overwrite"`, or `"rename"` | `"skip"` |
| `audio` | `"all"`, `"none"`, or an array of 1-based track numbers | `"all"` |
| `subtitles` | `"all"` or `"none"` | `"all"` |

Template placeholders (from `rmedia clip --help`): `{source}`, `{index}`, `{index:03}`, `{name}`,
`{name-or-range}`, `{name-or-start}`, `{start}`, `{end}`, `{range}`, `{ext}`.

### Example

```json
{
  "defaults": {
    "mode": "accurate",
    "output_dir": "/home/<YOUR_USERNAME>/Videos/clips",
    "on_exists": "rename"
  },
  "jobs": 2,
  "proxy_cache_max_mb": 2048
}
```

### Engine paths are set only through the native file dialog

The page (web UI) cannot set `ffmpeg_path` or `ffprobe_path`. When the desktop app saves settings
from the page, it keeps the current engine paths and discards any engine paths the page sent
(`AppState::set_settings` in `ops.rs`). Engine paths change only through **Settings** ›
**Video engine** › **Choose ffmpeg…**, which opens a native dialog in Rust. That flow:

1. Requires the chosen file name to end with `ffmpeg`.
2. Requires the matching `ffprobe` (same name with `ffmpeg` replaced by `ffprobe`) in the same
   folder.
3. Saves both paths.

**Use automatic** clears both paths.

You can also edit `settings.json` by hand while the app is closed.

### The default output folder is trusted across restarts

`defaults.output_dir` follows a similar rule. When the page saves settings, a changed output folder
is accepted only if the user already picked it with the native folder dialog (**Choose…**);
otherwise the save fails with `choose the output folder with Choose…`. An unchanged folder is
always accepted. At startup the app re-allows the saved folder (`AppState::new` in `ops.rs`), so
clips can be written there without picking it again. A folder written into `settings.json` by hand
is trusted the same way.

The desktop **Export…** to `.json` writes these `defaults` into the exported manifest (queue
values win), so `rmedia batch` on the export uses the same options even though the CLI never reads
`settings.json`.

## Engine Location Order

rmedia needs both `ffmpeg` and `ffprobe`. Each is located independently, in this order
(`crates/rmedia-core/src/tools.rs`, `find_tool`):

| Order | Source | Reported source | Notes |
|---|---|---|---|
| 1 | Explicit path: `--ffmpeg` / `--ffprobe` (CLI) or `ffmpeg_path` / `ffprobe_path` (desktop `settings.json`) | `config` | The file must exist, or the lookup fails (it does not fall through). |
| 2 | `RMEDIA_FFMPEG` / `RMEDIA_FFPROBE` environment variable (ignored when empty) | `config` | The file must exist, or the lookup fails. |
| 3 | Bundled: `rmedia-ffmpeg[.exe]` / `rmedia-ffprobe[.exe]` in the same directory as the running executable | `bundled` | |
| 4 | `ffmpeg` / `ffprobe` on `PATH` | `path` | |

Explicit paths are resolved to absolute paths. After locating the programs, rmedia runs
`ffmpeg -hide_banner -version` and checks the version:

- Release versions must be **6.0 or later** (`MIN_VERSION = (6, 0)`).
- Git or nightly builds that report no release number (for example `N-118000-g…`) are accepted as
  recent.
- A build that does not report an FFmpeg version is rejected.

The version check applies to `ffmpeg` only.

`rmedia doctor` (CLI) and **Settings** › **Video engine** (desktop) show the selected paths, their
source, the version, the availability of the key encoders (`libx264`, `aac`, `libvpx-vp9`,
`libopus`), and which containers accurate mode can write. If no accurate-mode encoders are
available, doctor reports `no encoders for accurate mode (fast mode still works)` and still returns
ready. The CLI `rmedia doctor` (since 0.2.0) also lists `libx265` and the upscaling filters;
the desktop **Video engine** section was not changed.

The optional GIF engines gifsicle and gifski (0.3.0) are found the same way, with their own
names: `--gifsicle`/`--gifski` or `gifsicle_path`/`gifski_path`, then `RMEDIA_GIFSICLE`/
`RMEDIA_GIFSKI`, then the bundled `rmedia-gifsicle`/`rmedia-gifski`, then `PATH` (only when no
bundled copy exists). Each must report version 1.x to `--version`. A missing or failing engine is
not an error; see [CLI Reference › GIF Engine Lookup Order](../api/cli-reference.md#gif-engine-lookup-order).

CI tests the minimum supported version against the distribution FFmpeg 6.1 on `ubuntu-24.04`
(`minimum-engine` job in `.github/workflows/ci.yml`).

## Crash Journal (State Directory)

Every run (CLI or desktop) records its in-progress temporary output files in a journal so that a
crash or kill never leaves partial outputs behind permanently (`exec/journal.rs`).

| OS | Journal directory |
|---|---|
| Linux | `$XDG_STATE_HOME/rmedia/inflight/` (default `~/.local/state/rmedia/inflight/`) |
| Windows | `%LOCALAPPDATA%\OpenLogic\rmedia\data\inflight\` (Windows has no state directory, so the local data directory is used) |
| Any | `$RMEDIA_STATE_DIR/inflight/` when `RMEDIA_STATE_DIR` is set and not empty |

How it works:

- Each run creates `inflight/<run_id>-<pid>.json` and holds an exclusive OS file lock on it while
  the run is alive. On Unix, the `inflight` directory is created with mode `0700`.
- Clips are written to a temporary file named `.<stem>.rmedia-<run_id>-<job>.part` in the output
  folder, and are renamed into place only after verification.
- At the start of every run, rmedia scans `inflight/` for journals that are no longer locked (their
  process died) and deletes the temporary files they list. It then deletes the journal itself.
- For safety, rmedia deletes a listed file only if all of the following are true: the path is
  absolute, the name exactly matches the `.part` pattern for that run, and the path is a regular
  file (not a symlink). Unlocked, empty journals younger than 60 seconds are left alone.
- If the journal cannot be opened, the run continues without crash protection. The
  `crash journal unavailable: …` warning is emitted through `tracing` only, so it is not visible
  (see [Logging](#logging)).

The same state folder holds `capability.json`, the machine capability record (GPU devices,
tier self-test speeds, tiers that failed since; see [`rmedia doctor`](../api/cli-reference.md#rmedia-doctor)),
and its `capability.lock`. It is rewritten when the hardware, driver, engine or app version
changes, or on `rmedia doctor --recheck`. Deleting it is safe: the next `upscale` or `doctor`
measures again (a few seconds).

Operational notes:

- Cleanup of a crashed run happens at the **next run**, not at startup of the app.
- You can delete the `inflight` directory when no rmedia process is running. Stale `.part` files in
  output folders then need manual removal.

## Preview Cache (Desktop App Only)

The desktop app stores browser-playable preview copies ("proxies") and thumbnails here
(`settings::cache_dir()`):

| OS | Cache directory |
|---|---|
| Linux | `$XDG_CACHE_HOME/rmedia/preview/` (default `~/.cache/rmedia/preview/`) |
| Windows | `%LOCALAPPDATA%\OpenLogic\rmedia\cache\preview\` |
| Fallback | `<system temp>/rmedia-preview` if no per-user cache directory can be determined |

`RMEDIA_CACHE_DIR` replaces the cache root (the folder that contains `preview/`); the preview
cache is then `$RMEDIA_CACHE_DIR/preview/`.

Behaviour (`crates/rmedia-core/src/preview.rs`):

- Proxies are 480p H.264/AAC MP4 or, when the webview has no H.264 decoder, VP8/Opus WebM. They are
  named `<key>-proxy.mp4` or `<key>-proxy.webm`.
- The cache key is derived from the source's canonical path, size, and modification time. Changing
  the source file creates a new cache entry.
- Size cap: `proxy_cache_max_mb` (default 5120 MB). After a proxy is created, least recently
  modified files are deleted until the cache fits the cap. The newly created proxy is never evicted.
  Reusing a cached proxy refreshes its timestamp.
- Clips are always cut from the original source, never from a proxy.

The cache is safe to delete while the app is closed.

## Environment Variables

| Variable | Read by | Effect | Default |
|---|---|---|---|
| `RMEDIA_FFMPEG` | `rmedia`, `rmedia-desktop`, test kit | Path to FFmpeg. Second in the [engine location order](#engine-location-order). In tests, the FFmpeg used to generate fixtures. | not set |
| `RMEDIA_FFPROBE` | `rmedia`, `rmedia-desktop`, test kit | Path to ffprobe, as above. | not set |
| `RMEDIA_SR` | `rmedia`, `rmedia-desktop` | Path to the AI worker `rmedia-sr`. Without it the worker is looked for next to the executable (and, in development builds, in the Cargo target folder). Added in 0.2.0. | not set |
| `RMEDIA_GIFSICLE`, `RMEDIA_GIFSKI` | `rmedia`, `rmedia-desktop`, test kit | Paths to the optional GIF engines, after the `--gifsicle`/`--gifski` flags and the settings, before the bundled copies and `PATH`. In tests, they enable the engine tests. Added in 0.3.0. | not set |
| `RMEDIA_REQUIRE_GIF_TOOLS` | test kit only | When `1`, the gifsicle and gifski tests fail instead of being skipped if the engines are missing. Set in the CI bundled-engine jobs. | not set |
| `RMEDIA_STATE_DIR` | `rmedia`, `rmedia-desktop` | Replaces the state directory (crash journal parent). Used by the CLI tests. | per-user state directory |
| `RMEDIA_CONFIG_DIR` | `rmedia`, `rmedia-desktop` | Folder that holds `settings.json`, the user device presets (`presets/*.json`, read by `rmedia upscale`) and GIF targets (`gif-targets/*.json`). Used by tests. | per-user config directory |
| `RMEDIA_CACHE_DIR` | `rmedia`, `rmedia-desktop` | Replaces the cache root, which holds `preview/`, `reframe/` (smart-reframe analyses) and `models/`. Used by tests. Added in 0.2.0. | per-user cache directory |
| `RMEDIA_REQUIRE_FFMPEG` | test kit only | When `1`, media tests fail instead of being skipped if FFmpeg is missing. Set in CI. | not set |
| `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` | Linux | Standard XDG base directories, honoured when they are absolute paths. | `~/.config`, `~/.local/state`, `~/.cache` |

Empty values of the `RMEDIA_*` path variables are treated as not set.

## File Locations

Per-user directories come from `directories::ProjectDirs::from("com", "OpenLogic", "rmedia")`. On
Linux the folder name is the lowercased application name (`rmedia`); on Windows it is
`OpenLogic\rmedia`.

| Purpose | Linux | Windows |
|---|---|---|
| Settings (`settings.json`) | `~/.config/rmedia/` | `%APPDATA%\OpenLogic\rmedia\config\` |
| Crash journal (`inflight/`) | `~/.local/state/rmedia/` | `%LOCALAPPDATA%\OpenLogic\rmedia\data\` |
| Preview cache (`preview/`) | `~/.cache/rmedia/` | `%LOCALAPPDATA%\OpenLogic\rmedia\cache\` |
| User device presets (`presets/*.json`) | `~/.config/rmedia/presets/` | `%APPDATA%\OpenLogic\rmedia\config\presets\` |
| User GIF targets (`gif-targets/*.json`, 0.3.0) | `~/.config/rmedia/gif-targets/` | `%APPDATA%\OpenLogic\rmedia\config\gif-targets\` |
| GIF output (default, 0.3.0) | `gifs/` next to each source video | `gifs\` next to each source video |
| Model files (searched in order) | `models_dir` setting, `models/` beside the executable, `/usr/lib/rmedia/models/` (deb; `usr/lib/rmedia/models/` inside the AppImage), `~/.cache/rmedia/models/` | same pattern; cache under `%LOCALAPPDATA%\OpenLogic\rmedia\cache\models\` |
| Upscaled output (default) | `upscaled/` next to each source video | `upscaled\` next to each source video |
| Program files (deb) | `/usr/bin/`, `/usr/lib/rmedia/licenses/` | See [Installation](installation.md) |
| Clip output (default) | `clips/` next to each source video | `clips\` next to each source video |

The Linux paths were confirmed on a development machine (`~/.local/state/rmedia/inflight` and
`~/.cache/rmedia/preview` exist after use). The Windows paths are derived from the `directories`
6.0.0 source (`src/win.rs`).

> **Unverified:** The Windows paths have not been observed on a Windows machine.

The `com.openlogic.rmedia` Tauri identifier (`tauri.conf.json`) is not used for these directories.

## Logging

rmedia writes no log files. The CLI prints human-readable output to standard output and diagnostics
to standard error. `-v` shows the FFmpeg commands. `--json` produces machine-readable NDJSON events
(or one document for `--dry-run`). The core emits `tracing` events (for example, the
`crash journal unavailable` warning), but no crate in the repository installs a `tracing`
subscriber, so these events are currently discarded and never shown.

## Backup

Only `settings.json` holds user configuration worth backing up. The crash journal and preview cache
are disposable.
