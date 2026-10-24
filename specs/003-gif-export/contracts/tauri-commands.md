# Desktop IPC additions: GIF view

This extends `specs/001-video-clipping/contracts/tauri-commands.md` and
`specs/002-mobile-upscale/contracts/tauri-commands.md`. The path policy, binding generation and
bridge tests are unchanged. Every command is a thin adapter over a core function.

| Command | Args | Returns | Core function |
|---|---|---|---|
| `list_gif_targets` | — | `{ default, targets: GifTarget[], problems }` | `gif::target::GifTargetRegistry::load` |
| `gif_estimate` | `estimateId, path, span?: {start, end\|null}, options: GifOptions, onEvent: Channel<EstimateEvent>` | `{ estimate: Estimate, notes, notices }`: the predicted result of the budget search, with the settings it expects to lower (research R4) | `gif::estimate::predict`: the job's search without the full encode (research R4) in an app-cache scratch folder, removed afterwards. With smart framing it uses the cached 002 analysis, or a centred crop flagged `approximate` when no analysis exists. A new estimate cancels the previous one. |
| `cancel_estimate` | `estimateId` | — | cancel token |
| `open_output` | `runId, jobId` | `{ url, kind: "image" \| "video", bytes, width, height }` | registers a finished output of **this app's** run with the media server; refused for any other path |

**`EstimateEvent`:** `{ type: "progress", fraction }` or `{ type: "done" }`.

**Estimate timing:** it must return within 5 s for a typical 1080p source and a ≤ 15 s range
(US6 scenario 1). A search needing more samples than fit in 5 s returns the best estimate so far,
marked `partial`, and keeps refining in the background through `EstimateEvent`. [U] The 5 s
figure is measured in US6. The store debounces setting changes by 400 ms, and each new request cancels
the one in flight.

## Existing commands extended

- **`plan` / `run`:** queue entries may carry `options.gif` (`PartialGif`). GIF jobs appear in
  `Plan.jobs` with the `gif` block ([json-output.md](./json-output.md)). Their events carry the
  GIF stages and `JobMeasure::Gif`.
- **`export_manifest`:** writes `gif` objects ([manifest-gif.md](./manifest-gif.md)), so the CLI
  reproduces desktop outputs byte for byte (SC-007).
- **`doctor`:** the report includes the `gif` block. Settings shows the engine lines and the
  "limits unverified" note.
- **`get_settings` / `set_settings`:** add `gif_defaults`, validated like CLI flags.
  - `gifsicle_path` and `gifski_path` are copied back from the stored settings in `set_settings`,
    so page values are ignored. A bridge test asserts this.
  - They are set only through `pick_engine(kind: "ffmpeg" | "gifsicle" | "gifski")`, a Rust file
    picker. It refuses symlinks and folders and runs the version-banner check (research R7)
    before saving.

## Media server

- `content_type()` adds `gif → image/gif` (and `jpg`/`jpeg → image/jpeg` for completeness).
  `nosniff` stays.
- GIFs play in `<img>` and MP4s in `<video>`. The CSP is unchanged: `img-src` and `media-src`
  already allow `http://127.0.0.1:*`.
- Registered outputs follow the `register_preview` cap and eviction: at most 48 live URLs.

## Estimate isolation (research R13)

- One estimate runs at a time. A new request kills the previous process tree and waits for it
  to exit.
- The scratch folder is a fresh 0700 tempdir under the app cache. It is removed on every exit
  path, and stale ones are removed at startup. A size watchdog aborts a runaway sample.
- `estimateId` is only a map key.
- The core re-clamps `options`, and bounds the sample to 3 s and 150 frames.
- The sample file is never registered with the media server: only sizes are returned.

## Path policy additions

- `gif_estimate` accepts only sources that are already allowed (`require_allowed`). Its scratch
  folder is under the app cache directory.
- `open_output` resolves `(runId, jobId)` to the committed output path from the run's own
  outcome. The page never sends a path.
  - At registration it checks that the file is a regular file, not a symlink, and still the
    committed output.
  - Output URLs are forgotten when the run is cleared. Live preview URLs are not evicted for
    them.

## UI contract (presentational)

**`GifView.svelte`** is a third tab next to Clip and Upscale (`app.view = "gif"`). It has:
- a target picker, showing the budget and "(limits unverified)";
- a format toggle: GIF / Reddit MP4 (recommended for posts);
- shape and framing pickers;
- an overrides drawer with every `PartialGif` field the CLI has: width, fps, colours, dither,
  lossy, loop, budget and hard limit, allow trim, engine, strict engine, optimise;
- the live estimate: size, and for a GIF the "MP4 ≈ N MB" hint;
- the result card: the playing output, its size, width, fps and the lowered settings;
- **Export now** (add and run only this job) and **Add to queue**.

When a budget is unreachable, the result card offers **Trim to N s**. That re-queues the job
with `allow_trim: true` (US2 scenario 2: confirmation in the desktop app).

**SC-007:** 3 interactions after opening a video: choose the GIF tab, choose the target (or keep
the default from `gif_defaults`), then **Export now**. The range is the player's In/Out marks
when they are set, and the whole video otherwise, as in 002. Setting In/Out is optional and not
counted, as in 002's SC-007.
