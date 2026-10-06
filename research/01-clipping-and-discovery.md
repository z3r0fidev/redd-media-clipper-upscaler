# 01 - Clipping and Discovery Research

Project: cross-platform (Linux + Windows) video clipper, CLI + native GUI.
Written: 2026-10-06. Compiled from background knowledge of FFmpeg docs/behavior (no live web verification this session). Items marked **[verify]** are version- or date-sensitive and should be re-checked before being relied upon. Sources are listed per section.

---------------------------------------------------------------------------

## 1. FFmpeg clipping techniques

### 1.1 Stream copy (`-c copy`) vs re-encode

| Aspect | `-c copy` | Re-encode |
|---|---|---|
| Speed | Near disk speed, no CPU load | Real-time-ish or slower, CPU/GPU bound |
| Quality | Bit-identical to source | Generational loss (mitigate with low CRF) |
| Cut accuracy | Only at keyframes (video) | Frame-accurate |
| Container/codec flexibility | Must be compatible with output container | Any |
| Filters (crop, scale, burn subs) | Impossible | Possible |

Stream copy cannot start decoding mid-GOP: a P/B frame depends on prior reference frames. With `-c copy`, ffmpeg starts at the keyframe at-or-before the requested start (when seeking before `-i`) and the output may contain leading frames that are not displayed (edit list / negative timestamps) or, in players that ignore edit lists, a frozen/gray/garbled lead-in. The end cut is also keyframe/packet-granular: `-to`/`-t` stops at the first packet beyond the end, so end is accurate to roughly one frame for all-intra-ish content but the *start* is the main issue for long-GOP H.264/H.265 (typical GOP 2-10 s, sometimes 250 frames).

### 1.2 `-ss` placement: input vs output seeking

- **`-ss` before `-i` (input seeking)**: demuxer seeks to nearest keyframe at or before the target. Fast (no decode of skipped data). Since FFmpeg 2.1 (2013), when transcoding, ffmpeg then decodes from that keyframe and discards frames until the exact target, so it is **frame-accurate when re-encoding** and fast. With `-c copy`, there is no decode so it stays keyframe-snapped. Timestamps are reset so output starts near 0.
- **`-ss` after `-i` (output seeking)**: decodes and discards everything up to the target. Accurate but slow for far offsets; with `-c copy` it effectively still starts from the container start and drops packets until time, producing keyframe-related artifacts; generally not recommended for copy.
- `-to` vs `-t`: `-to` is an end position, `-t` a duration. Caveat: with `-ss` as an *input* option, `-to` as an *output* option is relative to the new (reset) timeline in modern ffmpeg, i.e. `-ss 60 -i f -to 70` yields 70 s of output in older documented behavior discussions - this is a notorious confusion. Safest: compute `duration = end - start` in the app and always pass `-t duration`. Put `-ss` before `-i` and `-t` as an output option (or `-to` as an input option next to `-ss`: `-ss A -to B -i f` is allowed and means A..B in source time).
- `-accurate_seek` is default on; `-noaccurate_seek` disables the decode-and-discard behavior (used with copy to keep leading frames).
- `-seek_timestamp 1` makes `-ss` use the file's timestamps rather than starting at 0 (matters for files with a non-zero start_time, e.g. MPEG-TS and some broadcasts).

### 1.3 Timestamp flags

- `-avoid_negative_ts make_zero` / `make_non_negative`: shifts timestamps so none are negative; needed for copy cuts so players do not see negative DTS from preroll packets. `make_zero` is the usual choice for clipping to a fresh file. For MP4 muxer, default behavior has changed across versions (mp4 uses `make_non_negative` handling and writes edit lists); **[verify]** on the ffmpeg version you bundle.
- `-copyts`: preserve source timestamps instead of rebasing to zero. Useful if you need to cut by absolute source PTS or concatenate later; combine with `-start_at_zero` if you want ffmpeg to still shift. Combining `-copyts` with `-ss` input seeking and `-to` makes `-to` refer to source timeline (this is the clean way to get "absolute" end semantics). Downside: output has large start offset unless fixed with `-avoid_negative_ts make_zero`/`-start_at_zero`.
- `-fflags +genpts` helps with sources lacking PTS (raw H.264, some TS/AVI). `-fflags +discardcorrupt` for damaged inputs.
- `-map 0` (all streams) instead of default one-best-per-type if you want to preserve extra audio tracks, subtitles, chapters (`-map_chapters` is default 0 from input, but clipped chapters may be wrong; consider `-map_chapters -1`).
- `-movflags +faststart` for MP4 outputs meant for web/Reddit-style upload; has a second-pass cost but is a cheap file rewrite.

### 1.4 Frame-accurate "smart cut" (hybrid)

Goal: keep original quality for most of the clip, re-encode only what is necessary.

Algorithm (what LosslessCut "experimental smart cut" and tools like `vcut`/`smartcut`/Avidemux do):
1. Probe keyframes in range: `ffprobe -v error -select_streams v:0 -skip_frame nokey -show_entries frame=pts_time -of csv=p=0 -read_intervals START%END file` (or `-show_entries packet=pts_time,flags` and filter `K`).
2. Find first keyframe K1 >= start and last keyframe K2 <= end (for end, the segment after K2 must be re-encoded up to end only if end falls mid-GOP; the typical hybrid re-encodes the head [start, K1) and optionally the tail [K2', end)).
3. Produce up to 3 parts: head (re-encode start..K1), middle (`-c copy` K1..K2), tail (re-encode K2..end).
4. Concatenate with concat demuxer (`-f concat -safe 0`, `-c copy`).

Hard parts and pitfalls:
- The re-encoded segments must match the source stream parameters exactly (codec, profile, level, resolution, SAR, pix_fmt, frame rate/timebase, bit depth, SPS/PPS compatibility, number of reference frames, closed vs open GOP) so the concatenated stream decodes without glitches. H.264 SPS/PPS differences can break some decoders; x264 can be coaxed with `-x264-params` (`keyint`, `ref`, `bframes`, `repeat-headers`/`stitchable=1`) but cannot reproduce the original encoder's exact parameter sets in general. Hardware-encoded or exotic sources (interlaced, 10-bit, HEVC with open GOP/CRA leading pictures) are risky.
- Open-GOP / leading pictures (RASL in HEVC, B-frames referencing prior GOP in H.264) at the copy boundary: the first keyframe may be an I-frame whose following B-frames reference earlier frames; cutting at it needs those frames dropped or re-encoded. Use IDR-only boundaries or re-encode a bit more.
- Audio: cut audio at sample/packet precision in its own pass (AAC priming/encoder delay ~1024 samples; MP3 ~1105). Re-encoding audio only (cheap) is a common compromise: `-c:v copy -c:a aac` when audio boundaries glitch.
- Pragmatic production approach: ship "smart cut" as optional, auto-fallback to full re-encode when stream parameters or container are risky (non-H.264/HEVC, interlaced, open-GOP, VFR, etc).

Existing implementations to study: LosslessCut (smart cut experimental since v3.50-ish **[verify]**), `smartcut` (Python, skeskinen/smartcut), `ffmpeg-smart-cut` style scripts, Avidemux copy mode with "smart copy" for MPEG-2/H.264, `mkvmerge --split parts:` (keyframe-only), VideoReDo (commercial; the gold standard for MPEG-2/H.264 smart rendering), `cutlist` ecosystems.

### 1.5 Audio sync, subtitles, VFR

- **Audio sync with copy cuts**: video starts at keyframe before target while audio can start exactly at target; the difference shows as A/V offset or leading video. `-avoid_negative_ts make_zero` + keeping both streams starting at the same keyframe-aligned time (i.e. seek with `-ss` before `-i` and let both streams start at the same demux point) normally keeps sync. Do not mix input `-ss` on one input and output `-ss` on another unless intentional. For MP4 sources with edit lists, check `ffprobe -show_entries stream=start_time` for per-stream offset.
- **Re-encode audio sync**: `-af aresample=async=1:first_pts=0` fixes small gaps; `-async` is deprecated. Use `-vsync`/`-fps_mode` (renamed: `-vsync` deprecated in favour of `-fps_mode` since FFmpeg 5.1) `passthrough` for VFR-preserving copy-like timing, `cfr` to force constant frame rate, `vfr` to drop/dup minimally. Default for mp4 muxer is auto -> may duplicate/drop frames; for exact timing on VFR sources prefer `-fps_mode passthrough` unless a constant frame rate is required.
- **VFR sources** (phone recordings, screen capture, OBS): frame-number-based timestamps (`frame 1234`) are ambiguous. Convert frame -> time using the actual PTS list rather than `frame/fps`; use `r_frame_rate` vs `avg_frame_rate` from ffprobe, and warn when they differ materially. Re-encode to CFR only if the target platform demands it.
- **Subtitles**: Text subtitles (SRT/ASS/mov_text/WebVTT) are cut by time and copy OK into compatible containers (MKV accepts all; MP4 accepts `mov_text` only - convert with `-c:s mov_text`). Bitmap subs (PGS, DVD VOBSUB) copy but only at packet granularity; start-of-clip subtitle events that began before the clip start are lost with copy (ASS "events spanning the cut"). To burn in: re-encode with `-vf subtitles=file.srt` or `-filter_complex` overlay (needs `-ss` handling: use `-copyts` with `-vf subtitles` or `setpts=PTS+START/TB` trick, since subtitles filter reads timestamps from the stream start). Provide option `--subs {keep,drop,burn}`.
- Attachments (fonts in MKV) -> `-map 0:t?`; data streams (`-map 0:d?`) often unsupported in MP4: use `-map 0:v:0 -map 0:a? -map 0:s? -dn` as the default conservative map.

### 1.6 Recommended command templates

Fast (copy) mode:
```
ffmpeg -hide_banner -nostdin -y -ss START -to END -i IN -map 0:v:0? -map 0:a? -map 0:s? -dn -c copy -avoid_negative_ts make_zero -movflags +faststart OUT.mp4
```
Accurate mode (default encoder libx264, or hardware if selected):
```
ffmpeg -hide_banner -nostdin -y -ss START -i IN -t DURATION -map 0:v:0 -map 0:a? -c:v libx264 -crf 18 -preset veryfast -pix_fmt yuv420p -c:a aac -b:a 192k -movflags +faststart OUT.mp4
```
(`-ss` before `-i` + re-encode is accurate. Use `-t` duration to sidestep `-to` semantics.)

Probe preflight: `ffprobe -v error -show_format -show_streams -of json IN` (cache result); keyframe listing as in 1.4.

### 1.7 Recommended strategy

1. **Default "fast" mode**: stream copy with input seek, snap start to the previous keyframe (or optionally next), show the user the actual snapped start/end and the resulting error in the UI/CLI log. Warn when snap error > threshold (e.g. > 0.5 s).
2. **"accurate" mode**: re-encode (H.264 CRF 18, or source codec when possible; audio AAC 192k or copy where safe) - always frame-accurate.
3. **"smart" mode** (phase 2): hybrid head/tail re-encode + copy middle, auto-fallback to accurate on parameter mismatch; copy-then-validate with ffprobe (frame count, duration drift).
4. Always pass `-nostdin`, `-map` explicitly, `-avoid_negative_ts make_zero`, never overwrite inputs, write to temp file then rename (atomic).

Sources:
- FFmpeg docs: https://ffmpeg.org/ffmpeg.html (options `-ss`, `-to`, `-t`, `-accurate_seek`, `-seek_timestamp`, `-copyts`, `-avoid_negative_ts`, `-fps_mode`)
- FFmpeg Wiki Seeking: https://trac.ffmpeg.org/wiki/Seeking
- FFmpeg Wiki Concatenate: https://trac.ffmpeg.org/wiki/Concatenate
- FFmpeg formats docs (segment, concat, mp4 muxer): https://ffmpeg.org/ffmpeg-formats.html
- LosslessCut smart cut/issues notes: https://github.com/mifi/lossless-cut (see docs/ and issue "smart cut")
- smartcut: https://github.com/skeskinen/smartcut
- ffprobe docs `-read_intervals`: https://ffmpeg.org/ffprobe.html

---------------------------------------------------------------------------

## 2. Timestamp formats to accept

### 2.1 Formats

| Format | Examples | Notes |
|---|---|---|
| Clock `HH:MM:SS[.fraction]` | `01:02:03.500`, `1:02:03` | Fraction 1-9 digits; also accept `,` as decimal (SRT style `00:01:02,500`) |
| `MM:SS[.fraction]` | `02:03.5`, `2:03` | 2 fields = minutes:seconds; 1 field = seconds |
| Plain seconds | `90`, `90.25`, `1.5e2`(reject exp) | Decimal; `.5` and `5.` allowed |
| Unit suffix | `1h2m3.5s`, `90s`, `1500ms` | ffmpeg-style `-ss` accepts `[-][HH:]MM:SS[.m...]` or `[-]S+[.m...][s|ms|us]` (see ffmpeg-utils "Time duration") |
| Frames | `f1234`, `1234f`, `00:01:02+15f` | Requires fps; for VFR resolve through PTS table |
| SMPTE timecode | `01:02:03:15` (NDF) or `01:02:03;15` (DF) | `;` or `.` before frames signals drop-frame (29.97/59.94); must know fps; start TC offset of file (`timecode` tag) |
| Percent / relative | `50%`, `end`, `end-10s`, `+30s` | Optional sugar; `end` resolved from duration |
| Duration instead of end | `start=10 duration=30`, `10+30`, `10..+30` | Mutually exclusive with `end` |
| Negative (from end) | `-30` meaning 30 s before end | Optional; distinguish from ffmpeg option semantic |

### 2.2 Parsing edge cases

- Disambiguate 4 colon-separated fields as SMPTE (HH:MM:SS:FF), never as HH:MM:SS:mmm. Provide explicit `--tc-mode`. Require fps for SMPTE/frames or probe it.
- Drop-frame math: 29.97 DF drops frame numbers 00 and 01 at each minute not divisible by 10; 59.94 DF drops 4. Implement via the standard formula; do not apply DF to 23.976 / 24 / 25 / 30 (NDF only).
- Use exact rational arithmetic (`Fraction` / i64 numerators over timebase) internally, not floats: 29.97 = 30000/1001. Convert to string only at ffmpeg invocation; cap to microseconds (ffmpeg's internal AV_TIME_BASE = 1e6).
- Fractions with more than 3 digits are legal; reject but do not round silently beyond microseconds; warn when `.mmm` is ambiguous (`1:02.5` = 2.5 s, not 5 ms).
- Minutes/seconds > 59 in non-leading field: reject (`01:75:00`) but allow overflow in leading field (`90:00` = 90 min? ambiguous: define as MM:SS so 90 min). Decide and document: leading field may overflow.
- Whitespace, unicode digits, BOM, locale decimal commas (`1,5`) - accept comma only in the fraction position of clock format.
- Validation: start >= 0; end > start; start < media duration (clamp end to duration with warning; error for start beyond duration); zero-length clips error; overlapping clips allowed (independent outputs).
- Start-time offset: some containers (TS, broadcast MP4) have non-zero `start_time`; decide whether user times are relative to 0 or to container start (default: relative to first PTS, i.e. 0-based as players display).
- Both ends open: `00:30 -` = from 30 s to EOF; `- 01:00` = from start to 1 min.
- Output of parser: `{start: Rational, end: Rational|None, duration: Rational|None, source_format}`; keep a normalized `HH:MM:SS.mmm` pretty-printer for logs and filenames (use `-` or `_` instead of `:` on Windows filenames).

Sources: FFmpeg utils "Time duration" https://ffmpeg.org/ffmpeg-utils.html#Time-duration ; SMPTE drop-frame explainer https://en.wikipedia.org/wiki/SMPTE_timecode ; EDL timecode in CMX3600 spec (see 3.5).

---------------------------------------------------------------------------

## 3. Text-file formats: multi-clip and batch manifests

### 3.1 Comparison

| Format | Pros | Cons | Fit |
|---|---|---|---|
| Plain line (`00:10 00:25 name`) | Trivial to hand-write, diff-friendly, copy-paste from notes/YouTube chapters | Weak typing, no per-clip options, quoting of names/paths | Great for **one video, many clips** |
| CSV/TSV | Spreadsheet editing, easy batch export; LosslessCut CSV compat | Quoting, no nesting, header conventions | Good for **batch** where each row has a source path |
| YAML | Comments, readable, nested defaults per video | Whitespace/type pitfalls (the Norway problem `no`->false, `1:30` -> sexagesimal int in YAML 1.1!) | Good human-edited manifest; must force strings |
| JSON | Strict, tooling/schema, easy GUI round trip | No comments, verbose, hand-editing painful | Best **machine/GUI interchange** and canonical internal form |
| TOML | Comments, strict types, no sexagesimal trap | Nested arrays-of-tables verbose | Alternative to YAML |
| EDL (CMX3600) | NLE interchange | Frame-based, reel names, fps context, limited metadata | Import only |
| ffmpeg concat list (`file 'x'` / `inpoint` / `outpoint`) | Native to ffmpeg | Meant to join, not to name outputs | Interop reference |
| ffmpeg segment muxer `-segment_times` | Native splitting | Keyframe-bound, single input | Reference only |

Note YAML 1.1 pitfall: unquoted `1:30:00` or `12:30` may parse as base-60 integer in PyYAML (YAML 1.1). `ruamel.yaml` (1.2) or `serde_yaml` handle it differently; require quoting or always accept ints/floats as seconds. This is a significant reason to prefer JSON/TOML for canonical form.

### 3.2 Existing conventions

- **LosslessCut**: project file `.llc` (JSON: `version`, `mediaFileName`, `cutSegments` array of `{start, end, name, tags}`) and CSV import/export with columns `start,end,label` (seconds, decimal; empty end = to end of media) per its docs "Export/import segments" (also supports DaVinci Resolve/Final Cut Pro XML, CUE, EDL-ish, YouTube chapter text import "timestamps"). **[verify]** exact field names in current release. Emulating `start,end,label` CSV gives free interop.
- **YouTube chapters** text: `0:00 Intro` lines (start + title; end = next start). Worth supporting as an import mode (`--chapters-style`).
- **SRT/VTT/CUE**: possible import sources for cut lists.
- **EDL CMX3600**: lines like `001  AX  V  C  00:00:10:00 00:00:25:00 00:00:00:00 00:00:15:00` (event, reel, track, transition, src-in, src-out, rec-in, rec-out), FCM header `FCM: DROP FRAME` / `NON-DROP FRAME`, `* FROM CLIP NAME:` comments. Supports import where src-in/src-out are SMPTE and need fps.
- **ffmpeg concat demuxer**: `file 'path'`, `inpoint 10.0`, `outpoint 25.0` - can be exported by the app so users can recreate the cut with plain ffmpeg.

### 3.3 Recommended formats

Support three input styles, normalized to one internal model:
1. **Line format** (per-video clip list): `START END [name...]` or `START +DURATION [name]`, `#` comments, blank lines ignored, optional `key=value` tail options. Separator: whitespace, `-`, `,`, or `->`; first two tokens are times, remainder is label.
2. **CSV/TSV** (batch + LosslessCut-compatible): header row required for batch.
3. **JSON (and YAML as a superset convenience)**: full-featured manifest with defaults and per-video clips. Canonical.

### 3.4 Recommended schema (manifest v1)

Internal model: `Manifest { version, defaults, jobs[] }`, `Job { input, clips[], overrides }`, `Clip { start, end|duration, name?, mode?, output?, tags? }`.

JSON/YAML example:
```yaml
version: 1
defaults:
  mode: fast            # fast | accurate | smart
  container: mp4
  output_dir: ./clips
  name_template: "{stem}_{index:03d}_{start}-{end}"
  audio: copy           # copy | aac | drop
  subs: keep            # keep | drop | burn
  on_exists: skip       # skip | overwrite | rename
videos:
  - input: "/media/a/game1.mkv"
    clips:
      - { start: "00:01:10.500", end: "00:01:42", name: "ace" }
      - { start: "00:05:00", duration: "30", name: "clutch", mode: accurate }
  - input: "D:\\media\\b\\match2.mp4"
    clips:
      - { start: "12:00", end: "12:45" }
```
CSV (batch) example:
```
input,start,end,duration,name,mode,output
/media/a/game1.mkv,00:01:10.5,00:01:42,,ace,fast,
/media/a/game1.mkv,00:05:00,,30,clutch,accurate,
D:\media\b\match2.mp4,12:00,12:45,,,,
```
Line format (single video, passed with `--input game1.mkv --clips clips.txt`):
```
# start    end        name
00:01:10.5 00:01:42   ace
00:05:00   +30        clutch   mode=accurate
1:12:00    end        outro
```
Rules: relative paths resolve against manifest directory (not CWD); UTF-8 (with/without BOM), LF/CRLF; `~` and env vars not expanded unless asked; per-row options override job overrides override defaults; unknown keys = error under `--strict`, warning otherwise; schema published as JSON Schema; `--validate`/`--dry-run` prints the resolved plan (snapped times, output paths) without running.

Output naming: sanitize for Windows (`<>:"/\|?*`, reserved names CON/PRN/NUL..., trailing dot/space, 260-char path limit unless long-path manifest enabled), guarantee uniqueness, deterministic.

Sources:
- LosslessCut docs/segments: https://github.com/mifi/lossless-cut/blob/master/import-export.md **[verify path]**
- FFmpeg concat demuxer: https://ffmpeg.org/ffmpeg-formats.html#concat
- FFmpeg segment muxer: https://ffmpeg.org/ffmpeg-formats.html#segment_002c-stream_005fsegment_002c-ssegment
- CMX3600 EDL: https://xmil.biz/EDL-X/CMX3600.pdf ; overview https://en.wikipedia.org/wiki/Edit_decision_list
- YAML 1.1 sexagesimal: https://yaml.org/type/int.html
- JSON Schema: https://json-schema.org/

---------------------------------------------------------------------------

## 4. Directory scanning and video detection

### 4.1 What "video archive" probably means

Ambiguous in the brief. Likely interpretations, in order of probability:
1. **A folder/collection of stored video files** (an "archive" of recordings, e.g. downloaded Reddit media / stream VODs). Scan directory -> list video files. Most likely, given "Given an input directory it must scan and identify video files".
2. A **container archive file** (zip/rar/7z/tar) that holds videos.
3. A **video archive format** such as a segmented recording (HLS playlist + .ts segments, DASH, DVD VIDEO_TS/BDMV structure, camera "AVCHD" folders, DAV/NVR proprietary dumps).

Recommendation: implement (1) fully; treat (3) as "multi-file logical videos" detected via known layouts (`.m3u8`, `VIDEO_TS`, `BDMV`) in a later phase; treat (2) as opt-in: list video entries inside zip/7z/tar and extract to temp (ZIP supports random access; RAR needs unrar/7z; **stored-method** archives with no compression could be streamed/seekable but not generally). Clarify with the product owner; surface a `--archives` flag rather than default behavior. Never extract by default (zip-slip: sanitize entry paths; guard against zip bombs with size/ratio limits).

### 4.2 Detection strategies

| Method | Cost | Accuracy | Notes |
|---|---|---|---|
| Extension allowlist | ~free (no I/O besides readdir) | Misses extensionless/wrong ext; false positives (`.ts` TypeScript!, `.m2ts`, `.mod`, `.dat`) | Use as fast pre-filter |
| Magic-byte sniff (read first ~4-64 KB) | One small read per file | Good for MP4/MOV (`ftyp` at offset 4), MKV/WebM (`1A 45 DF A3`), AVI (`RIFF....AVI `), FLV (`FLV`), MPEG-TS (0x47 every 188 bytes), OGG (`OggS`), ASF/WMV (GUID), MPEG-PS (`00 00 01 BA`) | Libs: `infer` (Rust), `filetype`/`python-magic` (libmagic) (Python), `file-type` (JS). Cannot distinguish audio-only vs video |
| ffprobe validation | Process spawn ~30-150 ms (more on Windows/AV scanners) + file read | Authoritative: has a video stream with non-attached-pic disposition, duration, decodable headers | Run only on candidates, in parallel, cached |

Recommended pipeline (tiered):
1. Walk and filter by extension allowlist (case-insensitive): `mp4 m4v mov mkv webm avi wmv flv f4v mpg mpeg m2ts mts ts vob 3gp 3g2 ogv mxf asf rm rmvb divx gif?` (gif off by default; `.ts` ambiguous -> require magic check; `.mod`, `.dat` only with sniff).
2. Optional `--sniff`: for non-allowlisted files below size floor/ceiling or extensionless, magic-byte check.
3. `ffprobe` validation for the selected files (`-v error -show_entries stream=codec_type,codec_name,width,height,disposition:format=duration,format_name -of json`), requiring >=1 video stream whose `disposition.attached_pic == 0` (excludes MP3 cover art). Run lazily (when listing details or before processing) with a worker pool = CPU count; cache keyed by (path, size, mtime) in a small SQLite/JSON file.
4. Results flag: `ok`, `unreadable`, `no_video_stream`, `corrupt/truncated` (moov missing -> ffprobe error), `in_progress` (file still being written; mtime recent).

### 4.3 Walking: recursion, symlinks, hidden files, performance

- Recursive with `--max-depth`, `--no-recurse`; include/exclude globs (`--include "*.mp4"`, `--exclude "**/node_modules/**"`), honor optional `.clipignore`.
- **Symlinks**: default do not follow directory symlinks (cycle risk); follow file symlinks optionally (`--follow-symlinks`); if following, track visited (dev, inode) / canonical path. **Windows**: directory junctions and reparse points (OneDrive placeholders, "files on demand" cloud files: reading triggers download - detect `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`/`OFFLINE` and skip or warn); symlink creation needs privileges but reading is fine.
- **Hidden**: Unix dot-files/dirs skipped by default; Windows `FILE_ATTRIBUTE_HIDDEN`/`SYSTEM` skipped by default (also skip `$RECYCLE.BIN`, `System Volume Information`, `.Trash-*`, `.git`, `@eaDir` (Synology thumbnails), `.thumbnails`); flag `--hidden` to include. Skip sidecar/temp artifacts: `*.part`, `*.crdownload`, `*.tmp`, `.!qB`, `*.ytdl`, `._*` (macOS AppleDouble).
- **Performance**: use `readdir` with `d_type`/Win32 `FindFirstFileEx` data (avoid a `stat` per file). Rust: `walkdir` or `ignore` (parallel walker, ripgrep's, supports gitignore-style filters), Python: `os.scandir` (3-20x faster than `os.walk` pre-3.5; `os.walk` now uses scandir) with threads for network drives; `pathlib.rglob` is slower. Do not ffprobe during the walk; stream results to the GUI incrementally (iterator/channel) with cancel support. Network shares (SMB/NFS): limit concurrency, avoid magic reads unless requested.
- Paths: handle non-UTF-8 names on Linux (`OsString`/surrogateescape), Windows long paths (`\\?\` prefix, manifest `longPathAware`), case-insensitive filesystems (dedupe by normalized path), Unicode normalization (NFC/NFD differences between manifest text and FS), trailing dots/spaces on Windows.
- Output: stable sorted order (natural sort), emit per-file `{path, size, mtime, ext, sniffed_type, probe(optional)}`, also `--json` output for scripting.

### 4.4 Archive file handling (if opted in)
- ZIP: Python `zipfile`, Rust `zip` crate; RAR: needs `unrar`/`7z`/`libarchive` (`bsdtar` reads rar/7z/zip/tar); 7z: `7z`, `py7zr`, Rust `sevenz-rust`.
- Prefer listing then extracting only chosen entries to a temp dir; check free disk first; clean up on exit.
- Multi-part archives (`.part1.rar`, `.z01`) - out of scope v1.

Sources:
- ffprobe docs: https://ffmpeg.org/ffprobe.html
- File signatures list: https://en.wikipedia.org/wiki/List_of_file_signatures
- `infer` crate: https://crates.io/crates/infer ; `walkdir`: https://crates.io/crates/walkdir ; `ignore`: https://crates.io/crates/ignore
- Python `os.scandir` (PEP 471): https://peps.python.org/pep-0471/
- Windows file attributes: https://learn.microsoft.com/windows/win32/fileio/file-attribute-constants
- Windows long paths: https://learn.microsoft.com/windows/win32/fileio/maximum-file-path-limitation
- Zip slip: https://security.snyk.io/research/zip-slip-vulnerability

---------------------------------------------------------------------------

## 5. Integrating FFmpeg into an app

### 5.1 Subprocess vs library bindings

| | Subprocess (`ffmpeg`/`ffprobe` binaries) | Library bindings (libav*) |
|---|---|---|
| Examples | Python `subprocess`/`ffmpeg-python` (unmaintained)/`python-ffmpeg`; Rust `std::process`, `ffmpeg-sidecar`, `tokio::process` | PyAV (Python, wraps libav*, ships wheels with LGPL-ish builds), Rust `ffmpeg-next` / `ffmpeg-sys-next` (needs system libs; maintenance variable - **[verify]** current status), `rsmpeg`, C++ direct |
| Crash isolation | Strong - a bad file cannot crash the app | Weak - segfaults/UB inside the process |
| Feature coverage | Everything the CLI does (filters, hwaccel, `-c copy` semantics, `-ss` logic, smart concat) | Must reimplement CLI behaviors (seek/discard logic, timestamp fixing, muxer flags) - substantial effort |
| Packaging | Ship 2 executables | Ship DLL/.so with matching ABI, build complexity (esp. Windows MSVC/MinGW) |
| Progress/cancel | `-progress pipe:1` parse + kill process | Direct callbacks |
| Licensing | Aggregate/"separate program" boundary - simpler | Linking: LGPL dynamic linking obligations; GPL contagion if GPL build linked |
| Updates | Swap binary, user can supply own | ABI-locked |

**Recommendation: subprocess.** Clipping is exactly the CLI's strength; the semantics (accurate seek, avoid_negative_ts, muxer quirks) are battle-tested. Use libav bindings only for niceties like in-GUI preview/scrubbing/thumbnails (alternatively `mpv`/libmpv or QtMultimedia/GStreamer for playback, and ffmpeg subprocess for thumbnails).

### 5.2 Progress reporting with `-progress`

- Add `-progress pipe:1 -nostats` (writes `key=value` blocks to stdout, each block ends with `progress=continue|end`). Keys: `frame`, `fps`, `bitrate`, `total_size`, `out_time_us` (also `out_time_ms` which is actually microseconds - historical bug; prefer `out_time_us` **[verify]** present in your build), `out_time`, `dup_frames`, `drop_frames`, `speed`, `progress`.
- Percent = `out_time_us / (clip_duration_us)`; for copy mode, updates are bursty. ETA = remaining / `speed`.
- Alternatives: `-progress tcp://127.0.0.1:PORT` or a Unix socket/named pipe to avoid mixing with stdout; or `-progress file`. Keep stderr separately for logs/errors (`-loglevel warning`, or `-v error` plus `-stats_period`). Drain both pipes concurrently (threads/async) to avoid deadlock on full pipe buffers.
- Cancel: send `q` to stdin (graceful, finalizes file; requires stdin NOT `-nostdin`) or SIGTERM/`terminate()`; on Windows, `GenerateConsoleCtrlEvent` is awkward - use `q` via stdin, then kill after timeout. Delete partial temp output. Use `CREATE_NO_WINDOW` on Windows (no console flash in the GUI); on Linux set process group to kill children.
- Parallelism: batch over files with N workers (`min(cpu/2, 4)` for re-encodes; copy mode I/O-bound -> more workers OK, cap 1-2 per HDD).
- Exit codes: nonzero = fail; also scan stderr for "Invalid data", "moov atom not found", "Non-monotonous DTS" (warning, usually harmless in copy mode) - classify.

### 5.3 Shipping/bundling FFmpeg

- **Windows builds**: BtbN FFmpeg-Builds (https://github.com/BtbN/FFmpeg-Builds) - GitHub Actions builds, `win64-gpl`, `win64-lgpl` (+ `-shared` variants), master and release branches (nightly `latest` tags + versioned auto-builds); gyan.dev (https://www.gyan.dev/ffmpeg/builds/) - `essentials` (GPL), `full` (GPL, includes more libs), release and git builds, 7z/zip; shared vs static. BtbN's LGPL builds exclude libx264/libx265 and use hardware encoders (NVENC/QSV/AMF) + native `aac`/`libopus`/`libvpx`/`libsvtav1`-style codecs depending on config **[verify exact enabled libs]**. Windows builds cross-compiled with MinGW-w64.
- **Linux builds**: johnvansickle static builds (https://johnvansickle.com/ffmpeg/ - GPLv3 static, amd64/arm64, release and git), BtbN `linux64-gpl/lgpl`, distro packages (`apt install ffmpeg`, Flatpak runtime org.freedesktop.Platform.ffmpeg-full). For AppImage/Flatpak/deb shipping, bundle a static build or declare dependency on system ffmpeg; discovery order: explicit `--ffmpeg PATH` / config -> bundled `resources/bin` -> `PATH`.
- Verify integrity: pin version + SHA256 of downloaded artifacts in the build script; download at build/package time (do not commit binaries); record version/commit in About dialog and `--version`. Run `ffmpeg -version` and `-buildconf`/`-encoders` at startup to feature-detect (`libx264`, `h264_nvenc`, `libfdk_aac`...) and adapt available presets.
- Alternative: first-run downloader (as yt-dlp GUIs and Handbrake-like tools do) keeps installer small and sidesteps redistribution burdens - but needs network and trust-chain; offer both.
- Code signing / SmartScreen / AV false positives: sign your installer; bundled ffmpeg.exe unsigned triggers occasional warnings.
- Size: static ffmpeg+ffprobe ~ 80-150 MB each on Windows GPL builds (shared builds smaller per-exe but add DLLs); ffprobe can be omitted if you parse `ffmpeg -i` output (not recommended) - ship both. LGPL "essential" custom builds can be 20-40 MB if you compile your own minimal (`--disable-everything` + enable demuxers/muxers/`copy`, `aac`, `h264` decoders...). Custom minimal build is attractive for copy-only mode.

### 5.4 Licensing: LGPL vs GPL

- FFmpeg core is LGPL v2.1+; enabling GPL components (`--enable-gpl`: libx264, libx265, libxvid, some filters like `postproc`, `frei0r`... ) makes the whole binary GPL v2+; `--enable-version3` -> (L)GPLv3; `--enable-nonfree` (libfdk_aac, openssl w/ GPL) makes the binary **non-redistributable**.
- **Subprocess boundary**: running an unmodified GPL ffmpeg binary from your app is generally considered "aggregation"/separate programs - your app need not be GPL provided you do not link it and you comply with GPL for the ffmpeg binary itself: ship license text, state it is FFmpeg, and **offer corresponding source** (link to the exact source tarball/commit for the build you ship; BtbN/gyan publish build scripts/sources; written offer or hosting the source for 3 years under GPL v2 section 3 / immediate source for v3). Not legal advice - have counsel review.
- **Linking libav\* (bindings)**: with LGPL builds, dynamic linking OK (user must be able to relink/replace DLL; do not static link into proprietary app without providing object files); with GPL builds, your app becomes GPL.
- Codec patents (not copyright): H.264/H.265/AAC patent pools (Via LA, Access Advance, Avanci) may require royalties for distributing encoders in certain scenarios; stream copy avoids encoding. Hardware encoders (NVENC/QSV/VideoToolbox/AMF) delegate to drivers. Decide: ship LGPL build with hardware + `aac` + `libopenh264` (Cisco, BSD, patent-covered when using Cisco binaries only) vs GPL build with `libx264` for best quality/size. For an open-source app (GPL-compatible), GPL builds are fine.
- Recommended: **dual-config** - app is licensed to match your chosen policy; default bundled build = GPL (libx264/x265 enabled) if project is OSS/GPL-compatible; else LGPL build + hardware encoders and "bring your own ffmpeg" option. Document in a THIRD_PARTY_NOTICES file; FFmpeg legal checklist: https://ffmpeg.org/legal.html
- Also note Qt (LGPL/GPL/commercial) if GUI toolkit is Qt; PyQt is GPL/commercial, PySide LGPL - relevant for the GUI decision (outside this doc's scope but interacts).

Sources:
- FFmpeg legal: https://ffmpeg.org/legal.html
- FFmpeg download page (links to builds): https://ffmpeg.org/download.html
- BtbN: https://github.com/BtbN/FFmpeg-Builds ; gyan: https://www.gyan.dev/ffmpeg/builds/ ; johnvansickle: https://johnvansickle.com/ffmpeg/
- Progress option docs: https://ffmpeg.org/ffmpeg.html#Advanced-options (`-progress url`)
- PyAV: https://github.com/PyAV-Org/PyAV ; ffmpeg-next: https://github.com/zmwangx/rust-ffmpeg ; ffmpeg-sidecar: https://github.com/nathanbabcock/ffmpeg-sidecar ; rsmpeg: https://github.com/larksuite/rsmpeg
- GPL FAQ on aggregation: https://www.gnu.org/licenses/gpl-faq.html#MereAggregation

---------------------------------------------------------------------------

## 6. Prior art

### LosslessCut (https://github.com/mifi/lossless-cut, GPL-2.0, Electron + ffmpeg subprocess)
Features worth emulating:
- Lossless cut via `-c copy` with visible keyframe markers on timeline; options "keyframe cut" vs "normal cut" (accurate, slower) and experimental smart cut.
- Multiple segments on one video, labelled; export each as separate file or merge segments into one; invert segments.
- Project file (`.llc`), CSV/EDL/XML/CUE/YouTube-chapter import-export, CLI args (`--ffmpeg-path`, open files).
- Per-stream selection (drop/keep tracks), edit tags, `avoid_negative_ts` settings exposed, "shift timestamps" workaround options, output filename template (`${FILENAME}-${CUT_FROM}-${CUT_TO}-${SEG_NUM}${EXT}`), batch file list, scene detection / silent-part detection / black-frame detection to auto-create segments, waveform and thumbnails, keyboard-centric UX (I/O set in/out, J/K/L, frame step), "capture frame", chapters from segments, remux/convert on export.
- Known pain points to avoid: lead-in glitches on non-keyframe starts, MP4 edit-list/negative timestamp inconsistencies across players, HEVC B-frame issues, A/V offset - hence the mode warnings.

### Avidemux (https://avidemux.sourceforge.net/)
- A/B markers, "Copy" video codec for lossless cut with keyframe constraints, "smart copy" for MPEG-2/H.264 in some modes, frame-accurate re-encode, filters, job queue, scripting (tinyPy/Python) for batch, project files. Takeaways: job queue, scripting hook, frame stepping, keyframe navigation (Up/Down arrows jump to keyframes).

### Others
- **VideoReDo** (commercial) - best-in-class smart rendering for MPEG-2/H.264, ad detection (QuickStream Fix) - reference for smart-cut behavior.
- **smartcut** (https://github.com/skeskinen/smartcut) - Python, cuts with minimal re-encode using PyAV.
- **Shutter Encoder**, **Handbrake** (chapters/range via `--start-at/--stop-at`, queue), **ffmpeg-based cutters** like `ffcut`/`ffsplit` scripts, `mkvmerge --split parts:00:10-00:25` (keyframe-bound lossless splitting; its `parts:` syntax is a good minimal clip DSL), `ffmpeg -f segment`, `MP4Box -splitx` (GPAC), **LosslessCut forks**, `video-clip` CLI utilities, `yt-dlp --download-sections "*10:15-12:30"` (supports `*start-end`, multiple sections, chapter regex; uses ffmpeg `-ss/-to`) - useful syntax reference for the Reddit-style download-and-clip workflow.
- Players with clip features: mpv (`ab-loop`, `--start`/`--end`, `mpv-webm`/`mpv-gif` script to export clips via ffmpeg), VLC (record).

### Feature shortlist to emulate
1. Keyframe-aware preview of snap result; show "requested vs actual" times.
2. Modes: fast copy / accurate / smart; per-clip override.
3. Dry-run/plan output; resumable batch with per-job status report (JSON log), skip-existing.
4. Filename templates with tokens; collision handling.
5. Import/export interop (LosslessCut CSV, YouTube chapters, EDL import).
6. Stream selection; subtitles keep/drop/burn; metadata/chapters policy.
7. Auto-segmentation helpers (scene detect, silence, black) - phase 3.
8. Command echo: show the exact ffmpeg command (copy-to-clipboard) for transparency.

Sources: LosslessCut README/FAQ https://github.com/mifi/lossless-cut ; Avidemux wiki https://www.avidemux.org/admWiki/doku.php ; mkvmerge `--split` https://mkvtoolnix.download/doc/mkvmerge.html ; yt-dlp `--download-sections` https://github.com/yt-dlp/yt-dlp#download-options ; GPAC MP4Box https://wiki.gpac.io/MP4Box/MP4Box-std-opts/ ; mpv https://mpv.io/manual/stable/

---------------------------------------------------------------------------

## Recommendations (concise)

1. **Engine**: drive `ffmpeg`/`ffprobe` as subprocesses (not libav bindings). Isolate in an `Engine` module with: probe (cached), plan (resolve times, snap, naming), run (progress via `-progress pipe:1`, cancel, temp-file + atomic rename).
2. **Modes**: `fast` (default; `-ss` before `-i`, `-t` duration, `-c copy`, `-map` explicit, `-avoid_negative_ts make_zero`, report snapped times and error), `accurate` (re-encode, `-ss` before `-i`, libx264 CRF 18 / source-matching, audio AAC or copy), `smart` (phase 2; head/tail re-encode + copy middle + concat; auto-fallback to accurate on any parameter mismatch, VFR, interlaced, open-GOP uncertainty). Per-clip override allowed.
3. **Always compute duration yourself** and pass `-t`; avoid `-to` semantic ambiguity. Use rational time internally; frames/SMPTE converted through probed fps (PTS table for VFR; warn).
4. **Timestamp parser**: accept seconds, `[HH:]MM:SS[.fff]` (comma decimal too), unit suffix (`1h2m3s`, `500ms`), `f123` frames, SMPTE `HH:MM:SS:FF`/`;FF` (DF aware), `end`, `+duration`. Reject ambiguous input loudly; clamp end to media duration with warning.
5. **Manifest**: canonical JSON schema (v1) with `defaults` / `videos[]` / `clips[]`; YAML accepted only with string-forced times (document the YAML 1.1 sexagesimal trap; consider TOML); CSV `input,start,end,duration,name,mode,output` for batch and LosslessCut-compatible `start,end,label` for single video; simple line format `START END name` for one-video lists; support YouTube-chapter style and EDL import later. Relative paths resolve against manifest location. Provide `--validate`/`--dry-run` and JSON Schema.
6. **Discovery**: recursive scan with extension allowlist -> optional magic-byte sniff -> ffprobe validation (video stream, not attached pic), cached by (path,size,mtime), parallel, streaming results to GUI; do not follow directory symlinks by default; skip hidden/system/temp/partial files by default; handle Windows reparse/cloud placeholders and long paths. Treat "video archive" primarily as a directory of videos; support zip/7z/rar contents only behind an explicit opt-in flag (with zip-slip/bomb guards) - confirm intended meaning with the product owner.
7. **Bundling**: do not commit binaries; fetch pinned, checksum-verified builds at packaging time (Windows: BtbN or gyan.dev; Linux: BtbN or johnvansickle static), search order `--ffmpeg` flag/config -> bundled -> PATH; feature-detect encoders at startup. Ship both ffmpeg and ffprobe; hide console windows on Windows.
8. **Licensing**: decide GPL vs LGPL build up front. Subprocess usage keeps the app's own license independent, but a GPL build requires shipping license text and a source offer for the ffmpeg binary. If proprietary: LGPL build (hardware encoders + native aac) plus bring-your-own-ffmpeg; if OSS: GPL build with libx264. Get legal review; maintain THIRD_PARTY_NOTICES.
9. **Emulate** from LosslessCut: keyframe-aware snapping display, multi-segment labelling, filename templates, stream selection, project/CSV interop, command echo; from Avidemux: job queue + scripting; from yt-dlp: `*start-end` section syntax as a CLI shorthand (`--clip 1:10-1:42`, repeatable).
10. **Open questions for owner**: meaning of "video archive"; license of the app; need for audio-only clips or GIF export; target output platforms (Reddit-style limits: H.264/AAC MP4, size/duration caps -> maybe a "reddit-ready" preset); fast-mode snap direction default (previous keyframe = include requested start vs next keyframe = exclude extra footage).
