# rmedia Installation, Upgrade, and Removal

## Overview

This guide is for the operator who installs rmedia on Linux or Windows workstations. It covers
each release package, what each package puts on disk, runtime dependencies, how to confirm that an
installation works, and how to upgrade or remove it.

rmedia ships two programs in every package:

- `rmedia`: the command-line interface (CLI).
- `rmedia-desktop`: the desktop app (Tauri 2). The standalone CLI archive does not include it.

Every package also bundles its own FFmpeg build as two separate programs, `rmedia-ffmpeg` and
`rmedia-ffprobe`. The renamed binaries never conflict with a system FFmpeg. rmedia runs them as
subprocesses and does not link FFmpeg libraries.

For settings, file locations, and environment variables, see
[rmedia Configuration](configuration.md). For how packages are produced, see
[Build and Release](../operations/build-and-release.md). For first use after installation, see
[Getting Started](../user/getting-started.md) and the [CLI Reference](../api/cli-reference.md).

## System Requirements

| Platform | Requirement | Source |
|---|---|---|
| Linux | x86_64, glibc 2.35 or later, X11 or Wayland | `specs/001-video-clipping/plan.md` (Target Platform) |
| Windows | Windows 10 or 11, x86_64 | `plan.md`, `README.md` |
| Both | No GPU required; CPU-only machines are fully supported | `plan.md` (Constraints) |

> **Unverified:** The glibc 2.35 minimum comes from the plan. It matches the `ubuntu-22.04` release
> runner (`.github/workflows/release.yml`), but no test on an older distribution has confirmed it.

Release builds exist only for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`
(`xtask/tools.lock.json`, `release.yml`). There are no ARM or macOS packages.

### Disk space

The bundled FFmpeg is a static GPL build of about 172 MB per program, uncompressed. Measured
package sizes for version 0.1.0 (`specs/001-video-clipping/checklists/validation.md`):

| Package | Size |
|---|---|
| `rmedia_0.1.0_amd64.deb` | 140 MiB |
| `rmedia_0.1.0_amd64.AppImage` | 284 MiB |

> **Unverified:** Sizes of the Windows installers and the CLI archives have not been measured.

## Choose a Package

| Package | File name | Platform | Includes desktop app | `rmedia` on PATH |
|---|---|---|---|---|
| Debian package | `rmedia_<version>_amd64.deb` | Debian/Ubuntu | Yes | Yes (`/usr/bin`) |
| AppImage | `rmedia_<version>_amd64.AppImage` | Other Linux | Yes | No (CLI stays inside the image) |
| NSIS installer | `rmedia_<version>_x64-setup.exe` | Windows | Yes | No (add the install folder to PATH) |
| MSI installer | `rmedia_<version>_x64_en-US.msi` (see note) | Windows | Yes | No |
| CLI archive | `rmedia-cli-<ref>-<triple>.tar.gz` / `.zip` | Linux / Windows | No | No (unpack anywhere) |

The package targets `deb`, `appimage`, `nsis`, and `msi` are set in
`apps/desktop/src-tauri/tauri.conf.json` (`bundle.targets`). The CLI archive is assembled by the
"Standalone CLI archive" step in `release.yml`.

> **Unverified:** The exact MSI file name comes from Tauri's defaults; no Windows build has been
> inspected locally.

## Runtime Dependencies

### Linux (`.deb`)

The Debian package declares these dependencies (read from the built
`rmedia_0.1.0_amd64.deb` with `dpkg-deb -f`):

- `libwebkit2gtk-4.1-0`, `libgtk-3-0` (desktop app webview; added by the Tauri bundler)
- `gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good`, `gstreamer1.0-libav` (declared in
  `tauri.conf.json` → `bundle.linux.deb.depends`; WebKitGTK uses GStreamer for `<video>` playback)

`apt` installs these automatically when you install the package from a local file.

### Linux (AppImage)

The AppImage bundles GStreamer (`bundle.linux.appimage.bundleMediaFramework: true`), so preview
playback works without system GStreamer plugins.

> **Unverified:** Whether the AppImage needs a system WebKitGTK or FUSE on the host is not
> documented in the repository. It was launched successfully under Xvfb on Ubuntu 24.04
> (`validation.md`).

GStreamer plugin-scanner warnings in the AppImage log are cosmetic; playback works
(`validation.md`, Open items).

### Windows

The desktop app needs Microsoft WebView2. It is preinstalled on Windows 11 and current Windows 10.
According to `README.md`, the setup program downloads WebView2 if it is missing.

> **Unverified:** The WebView2 install mode is not configured in `tauri.conf.json`, so the
> behaviour is Tauri's default (download bootstrapper). Neither the NSIS nor the MSI installer has
> been tested outside CI.

### CLI archive

The CLI and its bundled FFmpeg have no runtime dependencies beyond the operating system.

## Install

### Debian or Ubuntu (`.deb`)

1. Download `rmedia_<version>_amd64.deb`.
2. Install it with `apt` so that dependencies resolve:

   ```bash
   sudo apt install ./rmedia_<version>_amd64.deb
   ```

3. Verify the installation (see [Verify the Installation](#verify-the-installation)).

The package installs these files (from `dpkg-deb -c rmedia_0.1.0_amd64.deb`):

| Path | Contents |
|---|---|
| `/usr/bin/rmedia` | CLI |
| `/usr/bin/rmedia-desktop` | Desktop app |
| `/usr/bin/rmedia-ffmpeg` | Bundled FFmpeg |
| `/usr/bin/rmedia-ffprobe` | Bundled ffprobe |
| `/usr/lib/rmedia/licenses/THIRD_PARTY_NOTICES.md` | Rust and JavaScript component notices |
| `/usr/lib/rmedia/licenses/FFMPEG-NOTICE.md` | FFmpeg build and source offer |
| `/usr/lib/rmedia/licenses/FFMPEG-LICENSE.txt` | GPL v3 text |
| `/usr/share/applications/rmedia.desktop` | Desktop menu entry |
| `/usr/share/icons/hicolor/*/apps/rmedia-desktop.png` | Icons (32x32, 128x128, 256x256@2) |

The package section is `video` (`bundle.linux.deb.section`).

### Other Linux distributions (AppImage)

1. Download `rmedia_<version>_amd64.AppImage`.
2. Make it executable and run it:

   ```bash
   chmod +x rmedia_<version>_amd64.AppImage
   ./rmedia_<version>_amd64.AppImage
   ```

The AppImage does not put `rmedia` on PATH. For command-line use on these systems, also install
the CLI archive.

### Windows (NSIS setup, per user)

1. Download `rmedia_<version>_x64-setup.exe`.
2. Run it. The NSIS installer is configured for a per-user install
   (`bundle.windows.nsis.installMode: "currentUser"`), so it does not need administrator rights.
3. To use the CLI from any terminal, add the install folder to your user PATH, or use the CLI
   archive instead.

According to `README.md`, the per-user install folder is `%LOCALAPPDATA%\rmedia\`. `rmedia.exe`,
`rmedia-ffmpeg.exe`, and `rmedia-ffprobe.exe` are installed next to the desktop app executable.

> **Unverified:** The install folder and file layout come from `README.md` and Tauri's defaults
> for `currentUser` mode. No Windows installer has been inspected; Windows installers are only
> built in CI (`validation.md`, Open items).

### Windows (MSI)

1. Download the `.msi` file.
2. Run it.

> **Unverified:** The MSI install scope and folder are not configured in `tauri.conf.json`, so they
> follow Tauri's WiX defaults. Confirm them on a Windows test machine before you document them for
> users.

### CLI archive (Linux or Windows)

The archive is named `rmedia-cli-<ref>-<triple>`, where `<ref>` is the Git tag (for example
`v0.1.0`) with characters other than `A-Za-z0-9._-` replaced. It is a `.tar.gz` on Linux and a
`.zip` on Windows.

1. Unpack the archive anywhere, for example:

   ```bash
   tar -xzf rmedia-cli-<ref>-x86_64-unknown-linux-gnu.tar.gz
   ```

2. Optional: add the unpacked folder to PATH.

Archive contents (from `release.yml`):

```text
rmedia-cli-<ref>-<triple>/
├── rmedia[.exe]
├── rmedia-ffmpeg[.exe]
├── rmedia-ffprobe[.exe]
├── README.md
└── licenses/
    ├── THIRD_PARTY_NOTICES.md
    ├── FFMPEG-NOTICE.md
    └── FFMPEG-LICENSE.txt
```

Keep `rmedia-ffmpeg` and `rmedia-ffprobe` in the same folder as `rmedia`. rmedia finds the bundled
engine by looking next to its own executable (`crates/rmedia-core/src/tools.rs`).

## Verify the Installation

Run the diagnostics command:

```bash
rmedia doctor
```

Healthy output looks like this (abbreviated; the encoder lines vary by FFmpeg build):

```text
ffmpeg:   /usr/bin/rmedia-ffmpeg (bundled)
ffprobe:  /usr/bin/rmedia-ffprobe (bundled)
version:  n9.0.2-31-gc89d6ee1ca-20261021
encoder:  aac         yes
encoder:  libopus     yes
encoder:  libvpx-vp9  yes
encoder:  libx264     yes
accurate mode can write: mp4, m4v, mov, mkv, ts, webm
status:   ready
```

What to check:

- The source in parentheses is `bundled`. `config` means an explicit path or environment variable
  overrode it; `path` means a system FFmpeg on PATH was used. See
  [Engine location order](configuration.md#engine-location-order).
- `status:` is `ready`. Problems print on standard error as `problem:` lines.
- Exit code is `0` when ready and `4` when the engine is missing or unusable
  (`crates/rmedia-cli/src/commands/doctor.rs`).

For machine-readable output, run `rmedia doctor --json`.

In the desktop app, the same report is shown under **Settings** › **Video engine**
(`validation.md`, FR-026).

To confirm that the bundled engine is self-sufficient, run doctor with an empty PATH (this is how
the Linux packages were validated):

```bash
PATH= /usr/bin/rmedia doctor
```

## Upgrade

Install the newer package over the existing one:

- **deb:** `sudo apt install ./rmedia_<new-version>_amd64.deb`
- **AppImage:** replace the old `.AppImage` file with the new one.
- **CLI archive:** unpack the new archive and replace the old folder.
- **NSIS / MSI:** run the new installer.

> **Unverified:** In-place upgrade behaviour of the NSIS and MSI installers (whether the old version
> is removed automatically) follows Tauri defaults and has not been tested.

User data is not part of any package. `settings.json`, the crash journal, and the preview cache
stay in the per-user directories described in
[File locations](configuration.md#file-locations), and are kept across upgrades.

There is no automatic update mechanism; the Tauri updater plugin is not used
(`apps/desktop/src-tauri/Cargo.toml`).

### Roll back

Install the previous package version the same way. `settings.json` rejects unknown fields
(`#[serde(deny_unknown_fields)]` in `settings.rs`). If a newer version added a settings field and you
roll back, the older version silently ignores the whole file and uses defaults. Remove the new
field, or delete `settings.json`, to restore your settings.

> **Inferred:** No settings field has been added since 0.1.0, so this is a forward-looking caution
> derived from the code, not an observed problem.

## Uninstall

| Package | Command or action |
|---|---|
| deb | `sudo apt remove rmedia` |
| AppImage | Delete the `.AppImage` file |
| CLI archive | Delete the unpacked folder (and remove it from PATH if you added it) |
| NSIS / MSI | **Settings** › **Apps** › **rmedia** › **Uninstall** |

> **Unverified:** The Windows uninstall entry name assumes the Tauri default (product name
> `rmedia`).

Uninstalling does not remove per-user data. To remove it as well, delete these directories (see
[File locations](configuration.md#file-locations)):

- Linux: `~/.config/rmedia/`, `~/.local/state/rmedia/`, `~/.cache/rmedia/`
- Windows: `%APPDATA%\OpenLogic\rmedia\` and `%LOCALAPPDATA%\OpenLogic\rmedia\`

Clips that rmedia produced are ordinary files in your output folders and are never removed.

## Licensing Notices

Every package ships the license notices:

- `THIRD_PARTY_NOTICES.md`: every Rust crate and npm package compiled into rmedia.
- `FFMPEG-NOTICE.md`: the exact FFmpeg build, its configuration (`--enable-gpl --enable-version3`),
  and the GPL v3 §6 corresponding-source offer.
- `FFMPEG-LICENSE.txt`: GPL v3 text.

rmedia's own license has not been decided. The Cargo manifests declare `MIT OR Apache-2.0` as a
placeholder (`README.md`, Licensing).

## Security Notes

- Installers and binaries are **not code-signed**. Windows SmartScreen and similar tools may warn
  on first run. See [Build and Release](../operations/build-and-release.md#code-signing).
- The desktop app opens a media server bound to `127.0.0.1` on an ephemeral port to feed the video
  player. It does not listen on external interfaces, and access needs a per-session random token
  (`apps/desktop/src-tauri/src/media_server.rs`). No firewall rule is needed.
- rmedia needs no network access at run time (`plan.md`: "works offline").

## Troubleshooting

| Symptom | Likely cause | Resolution |
|---|---|---|
| `rmedia doctor` prints `NOT READY` and exits 4 with `ffmpeg not found: not bundled with rmedia and not on PATH (use --ffmpeg PATH)` | `rmedia-ffmpeg` is not next to `rmedia`, and no FFmpeg is on PATH | Keep the bundled programs next to `rmedia`, or pass `--ffmpeg PATH` |
| `problem:  ffmpeg <version> is too old; version 6.0 or newer is required` | A system FFmpeg older than 6.0 was selected | Use the bundled engine, or point `RMEDIA_FFMPEG` at FFmpeg 6.0 or later |
| `problem:  <path> does not exist` | `--ffmpeg`, `RMEDIA_FFMPEG`, or a desktop engine path points at a missing file | Fix or unset the override |
| Desktop app on Linux shows no video | GStreamer plugins missing (deb) | Install `gstreamer1.0-plugins-good` and `gstreamer1.0-libav` |
| `rmedia: command not found` after installing the AppImage or a Windows installer | These packages do not add the CLI to PATH | Use the CLI archive, or add the install folder to PATH (Windows) |
