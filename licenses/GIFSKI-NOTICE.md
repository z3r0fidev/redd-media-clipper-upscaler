# gifski notice

rmedia's "Max quality" GIF engine is **gifski 1.34.0**, a **separate program** renamed
`rmedia-gifski`. rmedia runs it as an independent process, sending it frames through a pipe; it
does not link to it. Its source is unmodified, built from the upstream crate below; only its
`Cargo.lock` moves the security updates listed in `xtask/gif-tools.lock.json` (`lock_updates`).

- Upstream: https://gif.ski/ and https://github.com/ImageOptim/gifski (Kornel Lesiński and
  contributors).
- Licence: **GNU Affero General Public License, version 3 or later** — the upstream licence
  file is `GIFSKI-LICENSE.txt`. This program comes with NO WARRANTY.
- The crates compiled into it are listed in `GIFSKI-DEPS.md`. Among them, gifski's colour
  quantiser **imagequant** (libimagequant) is licensed under the **GNU General Public License,
  version 3 or later**, which AGPL-3.0 §13 allows in the same program.
- Pinned in `xtask/gif-tools.lock.json` by SHA-256 (the crates.io checksum), and built by the
  rmedia workflow `.github/workflows/gif-tools.yml` (each binary carries a build attestation from
  it).

**Releases that bundle gifski wait for the application licence decision** (`cargo xtask
check-release-gate`, docs/roadmap.md).

## Build

From the extracted crate and the vendored dependency sources, with the `Cargo.lock` shipped in
the vendor tarball (the crate's, plus the listed updates), default features only (no FFmpeg is
linked):

```
cargo build --release --frozen --offline \
  --config 'source.crates-io.replace-with="vendored"' --config 'source.vendored.directory="vendor"'
```

Windows adds `RUSTFLAGS="-C target-feature=+crt-static"`.

## Corresponding source (AGPLv3 §6)

- Upstream crate: https://static.crates.io/crates/gifski/gifski-1.34.0.crate
  (SHA-256 `c246c795a61d4a2476fb1c8ab70bedfaa825c734882adc40e117fc837df81190`).
- The vendored dependency sources and the `Cargo.lock` the build used, as
  `gifski-1.34.0-vendor.tar.gz`.
- The rmedia project mirrors both on the `gif-tools-<date>` release that holds the built
  binaries; the exact URLs are recorded in `xtask/gif-tools.lock.json` when the build is pinned
  and listed here:

<!-- mirrored sources: cargo xtask pin-gif-tools -->
- https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/download/gif-tools-2026-10-25.40211502841/gifski-1.34.0.crate
- https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/download/gif-tools-2026-10-25.40211502841/gifski-1.34.0-vendor.tar.gz

On request, the rmedia maintainers will provide this source for at least three years after the
last distribution of a release that contains this binary.
