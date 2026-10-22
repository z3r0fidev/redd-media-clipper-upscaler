# FFmpeg notice

rmedia installers include two **separate programs** from the FFmpeg project, renamed
`rmedia-ffmpeg` and `rmedia-ffprobe`. rmedia runs them as independent processes; it does not link
to FFmpeg libraries.

- Build: BtbN FFmpeg-Builds, `ffmpeg-n9.0.2-31-gc89d6ee1ca` (`linux64-gpl` / `win64-gpl`),
  release `autobuild-2026-10-21-13-06`, pinned with SHA-256 in `xtask/tools.lock.json`.
- Configuration: `--enable-gpl --enable-version3` (no `--enable-nonfree`).
- License: **GNU General Public License, version 3 or later** — full text in
  `FFMPEG-LICENSE.txt`. These programs come with NO WARRANTY.

## Corresponding source (GPLv3 §6)

The complete corresponding source for the exact binaries shipped is published by the rmedia
project, next to its downloads, as the release `ffmpeg-source-n9.0.2-31-gc89d6ee1ca`:

https://github.com/z3r0fidev/redd-media-clipper-upscaler/releases/tag/ffmpeg-source-n9.0.2-31-gc89d6ee1ca

It contains:

1. FFmpeg at commit `c89d6ee1ca` (release branch 9.0, tag `n9.0.2` + 31 commits), also at
   https://git.ffmpeg.org/ffmpeg.git and https://github.com/FFmpeg/FFmpeg/commit/c89d6ee1ca
2. The build scripts, BtbN FFmpeg-Builds at `autobuild-2026-10-21-13-06`, which name every
   third-party library version used (x264, x265, libvpx, opus, …).
3. The source of every one of those libraries, as the build scripts download it, with
   `SHA256SUMS` for all files.

On request, the rmedia maintainers will provide this source for at least three years after the
last distribution of a release that contains these binaries.

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.
