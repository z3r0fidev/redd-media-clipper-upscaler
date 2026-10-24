# gifsicle notice

rmedia installers and CLI archives include **gifsicle 1.98**, a **separate program** renamed
`rmedia-gifsicle`. rmedia runs it as an independent process to optimise the GIFs it makes; it
does not link to it. It is shipped unmodified, built from the upstream source below.

- Upstream: https://www.lcdf.org/gifsicle/ (Eddie Kohler and contributors).
- Licence: **GNU General Public License, version 2** — full text in `GIFSICLE-COPYING.txt`.
  This program comes with NO WARRANTY.
- Pinned in `xtask/gif-tools.lock.json` by SHA-256, and built by the rmedia workflow
  `.github/workflows/gif-tools.yml` (each binary carries a build attestation from it).

## Build

- Linux (ubuntu-22.04):
  `./configure --disable-gifview --disable-gifdiff CFLAGS="-O2 -fstack-protector-strong -D_FORTIFY_SOURCE=2 -fPIE" LDFLAGS="-pie -Wl,-z,relro,-z,now" && make`,
  then `strip src/gifsicle`.
- Windows (windows-latest, MSVC): `nmake -f Makefile.w32` in `src/`, compiled with
  `/O2 /MT /GS /guard:cf` and linked with `/DYNAMICBASE /NXCOMPAT /HIGHENTROPYVA /guard:cf`.

## Corresponding source (GPLv2 §3)

- Upstream tarball: https://www.lcdf.org/gifsicle/gifsicle-1.98.tar.gz
  (SHA-256 `e9d4cc0816db95c64d32e79f778836db0f1f7eb15868490bd330104e5146b66e`).
- The rmedia project mirrors the same file on the `gif-tools-<date>` release that holds the
  built binaries; the exact URL is recorded in `xtask/gif-tools.lock.json` when the build is
  pinned and listed here:

<!-- mirrored sources: cargo xtask pin-gif-tools -->

On request, the rmedia maintainers will provide this source for at least three years after the
last distribution of a release that contains this binary.
