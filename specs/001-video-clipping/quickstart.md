# Quickstart & Validation: Video Clipping Base

Proves FR/SC coverage end to end. Commands are for Linux bash; on Windows use PowerShell
equivalents (CI runs the automated parts on both).

## Prerequisites
- Rust 1.94 (`rust-toolchain.toml`), Node 24, FFmpeg ≥ 6.0 on PATH **or** `cargo xtask fetch-tools`
  (downloads the pinned bundled build into `binaries/`).
- Linux GUI deps: webkit2gtk-4.1, librsvg2, libayatana-appindicator3 (`*-dev`).
- `cargo install tauri-cli --version "^2" --locked` (GUI only).

## 1. Automated suite
```bash
cargo xtask fetch-tools          # optional: use the shipped engine
cargo test --workspace           # unit + integration (lavfi fixtures) + CLI tests
cargo test --workspace -- --ignored bench   # SC-001 / SC-003 timing benches
(cd apps/desktop && npm ci && npm test)     # frontend stores
```
Expected: all green. Fixtures are generated under `target/rmedia-fixtures/` (not committed).

## 2. CLI scenarios (manual smoke)
```bash
ffmpeg -v error -f lavfi -i testsrc2=size=1280x720:rate=25:d=60 -f lavfi -i sine=d=60 \
       -c:v libx264 -g 50 -c:a aac -shortest /tmp/t.mp4
cargo run -p rmedia-cli -- doctor                                         # FR-026
cargo run -p rmedia-cli -- clip /tmp/t.mp4 -s 10.5 -e 20                  # US1 fast → reports start 10.0
cargo run -p rmedia-cli -- clip /tmp/t.mp4 -s 10.5 -d +5 --mode accurate  # US1 accurate → 5.000 s
printf '0:05 0:10 a\n0:20 +3 b\n' > /tmp/c.txt
cargo run -p rmedia-cli -- clip /tmp/t.mp4 -l /tmp/c.txt --dry-run --json # US2/FR-019, nothing written
printf '0:05 0:1x\n' > /tmp/bad.txt
cargo run -p rmedia-cli -- clip /tmp/t.mp4 -l /tmp/bad.txt; echo $?      # → line 1 error, exit 3
cargo run -p rmedia-cli -- scan ~/Videos --json                          # US4
cargo run -p rmedia-cli -- batch specs/001-video-clipping/contracts/examples/jobs.csv --dry-run  # US3
```
Check: outputs in `/tmp/clips/`, durations via `ffprobe -show_entries format=duration`, source
checksum unchanged (`sha256sum /tmp/t.mp4` before/after).

## 3. Cancellation (SC-007)
Start an accurate clip of a long 1080p source, press Ctrl-C once: summary printed, exit 130, no
`*.part` in output dir, `pgrep -f rmedia-ffmpeg` empty within 2 s.

## 4. Desktop (US5)
```bash
cd apps/desktop && npm ci && cargo tauri dev
```
Add `/tmp/t.mp4` by drag-and-drop → mark in/out with I/O keys → add 2 clips → Run → verify
progress and outputs; start a long run and Cancel all → nothing partial left. Export queue as
manifest and run it via `rmedia batch` → identical output hashes (SC-008). Try an MKV/HEVC source to
exercise the proxy preview path.

## 5. Packaging (FR-027)
CI artifacts: Windows NSIS/MSI, Linux AppImage/deb. On a clean VM: install → `doctor` reports
`source: bundled` → first clip in < 5 min (SC-005). Notices present in install dir.
