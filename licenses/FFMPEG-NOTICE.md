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

The complete corresponding source for the exact binaries shipped is:

1. FFmpeg at commit `c89d6ee1ca` (release branch 9.0, tag `n9.0.2` + 31 commits):
   https://git.ffmpeg.org/ffmpeg.git and https://github.com/FFmpeg/FFmpeg/commit/c89d6ee1ca
2. The build scripts and every third-party library version used (x264, x265, libvpx, opus, …) at
   https://github.com/BtbN/FFmpeg-Builds/tree/autobuild-2026-10-21-13-06

On request, the rmedia maintainers will provide this source for at least three years after the
last distribution of a release that contains these binaries. Before the first public release, a
copy of the source archive is mirrored alongside the rmedia release downloads (see
`specs/001-video-clipping/research.md` R9).

FFmpeg is a trademark of Fabrice Bellard, originator of the FFmpeg project.
