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
cargo xtask <fetch-tools [--target TRIPLE] | pin-tools | gen-bindings | stage-cli [--target TRIPLE] | notices | fetch-models | fetch-ncnn [--target TRIPLE] | archive | mirror <tag> | check-pins [--online]>
```

| Task | What it does | Writes |
|---|---|---|
| `fetch-tools [--target T]` | Downloads the pinned FFmpeg archive for target `T` (default: host), verifies its SHA-256, extracts `ffmpeg` and `ffprobe`, and installs them with Tauri sidecar names | `binaries/rmedia-ffmpeg-<T>[.exe]`, `binaries/rmedia-ffprobe-<T>[.exe]`, `binaries/FFMPEG-LICENSE.txt`, `binaries/.<T>.sha256` |
| `pin-tools` | Moves the pin to the newest BtbN autobuild of the **current release line** and records GitHub's asset SHA-256 digests and the BtbN commit of the tag (`scripts_commit`); the lock is written only after both new archives are downloaded and verified | `xtask/tools.lock.json` |
| `gen-bindings` | Regenerates the desktop app's TypeScript types from `rmedia-core` (ts-rs) | `apps/desktop/src/lib/bindings/` (deleted and recreated) |
| `stage-cli [--target T]` | Builds `rmedia-cli` and the AI worker `rmedia-sr` in release mode and copies them with sidecar naming (needs `fetch-ncnn` first) | `binaries/rmedia-<T>[.exe]`, `binaries/rmedia-sr-<T>[.exe]` |
| `fetch-ncnn [--target T]` | Downloads the pinned prebuilt ncnn 20260526 (static, Vulkan; `ubuntu-2204` or `windows-vs2022`) for target `T`, verifies its SHA-256 and installs it for `crates/rmedia-sr/build.rs`, which stops with "run `cargo xtask fetch-ncnn`" without it. `RMEDIA_NCNN_DIR` (build time) points the build at another ncnn package instead. Added in 0.2.0 (US6) | `third_party/ncnn/<T>/` (git-ignored) |
| `notices` | Generates the third-party notices for every Rust crate and bundled npm package | `THIRD_PARTY_NOTICES.md` |
| `fetch-models` | Downloads each model in `xtask/models.lock.json` (the YuNet face detector, the Real-ESRGAN animevideov3 x2/x3/x4 files taken from a pinned release archive, and a test-only portrait) with `curl` over HTTPS only, verifies the SHA-256 of each archive and file, and replaces it through a temp file and rename. Files that already match are kept. Added in 0.2.0. | `models/` (git-ignored; never committed) |
| `archive` | Downloads every pinned archive (FFmpeg and ncnn for both targets, model release archives) and checks it against its pin; lists SHA-256, size and path | `archives/<sha256>/` (git-ignored) |
| `mirror <tag>` | Uploads each pinned download without a mirror (archives and the directly downloaded model files) to the release `<tag>` (a `mirror-<date>` pre-release, created if missing; an existing asset of the same name must have the same SHA-256), adds the URLs under the name GitHub stored (it renames some characters, such as `~`) to `mirrors` in both lock files after checking the edit, and records the `ffmpeg-source-<version>` release as `source`; needs `gh` with write access | `xtask/*.lock.json` |
| `check-pins [--online]` | CI check: every pinned download has a mirror, the FFmpeg pin has `scripts_commit` (the BtbN commit of the shipped build) and `source`, and `licenses/FFMPEG-NOTICE.md` names the version, release tag and source release. `--online` also requests every mirror URL, so a renamed or deleted mirror asset is noticed | nothing |

Supported targets are `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. On Windows the host
triple is `x86_64-pc-windows-msvc`; elsewhere it is `x86_64-unknown-linux-gnu`.

`binaries/` is in `.gitignore`. Engine binaries are never committed.

## Pinned FFmpeg Supply Chain

### The pin

`xtask/tools.lock.json` pins one FFmpeg build per target:

| Field | Current value |
|---|---|
| Source | BtbN/FFmpeg-Builds, GPL flavour (`linux64-gpl`, `win64-gpl`) |
| `release` | `autobuild-2026-10-21-13-06` |
| `version` | `n9.0.2-31-gc89d6ee1ca` |
| Linux archive | `…-linux64-gpl-9.0.tar.xz`, SHA-256 `22120095…97bde` |
| Windows archive | `…-win64-gpl-9.0.zip`, SHA-256 `48c50bfe…9befa` |
| `mirrors` (per target, optional) | Our own copies of the same archive, tried before `url` |

### How `fetch-tools` verifies

1. If `binaries/.<target>.sha256` exists and matches the archive hash plus the **current** SHA-256
   of both installed binaries, the task does nothing (`already present (verified)`). A tampered or
   corrupted binary fails this check and is replaced.
2. Otherwise it takes the archive from `archives/<sha256>/` (gitignored) when the copy kept there
   still matches the pin. If not, it downloads from each `mirrors` URL and then `url` (HTTPS only,
   `curl -fL --retry 3`) until one matches the pin, and keeps that copy in `archives/`. When no
   source matches, it stops with `no source of <file> matches the pin:` and one line per source
   (a 404, or `checksum <hash>, expected <hash>`).
3. Archives are never deleted after extraction, so a pin keeps working on this machine after the
   upstream release is gone. `fetch-ncnn` and `fetch-models` (for model release archives) use the
   same store and the same `mirrors` field in `xtask/models.lock.json`.
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
   API as the SHA-256, then downloads both archives into `archives/` at once, because the
   autobuild release will be deleted upstream.
2. Fetch and test with the new build:

   ```bash
   cargo xtask fetch-tools
   export RMEDIA_FFMPEG=$PWD/binaries/rmedia-ffmpeg-x86_64-unknown-linux-gnu \
          RMEDIA_FFPROBE=$PWD/binaries/rmedia-ffprobe-x86_64-unknown-linux-gnu
   cargo test --workspace
   ```

3. **Publish the corresponding source** while the new autobuild tag still exists upstream (BtbN
   deletes old tags along with their releases):

   ```bash
   gh workflow run ffmpeg-source.yml
   ```

   See [The FFmpeg source release](#the-ffmpeg-source-release).
4. **Mirror the new archives and record the source release:**

   ```bash
   cargo xtask mirror mirror-$(date +%F)
   ```

   It uploads every pinned archive that has no mirror yet to a `mirror-<date>` pre-release of the
   repository (creating it if needed), adds the URLs under `mirrors` in both lock files, and sets
   `source` in `tools.lock.json` once the `ffmpeg-source-<version>` release exists. It needs `gh`
   logged in with write access.
5. **Update `licenses/FFMPEG-NOTICE.md` by hand.** The notice names the exact build, release tag
   and FFmpeg commit, and links the source release.
6. `cargo xtask check-pins` must pass (CI runs it on every push): every pinned archive has a
   mirror, `source` is set, and the notice names the version, the release tag and the source
   release. Commit `xtask/*.lock.json` and `licenses/FFMPEG-NOTICE.md` together in a reviewed commit.

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

**What happened (2026-10-22):** BtbN deleted `autobuild-2026-10-05-13-07`, and `fetch-tools`
failed with a 404 on a fresh checkout. That build's archives were lost, because `fetch-tools`
used to delete them after extraction. FFmpeg was re-pinned to `autobuild-2026-10-21-13-06`, and
`fetch-tools` now keeps every archive.

Mirrors are created by `cargo xtask mirror <tag>` (step 4 above); `cargo xtask archive` only
downloads and verifies every pinned archive into `archives/` (about 480 MB). To set up a machine
without network access to the upstream releases, copy `archives/` from a machine that has it;
`fetch-*` use those copies after checking their hashes.

**Current mirror:** the public pre-release
[`mirror-2026-10-22`](https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/tag/mirror-2026-10-22)
holds all five pinned archives and the two directly downloaded model files (YuNet and the test
portrait), and every pin lists it under `mirrors`. It was checked by fetching
everything with the upstream URLs pointed at a host that does not exist (all five came from the
mirror and matched), by breaking the mirror (the upstream URL was used), and by pointing a mirror
at a different file (rejected: `checksum …, expected …`).

### The FFmpeg source release

The mirror distributes the FFmpeg binaries publicly, so the GPL v3 §6 source offer in
`licenses/FFMPEG-NOTICE.md` applies. `.github/workflows/ffmpeg-source.yml` (run by hand) publishes
the complete corresponding source of the pinned build as the pre-release
`ffmpeg-source-<version>`. It runs from `main` only, in two jobs: `build` runs BtbN's scripts with
a read-only token, fails unless the pinned tag still points at `scripts_commit`, and uploads the
files as an artifact; `publish` holds the write token, checks `SHA256SUMS` and creates the release
(or reports that it already exists). The release contains:

- `ffmpeg-<version>.tar.xz`: FFmpeg at the pinned commit (taken from `version`, on
  `release/<line>`).
- `FFmpeg-Builds-<release>.tar.xz`: BtbN's build scripts at the pinned autobuild tag.
- `<stage>_<hash>.tar.xz`: every library source, fetched by BtbN's own `download.sh` in BtbN's base
  image (a superset: every target and variant BtbN builds).
- `SHA256SUMS`.

**Current source release:**
[`ffmpeg-source-n9.0.2-31-gc89d6ee1ca`](https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/tag/ffmpeg-source-n9.0.2-31-gc89d6ee1ca)
(130 files, about 2.1 GB; FFmpeg commit `c89d6ee1ca848314270155ceb13b1cab3744fc3c`, BtbN commit
`937868360a21d711ec94a33f5591d345ad9b7754`). Its `SHA256SUMS` and a sample of the files (FFmpeg,
the build scripts, x264) were checked after publishing.

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
| `test` (bundled engine) | `ubuntu-22.04`, `windows-latest` | `cargo fmt --all -- --check`; `cargo xtask check-pins`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo xtask fetch-tools`; `cargo test --workspace`; `cargo test --workspace --release -- --ignored bench` (with `RMEDIA_FFMPEG`/`RMEDIA_FFPROBE` set to the bundled engine) |
| `minimum-engine` | `ubuntu-24.04` | Installs distro `ffmpeg` (6.1, the minimum supported); `cargo xtask fetch-models` and `fetch-ncnn`; `cargo test --workspace` with `RMEDIA_ALLOW_MISSING_FEATURES=1` (tests needing a feature 6.1 lacks, such as libvmaf, skip) |
| `windows-check-gnu` | `ubuntu-22.04` | Installs mingw-w64 (tract-linalg compiles C); `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu --exclude rmedia-sr` (type-checks Windows code paths from Linux; the AI worker links MSVC ncnn and is built by the Windows test job) |
| `desktop` | `ubuntu-22.04`, `windows-latest` | Linux webview dependencies; `fetch-tools`, `fetch-ncnn`, `fetch-models` + `stage-cli` (tauri-build requires the sidecars and bundled models to exist); `gen-bindings` then `git diff --exit-code` on the bindings; `npm ci`, `npm run check`, `npm test`, `npm run build`; Linux only: desktop backend tests (against the staged pinned engine; no distro FFmpeg is installed) and the notices check; Windows only: `cargo build` of the desktop backend |
| `deny` | `ubuntu-22.04` | `cargo-deny` for `Cargo.toml` and for `apps/desktop/src-tauri/Cargo.toml`; `deny.toml` ignores one advisory, RUSTSEC-2024-0370 (unmaintained build-time proc-macro of gtk-rs 0.18, via Tauri's Linux backend), with its reason |

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

Triggers: push of a tag matching `v*`, or manual `workflow_dispatch`, whose `gifski` input
(default on) chooses whether gifski is bundled. A tag push always bundles it. Permissions:
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
3. Release gate: `cargo xtask check-release-gate` runs before and after staging. A tree with
   gifski staged fails until the application licence is decided (FR-025, docs/roadmap.md), so
   today only a manual run with `gifski` off can pass.
4. Stage the bundled programs and models: `cargo xtask fetch-tools --target <T>` (with
   `--no-gifski` when the input is off; gifsicle is always staged),
   `cargo xtask fetch-ncnn --target <T>`, `cargo xtask fetch-models` and
   `cargo xtask stage-cli --target <T>`.
5. Run `cargo test --workspace` against the bundled engine (`RMEDIA_REQUIRE_FFMPEG=1`).
6. `npm ci` in `apps/desktop`.
7. Linux: check that `THIRD_PARTY_NOTICES.md` is current. The file covers both platforms.
8. `npx tauri build --bundles <bundles>` in `apps/desktop`. Without gifski it adds `--config`
   with a JSON merge patch (made by `jq` from `tauri.conf.json`) that drops `rmedia-gifski` from
   `externalBin` and sets its licence resources to `null`.
9. Assemble the standalone CLI archive `rmedia-cli-<ref>-<triple>` (`.tar.gz` on Linux, `.zip` via
   `7z` on Windows) with `rmedia`, `rmedia-sr`, `rmedia-ffmpeg`, `rmedia-ffprobe`,
   `rmedia-gifsicle`, `README.md`, `models/` (YuNet and Real-ESRGAN x2/x3/x4) and
   `licenses/{THIRD_PARTY_NOTICES.md,FFMPEG-NOTICE.md,FFMPEG-LICENSE.txt,NCNN-NOTICE.md,
   NCNN-LICENSE.txt,GLSLANG-LICENSE.txt,GIFSICLE-NOTICE.md,GIFSICLE-COPYING.txt}`; with gifski,
   also `rmedia-gifski` and `GIFSKI-{NOTICE.md,LICENSE.txt,DEPS.md}`.
10. Upload the `.deb`, `.AppImage`, `.exe`, `.msi`, and archives as artifact `rmedia-<triple>`.
   The upload fails if no files are found.

The release workflow does not run clippy, the frontend tests, or the desktop backend tests. It
relies on CI having passed on the tagged commit.

## Packaging Details

### Sidecars

`tauri.conf.json` → `bundle.externalBin` lists `binaries/rmedia-ffmpeg`, `binaries/rmedia-ffprobe`,
`binaries/rmedia`, `binaries/rmedia-sr` (the AI worker) and the GIF engines
`binaries/rmedia-gifsicle` and `binaries/rmedia-gifski`. Tauri appends the target triple when it
looks for each file and strips it in the installed package, so the installed names are
`rmedia-ffmpeg`, `rmedia-ffprobe`, `rmedia`, `rmedia-sr`, `rmedia-gifsicle` and `rmedia-gifski`,
next to `rmedia-desktop`. This is why
`fetch-tools`, `fetch-ncnn` and `stage-cli` must run before any
`tauri build` or `tauri dev`, and before `cargo build` of the desktop crate (tauri-build checks that
the files exist).

At run time, both `rmedia` and `rmedia-desktop` find the engine by looking next to their own
executable ([Engine location order](../administration/configuration.md#engine-location-order)).

### Resources

The license files are bundled as resources under `licenses/`: `THIRD_PARTY_NOTICES.md`,
`FFMPEG-NOTICE.md`, `FFMPEG-LICENSE.txt`, `NCNN-NOTICE.md`, `NCNN-LICENSE.txt`,
`GLSLANG-LICENSE.txt`, `GIFSICLE-NOTICE.md`, `GIFSICLE-COPYING.txt`, `GIFSKI-NOTICE.md`,
`GIFSKI-LICENSE.txt` and `GIFSKI-DEPS.md`. In the deb they install to `/usr/lib/rmedia/licenses/`.

The models are bundled as resources under `models/` (YuNet and the six Real-ESRGAN files; never
the test-only portrait), so `fetch-models` must also run before a desktop build. On Windows they
land next to the executables; in the deb and AppImage in `<prefix>/lib/rmedia/models`, which the
model search checks when the program runs from `<prefix>/bin`.

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
