# Build and Release

## Overview

This guide is for maintainers who run CI, update the bundled FFmpeg, and produce rmedia release
packages. It covers the CI pipeline, the release workflow, the pinned FFmpeg supply chain,
packaging, third-party notices, and code-signing status.

Authoritative sources: `.github/workflows/ci.yml`, `.github/workflows/release.yml`,
`.github/dependabot.yml`, `deny.toml`, `xtask/src/main.rs`, `xtask/tools.lock.json`,
`apps/desktop/src-tauri/tauri.conf.json`, `licenses/FFMPEG-NOTICE.md`.

For local builds and tests, see the [Development Guide](../developer/development-guide.md). For
what the packages install, see [Installation](../administration/installation.md).

## Build Tasks (`cargo xtask`)

`cargo xtask` is an alias for `cargo run --package xtask --` (`.cargo/config.toml`). Usage:

```text
cargo xtask <fetch-tools [--target TRIPLE] | pin-tools | gen-bindings | stage-cli [--target TRIPLE] | notices>
```

| Task | What it does | Writes |
|---|---|---|
| `fetch-tools [--target T]` | Downloads the pinned FFmpeg archive for target `T` (default: host), verifies its SHA-256, extracts `ffmpeg` and `ffprobe`, and installs them with Tauri sidecar names | `binaries/rmedia-ffmpeg-<T>[.exe]`, `binaries/rmedia-ffprobe-<T>[.exe]`, `binaries/FFMPEG-LICENSE.txt`, `binaries/.<T>.sha256` |
| `pin-tools` | Moves the pin to the newest BtbN autobuild of the **current release line** and records GitHub's asset SHA-256 digests | `xtask/tools.lock.json` |
| `gen-bindings` | Regenerates the desktop app's TypeScript types from `rmedia-core` (ts-rs) | `apps/desktop/src/lib/bindings/` (deleted and recreated) |
| `stage-cli [--target T]` | Builds `rmedia-cli` in release mode and copies it with sidecar naming | `binaries/rmedia-<T>[.exe]` |
| `notices` | Generates the third-party notices for every Rust crate and bundled npm package | `THIRD_PARTY_NOTICES.md` |

Supported targets are `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. On Windows the host
triple is `x86_64-pc-windows-msvc`; elsewhere it is `x86_64-unknown-linux-gnu`.

`binaries/` is in `.gitignore`. Engine binaries are never committed.

## Pinned FFmpeg Supply Chain

### The pin

`xtask/tools.lock.json` pins one FFmpeg build per target:

| Field | Current value |
|---|---|
| Source | BtbN/FFmpeg-Builds, GPL flavour (`linux64-gpl`, `win64-gpl`) |
| `release` | `autobuild-2026-10-05-13-07` |
| `version` | `n9.0.2-22-g46d8f462ee` |
| Linux archive | `…-linux64-gpl-9.0.tar.xz`, SHA-256 `31674015…61314` |
| Windows archive | `…-win64-gpl-9.0.zip`, SHA-256 `1385d57b…877cf` |

### How `fetch-tools` verifies

1. If `binaries/.<target>.sha256` exists and matches the archive hash plus the **current** SHA-256
   of both installed binaries, the task does nothing (`already present (verified)`). A tampered or
   corrupted binary fails this check and is replaced.
2. Otherwise it downloads the archive with `curl -fL --retry 3` into `target/xtask-tools/`.
3. It compares the archive SHA-256 with the pin and stops on mismatch:
   `checksum mismatch for <url>: expected <hash>, got <hash>`.
4. It extracts with `tar -xf`. On Windows it uses `%SystemRoot%\System32\tar.exe` (bsdtar, which
   reads `.zip`), because a GNU tar earlier on PATH (Git Bash) cannot.
5. It copies `bin/ffmpeg` and `bin/ffprobe` from the archive, sets mode `0755` on Unix, copies the
   archive's `LICENSE.txt`, and writes the stamp file.

Prerequisites: `curl` and `tar` on PATH (Linux), or `curl` plus the system `tar.exe` (Windows).

### Update the pinned FFmpeg

1. Run the pin task (needs network access to `api.github.com`):

   ```bash
   cargo xtask pin-tools
   ```

   It reads the five most recent BtbN releases, takes the first whose tag starts with `autobuild-`,
   and selects the `ffmpeg-n<line>…-{linux64,win64}-gpl-<line>` assets for the **same major.minor
   line** as the current pin (currently 9.0). It records each asset's `digest` field from the GitHub
   API as the SHA-256.
2. Fetch and test with the new build:

   ```bash
   cargo xtask fetch-tools
   export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
          RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
   cargo test --workspace
   ```

3. **Update `licenses/FFMPEG-NOTICE.md` by hand.** `pin-tools` does not touch it. The notice names
   the exact build, release tag, and FFmpeg commit, and the GPL corresponding-source links point at
   that tag.
4. Commit `xtask/tools.lock.json` and `licenses/FFMPEG-NOTICE.md` together in a reviewed commit.

To move to a new FFmpeg release line (for example 9.0 to 9.1), `pin-tools` cannot help: it only
searches the current line. Edit `version` in `tools.lock.json` to the new line first, then run
`pin-tools`.

> **Inferred:** The "edit `version` first" step follows from how `pin_tools()` derives the line
> from the existing `version` field. It has not been exercised.

### Supply-chain caveats and the mirroring requirement

From `specs/001-video-clipping/research.md` R9 (audit M3):

- BtbN builds are **not signed**. The only integrity anchor is the SHA-256 in reviewed commits.
- BtbN prunes dated `autobuild-*` tags over time. When the pinned tag disappears, `fetch-tools`
  fails for every new checkout and CI run.
- **Before the first public release**, mirror the pinned archives to rmedia's own release storage,
  and keep the hash in reviewed commits.
- `licenses/FFMPEG-NOTICE.md` likewise states that a copy of the FFmpeg source archive is mirrored
  alongside the rmedia release downloads before the first public release, to satisfy the GPL v3 §6
  source offer.

> **Unverified / open:** Neither mirror exists yet. `tools.lock.json` still points at
> `github.com/BtbN/...`, and there is no release storage configured. Treat both mirrors as release
> blockers.

Installer size is another open owner decision. The static GPL binaries are about 172 MB each; a
shared or trimmed custom build would reduce it (`research.md` R9, `validation.md`).

## Continuous Integration (`ci.yml`)

Triggers: push to `main`, every pull request, and weekly on Monday at 06:17 UTC (to catch new
advisories against unchanged code). Permissions: `contents: read`. `RMEDIA_REQUIRE_FFMPEG=1` is set
for the whole workflow, so media tests fail rather than skip when FFmpeg is missing.

All third-party actions are pinned to full commit SHAs. The Rust toolchain comes from
`rust-toolchain.toml` through the runner's preinstalled rustup. Node.js is not set up explicitly;
jobs use the runner's preinstalled Node.

| Job | Runners | Steps |
|---|---|---|
| `test` (bundled engine) | `ubuntu-22.04`, `windows-latest` | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo xtask fetch-tools`; `cargo test --workspace`; `cargo test --workspace --release -- --ignored bench` (with `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` set to the bundled engine) |
| `minimum-engine` | `ubuntu-24.04` | Installs distro `ffmpeg` (6.1, the minimum supported); `cargo test --workspace` |
| `windows-check-gnu` | `ubuntu-22.04` | `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu` (type-checks Windows code paths from Linux) |
| `desktop` | `ubuntu-22.04`, `windows-latest` | Linux webview dependencies; `fetch-tools` + `stage-cli` (tauri-build requires the sidecars to exist); `gen-bindings` then `git diff --exit-code` on the bindings; `npm ci`, `npm run check`, `npm test`, `npm run build`; Linux only: desktop backend tests and the notices check; Windows only: `cargo build` of the desktop backend |
| `deny` | `ubuntu-22.04` | `cargo-deny` for `Cargo.toml` and for `apps/desktop/src-tauri/Cargo.toml` |

Generated-file gates: CI fails if `apps/desktop/src/lib/bindings` or `THIRD_PARTY_NOTICES.md`
differ from freshly generated output. Regenerate and commit them with `cargo xtask gen-bindings` and
`cargo xtask notices`.

Gaps visible in the workflow (not defects, but worth knowing):

- The desktop backend tests (`apps/desktop/src-tauri/tests`) run on Linux only. Windows builds the
  desktop backend but does not test it.
- Clippy and `cargo fmt` cover the root workspace only. The desktop crate is excluded from the
  workspace (`Cargo.toml` → `exclude`), so it is not linted with `-D warnings`.

### Dependency policy (`deny.toml`)

| Section | Rule |
|---|---|
| `advisories` | Yanked crates are errors. |
| `licenses` | Allowed: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, CC0-1.0 (confidence 0.9). |
| `bans` | Duplicate versions warn; wildcard version requirements are errors (path dependencies allowed). |
| `sources` | Unknown registries and unknown Git sources are errors. |

### Dependabot (`dependabot.yml`)

Weekly update PRs for:

- `github-actions` (root directory);
- `cargo` in the root workspace (`/`);
- `cargo` in the separate desktop manifest (`/apps/desktop/src-tauri`);
- `npm` in the desktop frontend (`/apps/desktop`).

## Release Workflow (`release.yml`)

Triggers: push of a tag matching `v*`, or manual `workflow_dispatch`. Permissions:
`contents: read`. The workflow **builds** installers and CLI archives and uploads them as workflow
artifacts. It does not create a GitHub release, sign, or publish anything.

Matrix:

| Runner | Triple | Bundles |
|---|---|---|
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `deb,appimage` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `nsis,msi` |

Steps per platform:

1. Check out the code (no persisted credentials) and set up the toolchain.
2. Linux: install `libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev patchelf
   libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-base
   gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav`.
3. Stage the bundled programs: `cargo xtask fetch-tools --target <T>` and
   `cargo xtask stage-cli --target <T>`.
4. Run `cargo test --workspace` against the bundled engine (`RMEDIA_REQUIRE_FFMPEG=1`).
5. `npm ci` in `apps/desktop`.
6. Linux: check that `THIRD_PARTY_NOTICES.md` is current. The file covers both platforms.
7. `npx tauri build --bundles <bundles>` in `apps/desktop`.
8. Assemble the standalone CLI archive `rmedia-cli-<ref>-<triple>` (`.tar.gz` on Linux, `.zip` via
   `7z` on Windows) with `rmedia`, `rmedia-ffmpeg`, `rmedia-ffprobe`, `README.md`, and
   `licenses/{THIRD_PARTY_NOTICES.md,FFMPEG-NOTICE.md,FFMPEG-LICENSE.txt}`.
9. Upload the `.deb`, `.AppImage`, `.exe`, `.msi`, and archives as artifact `rmedia-<triple>`.
   The upload fails if no files are found.

The release workflow does not run clippy, the frontend tests, or the desktop backend tests. It
relies on CI having passed on the tagged commit.

## Packaging Details

### Sidecars

`tauri.conf.json` → `bundle.externalBin` lists `binaries/rmedia-ffmpeg`, `binaries/rmedia-ffprobe`,
and `binaries/rmedia`. Tauri appends the target triple when it looks for each file and strips it in
the installed package, so the installed names are `rmedia-ffmpeg`, `rmedia-ffprobe`, and `rmedia`,
next to `rmedia-desktop`. This is why `fetch-tools` and `stage-cli` must run before any
`tauri build` or `tauri dev`, and before `cargo build` of the desktop crate (tauri-build checks that
the files exist).

At run time, both `rmedia` and `rmedia-desktop` find the engine by looking next to their own
executable ([Engine location order](../administration/configuration.md#engine-location-order)).

### Resources

The license files are bundled as resources under `licenses/`: `THIRD_PARTY_NOTICES.md`,
`FFMPEG-NOTICE.md`, and `FFMPEG-LICENSE.txt`. In the deb they install to
`/usr/lib/rmedia/licenses/`.

### Linux specifics

- **deb:** section `video`; adds `gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good`, and
  `gstreamer1.0-libav` to the dependencies Tauri computes (WebKitGTK uses GStreamer for `<video>`).
- **AppImage:** `bundleMediaFramework: true` bundles GStreamer into the image. Without it the
  AppImage could not play any video (`validation.md`, found during the packaging run). The build
  host therefore needs the GStreamer development and plugin packages listed in the release workflow.
- **patchelf** is required on the build host for AppImage bundling (`specs/001-video-clipping/quickstart.md`).

### Windows specifics

- **NSIS:** `installMode: "currentUser"` (per-user install, no administrator rights needed).
- **MSI:** Tauri defaults.

### Release profile

`[profile.release]` in the root `Cargo.toml` uses `lto = "thin"` and `strip = true`.

## Third-Party Notices

`cargo xtask notices` regenerates `THIRD_PARTY_NOTICES.md`:

- Rust crates: runs `cargo metadata --filter-platform` for both release targets and both manifests
  (`Cargo.toml` and `apps/desktop/src-tauri/Cargo.toml`), and keeps only packages reachable through
  normal (non-dev, non-build) dependencies. Using the union of both platforms makes the file
  identical wherever it is generated.
- npm packages: walks the `dependencies` of `@tauri-apps/api` and `svelte` under
  `apps/desktop/node_modules`. **Run `npm ci` in `apps/desktop` first**; missing packages are
  skipped silently.
- License texts come from `LICENSE*`, `LICENCE*`, `COPYING*`, and `NOTICE*` files next to each
  package manifest. Packages that ship none get an appendix with the canonical text of their SPDX
  license, or a link to `spdx.org`.
- FFmpeg is not listed. It ships as a separate program and is covered by
  `licenses/FFMPEG-NOTICE.md`.

Regenerate notices whenever dependencies change. CI and the release workflow fail if the committed
file is stale.

## Code Signing

Code signing is **not set up**. `tauri.conf.json` has no Windows certificate or signing settings,
and neither workflow signs artifacts. The release workflow header calls signing "a separate, manual
step". `validation.md` lists code signing as an open owner decision.

Effects: Windows SmartScreen reputation warnings on the installers. No package checksums or
signatures are published.

## Cut a Release

`README.md` › Releasing gives the short version (the release workflow header points there). The
procedure below expands it from the workflows and manifests.

> **Note:** README › Releasing lists three version locations. `apps/desktop/src-tauri/Cargo.toml`
> also carries the version (the desktop crate is outside the workspace), so update all four.

### Prerequisites

- CI is green on the commit to release, on both operating systems.
- The FFmpeg archives and FFmpeg source are mirrored (see
  [the mirroring requirement](#supply-chain-caveats-and-the-mirroring-requirement)).
- The owner has decided rmedia's own license (currently a `MIT OR Apache-2.0` placeholder).

### Steps

1. Set the new version in all four places (each is currently `0.1.0`):
   - `Cargo.toml` → `[workspace.package] version`
   - `apps/desktop/src-tauri/Cargo.toml` → `[package] version`
   - `apps/desktop/src-tauri/tauri.conf.json` → `version`
   - `apps/desktop/package.json` → `version`
2. Regenerate generated files and commit any changes:

   ```bash
   (cd apps/desktop && npm ci)
   cargo xtask notices
   cargo xtask gen-bindings
   ```

3. Commit with a Conventional Commits message (for example `chore(release): v0.2.0`), push to
   `main`, and wait for CI to pass.
4. Tag and push:

   ```bash
   git tag v<version>
   git push origin v<version>
   ```

5. Wait for the **Release** workflow to finish. Download the `rmedia-x86_64-unknown-linux-gnu` and
   `rmedia-x86_64-pc-windows-msvc` artifacts.
6. Smoke-test on clean machines, as in `quickstart.md` §5: install, run `rmedia doctor` (expect
   `bundled`), cut a first clip, and confirm the notices are present in the install folder.
7. Sign (when signing is set up) and publish manually, together with the FFmpeg source archive.

### Version alignment

> **Inferred:** Tauri takes the installer version from `tauri.conf.json`. The CLI reports the
> workspace version (`rmedia --version`, from clap's `version` attribute). The CLI archive name
> uses the Git tag. Keep all of these aligned; nothing checks them automatically.
