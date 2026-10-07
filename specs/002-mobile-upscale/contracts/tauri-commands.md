# Desktop IPC additions: upscale view

This extends `specs/001-video-clipping/contracts/tauri-commands.md`. The path policy, binding
generation and bridge tests are unchanged. Every command is a thin adapter over a core function.

| Command | Args | Returns | Core function |
|---|---|---|---|
| `list_presets` | — | `{ default, presets: Preset[], problems }` | `preset::PresetRegistry::load` |
| `capabilities` | `recheck: boolean` | `CapabilityRecord` (plus GPUs and models) | `upscale::capability::ensure` |
| `analyze_reframe` | `analysisId, path, span?, options: UpscaleOptions, onEvent: Channel<AnalysisEvent>` | `ReframeAnalysis` | `upscale::reframe::analyze` (cached; same cache as the render, research R10) |
| `cancel_analysis` | `analysisId` | — | cancel token |
| `upscale_preview` | `path, at: Ts, options: UpscaleOptions` | `{ before: string, after: string }`: localhost media-server URLs of two PNG stills at the target size | `upscale::preview_frame`: one ffmpeg run of the job's graph limited to one frame at `at` |

- **`AnalysisEvent`:** `{ type: "progress", fraction }`, `{ type: "shot", shot }` or
  `{ type: "done" }`.
- **`upscale_preview` timing:** it must return within 5 s for a typical 1080p source (US5
  scenario 1).
  - Smart reframe uses the cached analysis' x at `at` when there is one.
  - Without one, it uses a centred crop, flagged `approximate: true`.

## Existing commands extended

- **`plan` / `run`:** queue entries may carry `options.upscale` (`PartialUpscale`). Upscale jobs
  appear in `Plan.jobs` with the `upscale` block (json-output.md).
- **`export_manifest`:** writes `upscale` objects. Desktop and CLI outputs must match (FR-022).
- **`get_settings` / `set_settings`:** add `models_dir` and `upscale_defaults`.

## Path policy additions

- Preview stills are written under the app cache directory and served by the existing media
  server. The page never receives a filesystem path for them.
- `analyze_reframe` and `upscale_preview` accept only sources that are already allowed.
- `models_dir` can be set only with a folder picker in Rust, using the same mechanism as the output
  folder.

## UI contract (presentational)

**`UpscaleView.svelte`:**
- preset picker, with device classes shown;
- reframe, tier and motion pickers;
- before/after still with a slider;
- crop rectangle overlay on the Player, driven by `ReframeAnalysis.keyframes`;
- fallback ranges drawn on the timeline;
- "Add to queue".

**Defaults:** taken from `upscale_defaults`. With them, SC-007 takes 3 interactions after opening a
video: select the video, choose "Upscale for phone", then Run.
