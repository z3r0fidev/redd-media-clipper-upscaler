# Research & Decisions: Video Clipping Base

Source outlines: `research/01-clipping-and-discovery.md`, `research/04-app-stack-and-packaging.md`
(and `02`/`03`/`05` for forward compatibility). Items those outlines marked [K]/[verify] were
either re-checked locally on 2026-10-06 (marked **verified**) or carried as test-first assertions.

## R1. Application stack
- **Decision**: Rust workspace — `rmedia-core` lib, `rmedia-cli` (clap), Tauri **2.x** + Svelte 5 GUI.
- **Rationale**: one core with no IPC shared by both surfaces (Principle I); tokio + process-wrap
  give precise child-process control on both OSes; Tauri bundler covers NSIS/MSI/AppImage/deb/rpm
  + updater; small installers; team has the tauri-v2 skill. (research/04 §Recommendations)
- **Verified**: crates.io on 2026-10-06 — tauri latest is `3.0.0-alpha.4`, so we pin 2.x;
  `@tauri-apps/cli` 2.12.1; process-wrap 10.0.1; ts-rs 12.0.1; svelte 5.57.1. Linux build deps
  present locally (webkit2gtk-4.1 2.52.6, librsvg2-dev, libayatana-appindicator3-dev).
- **Alternatives**: Go + Wails (v3 still beta, no packager/updater parity); Python + PySide6
  (80–250 MB installers); egui/Slint/Iced (manual packaging). Fallback if webview proves
  unworkable: egui over the same core.

## R2. FFmpeg integration
- **Decision**: invoke `ffmpeg`/`ffprobe` as subprocesses with argv vectors; never link libav.
- **Rationale**: crash isolation, full CLI semantics, simple GPL boundary (research/01 §5.1).
- **Details**: every call uses `-hide_banner -nostdin -n -loglevel warning -progress pipe:1
  -nostats`; paths passed as `file:<path>` (prevents `-x.mp4` / `a:b.mp4` being read as
  options/protocols); explicit `-f <muxer>` because temp outputs end in `.part`.
- **Verified locally (ffmpeg 6.1.1)**: `-progress pipe:1` emits `out_time_us` and a final
  `progress=end`.

## R3. Fast vs accurate clipping
- **Decision**: fast = input `-ss S` + `-t D` + `-c copy` + `-avoid_negative_ts make_zero`;
  accurate = input `-ss S` + `-t D` + re-encode with `-fps_mode passthrough`. Duration is always
  computed by the app (never output `-to`).
- **Verified locally** (GOP-50 @ 25 fps fixture, request start 10.5, D 5):
  - `ffprobe -read_intervals "10.5%+#1" -select_streams v:0 -show_entries packet=pts_time,flags`
    → `10.000000,K` — **prediction works**.
  - fast output: format duration 5.685 s; video stream starts 0.085 s (B-frame composition offset)
    with 5.6 s duration → output covers keyframe 10.0 → requested end 15.5. **Fast mode keeps the
    requested end and snaps the start earlier**; the report must say "started 0.5 s early".
  - accurate output: exactly 5.000 s.
- **Implementation finding (2026-10-06, verified by tests)**: demuxer seeking is only exact for
  indexed containers. On MPEG-TS, input `-ss` lands after the preceding keyframe (accurate clips
  started ~1 s late, fast clips a GOP late), and when input and output `-ss` are combined on TS,
  the output timeline origin shifts by the container start time (a mislabelled TS started 1.46 s
  early). Final strategy — only combinations whose timing is unambiguous:

  | Mode | Indexed (decided from ffprobe `format_name`: mov/mp4/matroska/webm) | Other (TS/PS/AVI…, incl. mislabelled files) |
  |---|---|---|
  | fast | input `-ss S`; lands on keyframe K ≤ S | no demuxer seek; output `-ss` at K − min(4 frames, ½ GOP); leading non-keyframes are dropped → starts exactly on K |
  | accurate | input `-ss S` (frame exact, 48.7 dB first-frame match) | `-copyts` + coarse input `-ss S−10s` + `trim`/`atrim` at absolute source times (48.4 dB) |

  - K is found by scanning packets in a window before S (ffprobe `-read_intervals`), never from
    where the demuxer lands.
  - **Time base**: user time 0 = first video frame; every emitted `-ss` adds
    (first-video start − container start) so all-intra sources cannot gain a frame.
  - Accurate outputs are verified after encoding: video length must match the request within
    3 frames + 100 ms, else VERIFY_FAILED (never a silently wrong clip).
  - Test references must not use `-ss` themselves (they would share the bug): the testkit decodes
    sequentially and trims on timestamps.
- **Consequence**: `actual_start = predicted keyframe`, `actual_end = actual_start + measured
  output duration − video start offset`; measured values are authoritative and reported when
  |Δ| > 0.05 s (FR-014).
- **Alternatives**: output-side `-ss` (slow decode-all); smart cut (deferred; research/01 §1.4).

## R4. Accurate-mode encode profiles
- **Decision**: data table keyed by output container: mp4/m4v/mov/mkv/ts → libx264 CRF 18
  preset medium yuv420p + AAC 192k; webm → libvpx-vp9 CRF 24 + libopus 160k. Containers with no
  profile (avi, wmv, flv, mpg, 3gp, ogv) → output **mp4** with a warning (owner-default decision,
  revisable). Missing encoder in the engine build → plan-time error naming it (Principle VI).
- **Verified**: local engine lists libx264, libx265, libvpx-vp9, libopus, aac.

## R5. Clip list & manifest formats
- **Decision**: clip list = line format `START END|+DUR [NAME]` (whitespace or comma separated,
  `#` comments); structured manifest = **JSON** (strict, versioned); tabular manifest = **CSV**
  (`input,start,end,duration,name,mode,output`). No YAML/TOML.
- **Rationale**: JSON round-trips with the GUI and has no YAML 1.1 sexagesimal trap (`1:30` → 90);
  CSV is spreadsheet-friendly and close to LosslessCut CSV (research/01 §3).
- **Comma rule**: a comma immediately between `:SS` and exactly three digits is a decimal
  separator (`00:01:30,500`); every other comma separates fields.
- **Errors**: clip list/CSV by line; JSON syntax by line:column, JSON semantics by JSON path.

## R6. Directory discovery
- **Decision**: walkdir (no symlink following, skip hidden/system/temp/`.part`/`._*`), case-
  insensitive extension allowlist (`mp4 m4v mov mkv webm avi wmv flv f4v mpg mpeg m2ts mts ts vob
  3gp 3g2 ogv mxf asf`), then parallel ffprobe validation requiring ≥1 video stream with
  `disposition.attached_pic == 0`. Archives never opened (owner decision).
- **Rationale**: ffprobe is authoritative; magic sniffing adds little (research/01 §4.2).
  1,000 probes × ~50 ms / 8 workers ≈ 7 s ≪ 60 s (SC-003), so no persistent cache in v1.

## R7. Process lifecycle & cancellation
- **Decision**: process-wrap — Unix: own process group + kill-on-drop + `PR_SET_PDEATHSIG` set from
  a dedicated long-lived spawner thread; Windows: Job Object with KILL_ON_JOB_CLOSE +
  CREATE_NO_WINDOW. Cancel = kill tree immediately (no graceful `q`: partials are discarded
  anyway), then delete `.part` (retry ≤ 1 s on Windows sharing violations).
- **Output commit**: write `.<stem>.rmedia-<run>-<job>.part` → ffprobe-verify → `hard_link` to
  final (atomic no-clobber) → remove temp; fallback rename on filesystems without hard links.
- **Crash cleanup**: journal `state_dir/inflight/<pid>.json`; on start, purge entries of dead pids.

## R8. GUI preview in a webview
- **Problem**: WebKitGTK depends on distro GStreamer plugins (H.264 may be missing); WebView2
  lacks MKV/HEVC reliability.
- **Decision**: (1) direct play when allowlisted and `canPlayType` agrees; (2) `frame_at`
  still-frame scrubbing always available; (3) on-demand 480p proxy (H.264 ultrafast, else VP8/WebM)
  cached by hash with LRU cap; (4) thumbnail strip. Proxy uses passthrough timestamps, so marked
  times map 1:1 to source.
- **Spike result (T038, 2026-10-06, WebKitGTK 2.52.6 / Tauri 2, run headless under Xvfb,
  `apps/desktop/src-tauri` + `apps/desktop/spike-ui`)**:
  | Transport | Range fetch | `<video>` load | seek to 30 s |
  |---|---|---|---|
  | `asset://` (convertFileSrc) | 206 + Content-Range OK | **MEDIA_ERR_SRC_NOT_SUPPORTED for every codec** (h264, vp8, vp9, mkv) | — |
  | `blob:` (whole file in memory) | — | OK | 15 ms |
  | `http://127.0.0.1:<port>` range server | — | OK | 2 ms |
  `canPlayType` reports "probably" for H.264, HEVC, VP8, VP9 — it is not a reliable predictor.
- **Revised decision**: media playback in the desktop app goes through an **in-process localhost
  HTTP range server** (bind 127.0.0.1, random port, random per-session token in the URL path,
  serves only files explicitly allow-listed by the core, GET/HEAD only), on both Linux and Windows
  (one code path). The asset protocol is kept for thumbnails/stills only. CSP `media-src` allows
  `http://127.0.0.1:*`. Security review required (T061 checkpoint).
- **Security requirements for the media server** (security-auditor review, 2026-10-06; all MUST):
  1. Bind 127.0.0.1 only (not `localhost`/IPv6/0.0.0.0), ephemeral port, in-process, fail closed.
  2. ≥128-bit CSPRNG session token in the URL path, constant-time compare, rotated per launch,
     invalidated on window close.
  3. Serve by opaque ID registered by the core (`id → canonical path`) only for sources the user
     opened via native dialog/drag-drop/CLI; URLs never carry filesystem paths; unknown ID → 404.
  4. Per request: re-check the registered file, refuse symlink escapes, open with O_NOFOLLOW where
     available, serve from the opened handle, regular files only.
  5. GET/HEAD only (else 405); single strict `Range`, clamped; 416 on invalid; stream in
     64–256 KiB chunks, never buffer whole files.
  6. Validate `Host` = `127.0.0.1:<port>` and `Origin` (when present) = the app origin; no
     `Access-Control-Allow-Origin: *`; `Cross-Origin-Resource-Policy`, `X-Content-Type-Options:
     nosniff`, `Cache-Control: no-store`.
  7. `Content-Type` from a fixed video map, else `application/octet-stream`; never HTML/JS.
  8. Limits: ≤8 concurrent connections, read/write/idle timeouts, header size cap; tolerate files
     changing or disappearing mid-stream.
  9. Token and IDs never logged or exposed except via one `get_media_url(id)` command.
  10. Never serve outputs, temp files, journal/state dir; proxies/thumbnails only by hash ID.
  11. CSP narrowed at runtime to the exact port; production removes `unsafe-inline` and
      `withGlobalTauri`; asset-protocol static scope stays empty. A Tauri command must never take a
      webview-supplied path and widen a scope with it (arbitrary-file-read primitive).
  12. Tests: missing/wrong token, `../` and encoded traversal, symlink swap mid-session, bad
      `Host`/`Origin`, multi-range/oversized ranges, no ID enumeration, shutdown on last window.
- **Implementation status (security-auditor, 2026-10-06, after fixes)**: #1, #3, #5–#8, #10 met;
  #2 met (token revoked with all ids on window close); #9 met (token redacted from Debug); #12
  met (`apps/desktop/src-tauri/tests/media_server.rs`). Residual, accepted for v1: #4 re-checks
  the canonical path + `O_NOFOLLOW` but not dev/inode against registration (needs a local
  attacker racing a parent-directory swap); #11 CSP is static (`media-src http://127.0.0.1:*`,
  `style-src 'unsafe-inline'` for Svelte style attributes) — no script injection and no exfil
  channel (`connect-src` is IPC only). Also fixed from that audit: the page can no longer set
  engine paths (native picker in Rust only), outputs must be inside user-picked folders or next
  to the source, clip lists/manifests must be picked as such and manifests only grant video files,
  capabilities reduced to `core:event:allow-listen/unlisten`.
- Untrusted media metadata (titles, tags, chapter names) is rendered as text only — never
  `innerHTML`; the CLI strips control characters/ANSI escapes before printing it.

## R9. FFmpeg distribution & licensing
- **Decision**: bundle pinned, SHA-256-verified GPL builds (BtbN win64-gpl / linux64-gpl) as
  `rmedia-ffmpeg`/`rmedia-ffprobe` sidecars; fetched by `xtask fetch-tools` at build time (never
  committed); THIRD_PARTY_NOTICES + FFmpeg source offer. Locator order: config → bundled → PATH;
  minimum version 6.0.
- **Open**: owner to confirm app licence compatibility before public release.
- **Supply chain (audit M3)**: BtbN builds are unsigned and dated autobuild tags are pruned over
  time. Before the first release, mirror the pinned archives to our own release storage and keep
  the hash in reviewed commits; `xtask fetch-tools` re-verifies the installed binaries' hashes on
  every run. Static GPL binaries are ~172 MB each — consider shared builds or a trimmed custom
  build to reduce installer size.

## R10. Testing approach
- **Decision**: `rmedia-testkit` generates fixtures (testsrc2/sine, GOP 50, VFR, multi-audio + SRT,
  webm, no-audio, MPEG-TS with 10 s offset, cover-art m4a, fake and mislabeled files) into
  `target/rmedia-fixtures/<hash>/`; accuracy via ffprobe durations and first-frame PSNR; cancel
  and crash tests check for orphans with sysinfo; argv snapshots with insta.

## Forward compatibility (features 002 & 003)
- 002 upscaling: research/02 (tiered classical → libplacebo → ncnn-vulkan) and research/03
  (presets) — requires `JobCtx::spawn` to support multi-process pipelines in one kill group.
- 003 GIF export: research/05 — two-pass palette pipeline and size-budget iteration; requires a
  job to be able to run multiple ffmpeg passes and retry with new parameters inside one Job.
