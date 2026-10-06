# 04 - App stack and packaging (CLI + desktop GUI, Linux + Windows)

Date: 2026-10-06. Project: /home/z3r0d3v/projects/reddit_media
Provenance legend: [S] = verified by web search this session (source URL given); [K] = background knowledge (training data, not re-verified today; re-check before committing).

## 0. Requirements recap
- One shared core library; CLI and native GUI are thin front ends.
- Core = orchestrating external processes (ffmpeg/ffprobe, realesrgan-ncnn-vulkan, optionally ONNX Runtime). Workload is I/O + child process management, NOT CPU-heavy in-process. So language performance is irrelevant; process control, packaging, and GUI quality dominate.
- Targets: Linux + Windows. Dev toolchains present: Rust 1.94, Go 1.22, Python 3.12/uv, Node 24; Tauri v2 skill available.

## 1. Candidate stacks

### 1A. Rust core + clap CLI + Tauri v2 GUI (web frontend)
- Binary size [K]: Tauri app ~3-10 MB (uses system webview) + sidecars. CLI-only Rust binary ~1-4 MB stripped (LTO, opt-level=s).
- Startup [K]: fast native shell; webview init ~100-300 ms.
- Native feel [K]: web UI styled; not native widgets, but fine for a job-queue/form app. Native dialogs, tray, menus via Tauri plugins.
- Windows: requires WebView2 runtime (preinstalled on Win11 and most updated Win10). Installer can bootstrap (downloadBootstrapper / embedBootstrapper / offline installer) [K].
- Linux: requires webkit2gtk-4.1 + libayatana-appindicator; .deb/.rpm declare these deps. AppImage bundles libs but has known issues (GPU/Wayland/EGL rendering glitches, glibc floor = build host, bundle ~70+ MB) [K]; build on oldest supported Ubuntu (22.04/24.04). Flatpak: webkit via GNOME runtime works well [K].
- Packaging [K]: Tauri bundler -> NSIS (.exe) and MSI (WiX) on Windows; AppImage, .deb, .rpm on Linux; Flatpak via separate manifest (not built in).
- Cross-compile Windows from Linux [K]: possible via cargo-xwin with NSIS target only (MSI/WiX needs Windows); Tauri docs flag it as experimental. Prefer native Windows runner in CI.
- CI [K]: official tauri-apps/tauri-action; matrix ubuntu-22.04/24.04, windows-latest (macOS optional). Produces release artifacts + updater JSON.
- Signing: Windows Authenticode (certificate or Azure Artifact Signing, see section 7); Tauri updater has its own mandatory minisign-style signature keypair (separate from OS signing) [K].
- Auto-update: tauri-plugin-updater. Latest updater 2.10.1 (2026-04-04); 2.10.0 (2026-02-03) supports all bundle types: Deb, Rpm, AppImage on Linux; NSIS, MSI on Windows [S: https://v2.tauri.app/release/updater/]. Note: deb/rpm updating in place needs privileges; AppImage and NSIS are the smoothest self-update paths.
- Sidecars: tauri-plugin-shell. Shell plugin 2.3.5 (2026-02-03) fixed sidecars in tests, resolves from "deps" dir; 2.3.1 (2025-08-25) fixed sidecar names with dots on Windows [S: https://tauri.app/release/shell/all-versions/]. Watch the dotted-name issue (e.g. avoid naming binaries with dots).
- Licensing [K]: Tauri MIT/Apache-2.0; clap MIT/Apache; fully permissive.
- Pros: shared Rust core between CLI and GUI with zero IPC overhead; tiny; strong ecosystem (tokio::process, serde, tracing); the team has a Tauri skill.
- Cons: web frontend adds a second language/toolchain (Node) and webkit2gtk variance on Linux.

### 1B. Rust core + pure-Rust/native toolkits
- egui/eframe [K]: immediate mode, single binary ~5-15 MB, no webview, no system deps beyond GL/Vulkan (wgpu/glow); non-native look, weaker accessibility/IME; fastest to prototype; no packaging tooling (use cargo-packager / cargo-dist / cargo-bundle / cargo-deb / cargo-wix / NSIS manually).
- Slint [K]: declarative .slint DSL, small footprint, good native-ish look (Fluent/Material/Cupertino styles); license is GPLv3 / royalty-free desktop license (with attribution) / commercial: check terms for closed-source distribution.
- Iced [K]: Elm-style; MIT; still pre-1.0, API churn; text input/IME rough edges.
- gtk4-rs [K]: best native feel on Linux GNOME, poor on Windows (needs bundling GTK runtime ~50-100+ MB, awkward); not recommended for cross-platform.
- Verdict: viable for a simple GUI with zero web dependencies, and eliminates WebView2/webkit2gtk concerns, but yields rougher UX (lists, drag-and-drop, progress, rich forms) and hand-rolled packaging/updating.

### 2. Go core + cobra CLI + Wails or Fyne
- Wails v2 is the current stable; v3 is still beta (v3.0.0-beta.x series; desktop API said stable but prerelease) [S: https://newreleases.io/project/github/wailsapp/wails/release/v3.0.0-beta.0 ; https://github.com/wailsapp/wails/releases]. Re-check release page for a v3 stable before deciding.
- Same webview model as Tauri: WebView2 on Windows, webkit2gtk on Linux (same Linux caveats) [K]. Binary ~8-15 MB [K]. Packaging: NSIS via wails build -nsis; Linux packaging mostly DIY (nfpm, appimagetool) [K]. No built-in updater in v2 (community libs/go-selfupdate) [K].
- Fyne [K]: pure Go, OpenGL, cgo required (needs C toolchain; cross-compile via fyne-cross/Docker or zig); binaries ~15-25 MB; non-native look; BSD-3 license. fyne package produces Windows exe and Linux tar/Flatpak-ish.
- Go strengths: trivial cross-compile for CLI (CGO off), excellent os/exec + context cancellation; Go 1.22 on dev machine is old-ish (1.25/1.26 likely current as of 2026 [K]).
- Cons: GUI story weaker/less mature than Tauri; v3 beta churn; team has Tauri skill, not Wails.

### 3. Python core + Typer CLI + PySide6 (Qt)
- Binary size [K]: PyInstaller/Nuitka bundles of PySide6 are 60-200+ MB (Qt DLLs); Nuitka onefile smaller/faster startup but long compiles.
- Startup [K]: PyInstaller onefile cold start 1-5 s (unpacks to temp); onedir faster. 
- Native feel [K]: Qt Widgets is the most native-feeling and feature-rich option on both OSes (mature, accessibility, DnD, model/view). Qt for Python is LGPLv3 (dynamic linking OK; PyInstaller onedir keeps compliance easier; Nuitka onefile static concerns need care) or commercial.
- No WebView2/webkit2gtk dependency (Qt bundled); Linux needs xcb/wayland platform libs (libxcb-cursor0 for Qt 6.5+) [K].
- Packaging [K]: Briefcase (BeeWare) -> MSI (WiX) on Windows, AppImage/deb/rpm-ish/Flatpak on Linux; PyInstaller + Inno/NSIS/appimagetool/nfpm; Nuitka similar. Cross-compile not realistic: build on native runners (matrix).
- Auto-update: none built in (PyUpdater is dead; use custom or Sparkle-like DIY).
- Cons: heavy distribution, AV false positives with PyInstaller on Windows are common [K], two distribution surfaces (CLI vs GUI). Pros: fastest iteration; uv dev experience; easy to wrap ONNX Runtime in-process (python onnxruntime).

### Comparison matrix (subjective [K])
| Criterion | Rust+Tauri | Rust+egui/Slint | Go+Wails | Go+Fyne | Python+PySide6 |
|---|---|---|---|---|---|
| Installed size | ~5-15 MB (+sidecars) | ~5-20 MB | ~10-20 MB | ~15-30 MB | 80-250 MB |
| Native feel | web (good w/ care) | custom-drawn | web | custom | best (Qt) |
| Runtime deps | WebView2 / webkit2gtk | none | WebView2 / webkit2gtk | GL | none (bundled) |
| Installers built-in | NSIS, MSI, deb, rpm, AppImage | DIY | NSIS only-ish | basic | Briefcase partial |
| Auto-update | official plugin | DIY | DIY | DIY | DIY |
| CLI/GUI core sharing | direct crate | direct crate | direct pkg | direct pkg | direct pkg |
| Maturity (2026) | high | medium | v2 ok / v3 beta | medium | high |

## 4. Bundling external binaries (ffmpeg, ffprobe, realesrgan-ncnn-vulkan + models)

### Mechanisms (Tauri v2) [K unless noted]
- bundle.externalBin in tauri.conf.json: list base names (e.g. "binaries/ffmpeg"); each needs a file suffixed with the Rust target triple (ffmpeg-x86_64-unknown-linux-gnu, ffmpeg-x86_64-pc-windows-msvc.exe). Tauri strips suffix at bundle time and places next to the app exe. Spawn via app.shell().sidecar("ffmpeg") (needs shell:allow-execute capability scoped to sidecar; args validated). Prefer using tokio::process directly in the core crate with a resolved path, to keep the core GUI-agnostic, and use the sidecar only for path resolution (current_exe dir).
- bundle.resources for non-executable data: realesrgan models (.param/.bin pairs), resolved with app.path().resource_dir(). realesrgan-ncnn-vulkan finds models via -m <dir>; pass explicit path.
- The core crate should accept a ToolPaths config (ffmpeg, ffprobe, realesrgan, models_dir) with resolution order: explicit CLI flag/config -> bundled next to executable -> app data dir (downloaded) -> PATH (which crate). CLI release should work with PATH by default; GUI bundles.
- Linux AppImage: sidecars land in usr/bin inside the mount; they must be static or have bundled libs. Use static ffmpeg builds.
- realesrgan-ncnn-vulkan needs a Vulkan loader + GPU driver on host (libvulkan1 on Linux; ships vulkan-1.dll via drivers on Windows). Detect failure (no Vulkan device) and fall back to ffmpeg scale filters (lanczos/spline/ewa_lanczos, or libplacebo/ vulkan filters if available) [K]. Intel/AMD/NVIDIA; CPU fallback via -g -1 is very slow [K].
- ONNX Runtime route: Rust crate `ort` dynamic-loads onnxruntime shared lib (~10-20 MB CPU; GPU EPs hundreds of MB: CUDA/DirectML). Treat as optional plugin download, not bundled. DirectML covers any GPU on Windows [K]; Linux needs CUDA/ROCm/OpenVINO variants, fragmented. The ncnn-vulkan binary is the simpler cross-vendor path.

### Bundle vs first-run download
| Option | Pros | Cons |
|---|---|---|
| Bundle ffmpeg+ffprobe | works offline, deterministic version | +80-130 MB per OS (static GPL build ~ 70-130 MB each tool; "essentials" builds ~ 90 MB), licensing duty |
| Bundle realesrgan + models | works offline | +~30-60 MB (binary+models; realesr-animevideov3 / x4plus models ~17-64 MB each) |
| First-run download w/ checksum | tiny installer, can update tools independently, keeps GPL artifacts off your installer | needs network, mirror hosting, hash/signature pinning, offline failure UX |
| System PATH only | zero size | support burden, version skew |
Recommended hybrid: ship installer WITHOUT AI models; bundle (or download on first run) ffmpeg+ffprobe; AI upscaler = optional "component manager" downloading to app data dir with pinned SHA-256. For Linux deb/rpm, declare dependency on distro `ffmpeg` instead of bundling. For Flatpak, ffmpeg is in the freedesktop runtime (ffmpeg-full extension) [K].
- Windows/Linux static ffmpeg sources [K]: BtbN/FFmpeg-Builds (GPL and LGPL variants, win64/linux64, GitHub releases), gyan.dev (Windows), johnvansickle (Linux static). Verify current URLs/versions before pinning.

### Licensing [K - not legal advice]
- FFmpeg is LGPL-2.1+ by default; builds with --enable-gpl (libx264, libx265) or --enable-nonfree (fdk-aac) change terms: GPL builds are GPL-2.0+ (GPLv3 if --enable-version3); nonfree builds are not redistributable.
- Redistributing a GPL ffmpeg binary as a separate executable invoked via process spawn does not make your app GPL (aggregate / "arms-length" use), but you must: ship license text, provide corresponding source offer (or link to exact source tarball/build scripts), and not statically link it into your binary. Do NOT link libav* into your own code if the app stays non-GPL (another reason to shell out).
- LGPL-only ffmpeg builds avoid GPL duties but lack libx264/x265 (use hardware encoders / libopenh264 / SVT-AV1 (BSD) / libvpx).
- Patent caveat: H.264/HEVC patent licensing is a separate issue from copyright; commercial distribution may need counsel.
- realesrgan-ncnn-vulkan: MIT (code) [K]; model weights have their own terms (Real-ESRGAN BSD-3; verify per model, some anime models have different attributions). ncnn BSD-3. Include NOTICE file; consider cargo-about / cargo-deny for Rust deps.

## 5. Long-running job management (GUI)

Architecture (Rust/tokio) [K]:
- Core exposes `JobSpec` (serde) -> `JobHandle` with `events: mpsc/broadcast::Receiver<JobEvent>` and `cancel: CancellationToken` (tokio-util). Events: Queued, Started, Progress{pct, fps, speed, eta, out_time}, Log(line), Finished{outputs}, Failed{err}, Cancelled. CLI renders events with indicatif; GUI forwards them to the frontend.
- Scheduler: a `JobQueue` actor (single tokio task owning state) with `Semaphore` limiting concurrency (default: 1 for GPU upscaling; 2-3 for stream-copy clipping; configurable). Persist queue state (SQLite via rusqlite/sqlx, or JSON) for resume after crash. Batch manifests expand into N jobs sharing a parent id.
- Progress parsing: run ffmpeg with `-progress pipe:1 -nostats -loglevel error` (key=value blocks: out_time_us, fps, speed, progress=continue|end); compute pct from ffprobe duration (clip length = end - start for -ss/-to cuts). realesrgan-ncnn-vulkan prints percentages to stderr (parse `NN.NN%`); frame-based pipeline (extract frames -> upscale dir -> remux) gives per-frame progress by counting output files. Prefer ffmpeg pipe-based pipeline where possible to avoid huge temp dirs; temp dir cleanup on cancel.
- Streaming to UI (Tauri v2): use `tauri::ipc::Channel<JobEvent>` passed in a command for ordered, high-throughput streaming (recommended over global emit for progress); throttle progress to ~4-10 Hz. Use `app.emit` for coarse state changes (queue updated). Commands: enqueue_jobs, cancel_job, list_jobs, retry_job.
- Cancellation: send graceful `q` on ffmpeg stdin first (lets it finalize files), wait ~2 s, then hard kill; delete partial outputs (write to `*.part` then atomic rename on success).
- Windows process trees: `Child::kill` only kills the direct process; use a Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE so children die if the app crashes/exits (crates: `win32job`, `windows-sys`, or `command-group` / `process-wrap` `JobObject` wrapper). Use CREATE_NO_WINDOW (0x08000000) flag to avoid console flashes in the GUI. Graceful alternative: GenerateConsoleCtrlEvent needs CREATE_NEW_PROCESS_GROUP; ffmpeg 'q' via stdin is simpler.
- Unix: spawn child in its own process group (`process_group(0)`/setsid, `command-group`/`process-wrap`) and `killpg` SIGTERM then SIGKILL; set PR_SET_PDEATHSIG on Linux so children die with parent.
- Pause/resume: SIGSTOP/SIGCONT on Unix; NtSuspendProcess on Windows (optional, low priority).
- CLI: Ctrl-C handler (tokio::signal) triggers same CancellationToken path; exit codes: 0 ok, 1 error, 130 interrupted; `--json` events output (NDJSON) so the GUI could also drive the CLI as a subprocess if ever needed.
- GPU contention: serialize AI upscale jobs; ffmpeg stream-copy clipping can run concurrently. Expose `--jobs N`.

## 6. Project layout, testing, config

### Rust workspace layout [K]
```
reddit_media/
  Cargo.toml                 # [workspace] resolver="2", members, [workspace.dependencies], [workspace.lints]
  rust-toolchain.toml
  deny.toml                  # cargo-deny licenses/advisories
  crates/
    rm-core/                 # lib: model (JobSpec, Manifest), parsing (timestamps, multi-clip text, manifests),
                             #  ffprobe/ffmpeg command builders, tool discovery, runner, queue, events, config
    rm-cli/                  # bin "rmedia": clap derive, indicatif, JSON output, shell completions (clap_complete), man page (clap_mangen)
  apps/
    desktop/
      src-tauri/             # tauri app crate depends on rm-core; commands + Channel event bridge
      ui/                    # Svelte/React/Solid (Vite) frontend; types via specta/tauri-specta or ts-rs
  tests/                     # integration tests / fixtures generator
  xtask/                     # cargo xtask: fetch sidecars (per-triple), package, checksums
  research/ docs/
```
- Keep core free of Tauri deps; Tauri crate only adapts. Core has Cargo features: `ai-ncnn` (default), `ai-onnx` (optional, `ort`).
- Frontend choice: Svelte 5 or Solid give small bundles and simple reactive progress UIs; React has biggest ecosystem. Any is fine; pick what the team knows.
- Types: tauri-specta or ts-rs to generate TS bindings from Rust JobEvent/JobSpec to avoid drift.
- Error handling: thiserror in core, anyhow in binaries; tracing + tracing-appender for logs.

### Testing strategy [K]
- Generate fixtures at test time with ffmpeg lavfi, never commit binaries: `ffmpeg -f lavfi -i testsrc2=size=320x240:rate=25:duration=10 -f lavfi -i sine=frequency=440:duration=10 -c:v libx264 -pix_fmt yuv420p -g 25 -c:a aac fixture.mp4` (explicit keyframe interval -g makes cut accuracy testable). Cache in target/ via OnceLock.
- Assertions via ffprobe JSON: duration within tolerance (stream-copy cuts snap to keyframes; re-encode cuts are frame-accurate), stream count, resolution after upscale (2x/4x), codec.
- Golden tests: golden the *command lines* (insta snapshots of argv Vec for each JobSpec) and parsed manifests; for media output compare ffprobe metadata or per-frame hashes (`-f framemd5`) on deterministic lavfi input with a pinned ffmpeg version, or SSIM/PSNR thresholds (`-lavfi ssim`) rather than byte equality.
- Unit tests: timestamp parsing (HH:MM:SS.mmm, seconds, MM:SS), multi-clip text file grammar, manifest validation, directory scanning (tempfile + walkdir), progress parser fed with canned ffmpeg `-progress` output, cancellation test using a fake child (`sleep`/ping) to verify process-tree kill.
- Gate AI tests with `#[ignore]`/env var (needs Vulkan GPU); CI uses ffmpeg-only tests plus a fake upscaler script.
- CLI tests: assert_cmd + predicates. GUI: unit-test the bridge; E2E via tauri-driver/WebDriver (Linux: WebKitWebDriver; Windows: msedgedriver) optional, keep smoke-level.
- CI matrix [K]: ubuntu-22.04 (build base for AppImage glibc), windows-latest; steps: install ffmpeg (apt / choco / BtbN), cargo fmt/clippy/test/deny, tauri-action build, upload artifacts, sign on tag.

### Config conventions [K]
- Use `directories`/`dirs` or `etcetera` crates (ProjectDirs). Linux: config `$XDG_CONFIG_HOME/rmedia/config.toml` (default ~/.config), data (downloaded tools, queue DB) `$XDG_DATA_HOME/rmedia` (~/.local/share), cache/state `$XDG_CACHE_HOME`, `$XDG_STATE_HOME` (logs). Windows: `%APPDATA%\rmedia\config\config.toml` (Roaming) for config; `%LOCALAPPDATA%\rmedia\` for data/cache/tools/logs (not roamed; big files).
- Format: TOML (serde + toml / figment for layered merging). Precedence: CLI flags > env vars (RMEDIA_*) > project-local file (./rmedia.toml) > user config > defaults. Share same config file between CLI and GUI; GUI writes via core API with atomic write. Include `config_version` for migration. Provide `rmedia config path|get|set|doctor` (doctor checks ffmpeg/ffprobe/Vulkan).
- Tauri's own app dirs via app.path(); keep app identifier consistent with the CLI project dir name so both resolve the same dirs (identifier com.x.rmedia vs ProjectDirs qualifiers: set explicitly).

## 7. Cross-cutting: signing, CI, distribution facts
- Windows code signing 2026 [S]: CA/B Forum cap on code-signing certificate validity of 460 days from 2026-03-01 [S: https://devclass.com/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/]. Microsoft's Azure Artifact Signing (formerly Trusted Signing) is GA in USA, Canada, Europe; short-lived (24 h) certs renewed daily with timestamping; $9.99/mo (5,000 signatures, 1 profile) or $99.99/mo (100k, 10 profiles) [S: same devclass link; also https://techcommunity.microsoft.com/blog/microsoft-security-blog/-/4482789]. Eligibility is regional/org-verification dependent; check. Alternative: EV/OV cert from CA with cloud HSM (hardware token required since 2023 [K]) or SignPath Foundation (free for OSS) [K]. Tauri supports custom signCommand for Windows (use for Azure/relic/AzureSignTool) [K].
- Unsigned installers trigger SmartScreen warnings; MSI/NSIS both signable. Linux: no OS-level signing needed; sign AppImage (GPG) and repo/deb optionally; Flatpak via Flathub handles signing.
- Updater needs its own signing key (TAURI_SIGNING_PRIVATE_KEY in CI secrets) [K].
- Cross-compile Windows from Linux [K]: cargo-xwin (MSVC target using downloaded Windows SDK; mind Microsoft SDK licensing) or x86_64-pc-windows-gnu + mingw-w64; for Tauri GUI only NSIS is supported cross-built and WebView2 loader complicates mingw; for the pure CLI both work fine (cargo-xwin or `cargo zigbuild`). Recommendation: CLI may cross-build, GUI built on windows-latest runner.
- Flatpak [K]: write manifest (org.gnome.Platform + rust SDK extension, offline cargo-sources via flatpak-cargo-generator); sandbox means ffmpeg from runtime extension or bundled module; filesystem portal access for user videos.

## Recommendations

Primary stack: **Rust workspace: `rm-core` library + `rmedia` clap CLI + Tauri v2 GUI (Svelte or whatever the frontend dev prefers; Svelte 5 + Vite suggested).**

Justification:
1. Process orchestration is the core job; tokio::process + CancellationToken + Job Objects/process groups gives precise control, and the same core crate serves both front ends without IPC or duplicate logic.
2. Smallest installs (single-digit MB app; size dominated by ffmpeg you choose to bundle) vs Qt/PyInstaller's 80-250 MB.
3. Tauri v2 bundler covers NSIS + MSI + AppImage + deb + rpm, plus an official updater supporting all those bundle types (updater 2.10.x, Feb-Apr 2026) and an official GitHub Action; Go/Wails v3 is still beta and has no equivalent updater/packager; egui/Slint/Iced would require building packaging/updating by hand.
4. Team already has a Tauri skill and all toolchains (Rust 1.94, Node 24).
5. Permissive licensing (MIT/Apache) throughout; only ffmpeg builds carry GPL considerations and these are handled by spawning external binaries.

Key decisions/mitigations:
- Keep ffmpeg/realesrgan as external processes (never link libav); ship license texts and source offer for GPL builds; offer an LGPL build option.
- Tool resolution order: config override > bundled > app-data downloaded > PATH. Bundle ffmpeg+ffprobe (Windows installer, AppImage); depend on distro ffmpeg for .deb/.rpm; make AI upscaler (realesrgan-ncnn-vulkan + models) an optional first-run/on-demand download with SHA-256 pinning. Treat ONNX Runtime as optional feature, not default.
- Use `tauri::ipc::Channel` for progress streaming; ffmpeg `-progress pipe:1`; graceful 'q' then kill; Windows Job Object (KILL_ON_JOB_CLOSE) + CREATE_NO_WINDOW; concurrency 1 for GPU jobs.
- CI: GitHub Actions matrix (ubuntu-22.04 for AppImage/deb/rpm, windows-latest for NSIS/MSI); do not rely on cross-compiling the GUI; CLI may be cross-built via cargo-xwin/zigbuild. Sign Windows artifacts with Azure Artifact Signing if eligible (else SignPath/OV cert); sign updater payloads with the Tauri key.
- Known Linux risk: webkit2gtk/AppImage rendering quirks; mitigate by building on oldest supported distro, testing on Wayland+X11 and NVIDIA, and also publishing .deb/.rpm (and Flatpak later).
- Fallback if webview risk proves unacceptable: Slint (check license) or egui over the same rm-core; this is cheap because the core is GUI-agnostic. Choose Python+PySide6 only if rapid prototyping and Qt-native feel outweigh 100+ MB installers.

Items to re-verify before committing (marked [K] above): current Tauri 2.x core version/changelog, cargo-xwin + Tauri NSIS status, current BtbN/gyan ffmpeg sizes, Wails v3 stable status, Slint license terms, Azure Artifact Signing eligibility for your org/region.

Sources:
- https://v2.tauri.app/release/updater/
- https://tauri.app/release/shell/all-versions/
- https://github.com/wailsapp/wails/releases
- https://newreleases.io/project/github/wailsapp/wails/release/v3.0.0-beta.0
- https://devclass.com/2026/01/14/code-signing-windows-apps-may-be-easier-and-more-secure-with-new-azure-artifact-service/
- https://techcommunity.microsoft.com/blog/microsoft-security-blog/-/4482789
