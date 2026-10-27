# Bundled GIF engines: pins, build, notices, release gate

This covers the owner decision of 2026-10-23: bundle gifsicle and gifski. The pin file follows
the FFmpeg pin (`xtask/tools.lock.json`) and the mirror and `check-pins` rules in
`docs/operations/build-and-release.md`. The security review of the design (2026-10-23) shaped
the build and integrity chain below.

## `xtask/gif-tools.lock.json`

```json
{
  "comment": "Pinned GIF engines bundled with rmedia, built from source by .github/workflows/gif-tools.yml. Refresh with `cargo xtask pin-gif-tools <run-id>`.",
  "release": "gif-tools-2026-10-24",
  "build_run": 1234567890,
  "build_commit": "<40-hex commit on main>",
  "gifsicle": {
    "version": "1.98", "license": "GPL-2.0-only",
    "source": { "url": "https://www.lcdf.org/gifsicle/gifsicle-1.98.tar.gz",
                "mirrors": ["https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/download/gif-tools-2026-10-24/gifsicle-1.98.tar.gz"],
                "sha256": "e9d4cc0816db95c64d32e79f778836db0f1f7eb15868490bd330104e5146b66e" },
    "targets": {
      "x86_64-unknown-linux-gnu": { "url": "…/releases/download/gif-tools-2026-10-24/gifsicle-1.98-x86_64-unknown-linux-gnu", "sha256": "…" },
      "x86_64-pc-windows-msvc":   { "url": "…/releases/download/gif-tools-2026-10-24/gifsicle-1.98-x86_64-pc-windows-msvc.exe", "sha256": "…" }
    }
  },
  "gifski": {
    "version": "1.34.0", "license": "AGPL-3.0-or-later",
    "source": { "url": "https://static.crates.io/crates/gifski/gifski-1.34.0.crate",
                "mirrors": ["…/gif-tools-2026-10-24/gifski-1.34.0.crate"],
                "sha256": "c246c795a61d4a2476fb1c8ab70bedfaa825c734882adc40e117fc837df81190" },
    "cargo_lock_sha256": "…",
    "lock_updates": ["crossbeam-epoch 0.9.18 -> 0.9.20 (RUSTSEC-2026-0204)"],
    "vendor": { "url": "…/gif-tools-2026-10-24/gifski-1.34.0-vendor.tar.gz", "mirrors": ["…"], "sha256": "…" },
    "targets": { "…": "as above" }
  }
}
```

Rules, enforced by `check-pins`:
- Every `sha256` is 64 lowercase hex characters, and every URL is https.
- `release` matches `^gif-tools-\d{4}-\d{2}-\d{2}$`.
- `build_run` is a positive integer, and `build_commit` is 40 hex characters.
- Each built-binary `url` points at `…/releases/download/<release>/`.
- Every `source` and the gifski `vendor` archive have at least one mirror.
- With `--online`, every URL, built binaries included, answers a HEAD request.
- `all_pins`, `archive_all`, `mirror` and `check-pins` read this file as well as `tools.lock.json`
  and the models lock. A same-name asset that already exists on a mirror release must have the
  same bytes.

## Pin time: `cargo xtask pin-gif-tools <run-id>`

This runs locally and records nothing until every check below passes.
1. The run belongs to `gif-tools.yml`. It succeeded, and its `head_sha` is on `main`; that sha
   becomes `build_commit`.
2. For each release asset, `gh attestation verify --repo <repo> --signer-workflow
   .github/workflows/gif-tools.yml` succeeds. Its digest also equals the hash the build job put
   in its `outputs`, read through the Actions API, and the digest computed after download.
3. The gifski `.crate` sha256 equals the checksum in the crates.io index entry, fetched at pin
   time.
4. For gifsicle, the release tarball is compared once against an independent copy (Debian's orig
   tarball, when one exists for this version). Debian's is a snapshot of the upstream repository,
   not the release tarball, so the hashes never match and the unpacked trees are compared: every
   file in both must be identical, and the release may add only autotools output (`configure`,
   `aclocal.m4`, `Makefile.in`, …, by exact path), which this check does not cover. Debian's
   tarball is scanned for links and escaping paths before it is unpacked. Without a Debian copy
   the pin fails unless `--without-debian` is given. The result is recorded in the commit
   message.
5. gifski's `Cargo.lock` is the crate's plus the security updates in `GIFSKI_LOCK_UPDATES`
   (gif-tools.yml) / `lock_updates` (the lock file); the lock used ships in the vendor tarball,
   and pinning fails unless it differs from the crate's in exactly those packages and has only
   crates.io sources. Every vendored package is compared file by file with its `.crate`
   downloaded from crates.io and checked against the lock's checksum. That lock is
   checked with `cargo deny check` (a `deny-gifski.toml` that allows gifski's own licence set)
   and with `cargo audit`. The sha256 of the crate's own lock becomes `cargo_lock_sha256`.
6. `licenses/GIFSKI-DEPS.md` is generated from that lock: the name, version and licence of every
   dependency compiled into the binary (L3).

The lock change is a normal reviewed commit.

## Workflow `.github/workflows/gif-tools.yml` (manual dispatch, `main` only)

- Top-level `permissions: contents: read`.
- `concurrency: gif-tools` (no parallel publishes).
- `timeout-minutes` on every job.
- Actions pinned by commit SHA.
- Downloads use `curl --proto =https --proto-redir =https`.
- Every archive is sha256-verified before extraction, and refused if any member path is absolute
  or has a `..` component.

1. **`vendor`** (ubuntu-24.04, `contents: read`, no secrets, no `actions/cache`):
   - Verify the `.crate` and extract it.
   - Fail if it contains `.cargo/`, `rust-toolchain`/`rust-toolchain.toml`, or a `Cargo.lock`
     whose sha256 differs from the pin. The pin's lock hash is checked on every run after the
     first pin.
   - Run `cargo vendor --locked` with a fresh `CARGO_HOME`, and upload the vendor tarball and
     its hash.
   - This is the only job that talks to crates.io.
2. **`build`**: a matrix of ubuntu-22.04 and windows-latest, `contents: read` only. No secrets,
   no `id-token`, no `attestations`, no caches.
   - **gifsicle:**
     - Linux:
       ```
       ./configure --disable-gifview --disable-gifdiff \
         CFLAGS="-O2 -fstack-protector-strong -D_FORTIFY_SOURCE=2 -fPIE" \
         LDFLAGS="-pie -Wl,-z,relro,-z,now"
       make && strip
       ```
     - Windows: `nmake -f Makefile.w32` with `/O2 /MT /GS /guard:cf` and link flags
       `/DYNAMICBASE /NXCOMPAT /HIGHENTROPYVA /guard:cf` (research R8).
   - **gifski:**
     - Build from the extracted crate plus the vendor tarball:
       `cargo build --release --frozen --offline`.
     - Use a fresh `CARGO_HOME`, `RUSTUP_TOOLCHAIN` set to the repo toolchain, and
       `--config 'source.crates-io.replace-with="vendored"' --config 'source.vendored.directory="vendor"'`.
     - Default features only.
     - Windows adds `-C target-feature=+crt-static`.
   - Smoke test:
     - `--version` matches the pin.
     - A 10-frame self-generated lavfi GIF goes through `gifsicle -O3 --lossy=40`, and through
       `gifski` from y4m.
     - PIE (Linux) and DYNAMICBASE/NXCOMPAT (Windows) are confirmed.
   - The **last step** hashes each binary and exposes the hashes as job `outputs`, not inside
     the artifact. The artifact holds the binaries and the licence texts.
3. **`attest`** (`needs: build`, `id-token: write`, `attestations: write`; it runs no build
   code):
   - Download the artifact, and recompute the hashes against `needs.build.outputs`.
   - Run `actions/attest-build-provenance` for each binary.
   - What this proves: "built by this workflow at this commit".
4. **`publish`** (`needs: [vendor, build, attest]`, `contents: write`; it runs no build code):
   - Recompute the hashes against the outputs.
   - Create the release `gif-tools-<date>` (`gif-tools-<date>.<run-id>` for a second build that day) with the binaries, both sources, the vendor tarball
     and the licence texts.

## Fetch and staging

- `cargo xtask fetch-tools [--target T]` also stages `binaries/rmedia-gifsicle-<triple>[.exe]`
  and `binaries/rmedia-gifski-<triple>[.exe]`. They are verified against the pins, cached under
  `archives/<sha256>/`, and get a stamp file, as FFmpeg does.
- Tauri `externalBin` adds `../../../binaries/rmedia-gifsicle` and `rmedia-gifski`.
- The release CLI archive copies both, without the triple suffix.

## Notices

- `licenses/GIFSICLE-NOTICE.md` and `licenses/GIFSKI-NOTICE.md` each give:
  - the version and licence;
  - the upstream homepage;
  - the mirrored corresponding source with its sha256: for gifski, the `.crate` **and** the
    vendor tarball;
  - a statement that the program is shipped unmodified as a separately executed program;
  - the exact build command.
- The licence texts are `licenses/GIFSICLE-COPYING.txt` (GPL-2.0) and
  `licenses/GIFSKI-LICENSE.txt` (AGPL-3.0).
- `licenses/GIFSKI-DEPS.md` attributes the crates compiled into gifski.
- `check-pins` fails if a notice does not name the pinned version and the mirrored source URLs,
  or if `GIFSKI-DEPS.md` is missing.
- `THIRD_PARTY_NOTICES.md` links all three.

## Release gate (FR-025): `cargo xtask check-release-gate`

- **Who calls it:** the first step of **every** `release.yml` job, for both triggers (tag push
  and `workflow_dispatch`, since downloadable artifacts are distribution too). It runs again
  after staging, and once more after packaging with `--scan apps/desktop/src-tauri/target/release
  --scan dist`.
- **What it checks:** whether `binaries/rmedia-gifski*` is staged. It checks the staged file,
  not the lock, so removing the engine from the lock also removes the need for the gate. With
  `--scan DIR`, it also checks the built output: any file under DIR named after gifski (build
  intermediates such as `deps/` are skipped), or an NSIS/WiX script (`*.nsi`, `*.wxs`, UTF-8 or
  UTF-16) that names it. Such a find needs the same decision ("gifski (AGPL-3.0) is in the
  build (<file>) but …").
- **When it fails:** gifski is staged and either of these is missing:
  - a root `LICENSE` file;
  - `[workspace.metadata.rmedia] app_licence = "<SPDX>"` in the root `Cargo.toml`, naming an id
    from the gate's allowlist (`AGPL-3.0-only`, `AGPL-3.0-or-later`, `GPL-3.0-only`,
    `GPL-3.0-or-later`, or a `commercial-gifski-imagequant` marker for bought gifski and
    imagequant licences: imagequant, compiled into gifski, is GPL-3.0-or-later on its own). The owner
    edits this allowlist only together with the licence decision.
- **Message:** "gifski (AGPL-3.0) is staged but the application licence is undecided
  (docs/roadmap.md)".
- **Tests:** unit tests in xtask cover a staged engine with no decision, one with a decision,
  and an unstaged engine. A CI step runs the gate against a temp tree with a fake staged file
  and asserts that it fails.
- **Until the owner decides:** release-workflow artifacts can't be built with gifski staged.
  Dropping gifski from staging (`fetch-tools --no-gifski`) is the documented way to produce
  artifacts meanwhile. CI is unaffected, because it doesn't run `release.yml`.
