# Desktop contract: Budgeted Reddit Video

Additions to `docs/api/desktop-ipc.md`. Every command calls the core; the app has no fit logic.

Changed in implementation: the prediction was planned to come from `plan`, but `plan` keeps each
plan for `run`, so predictions on every range change would pile up. `fit_plan` plans the one clip
like `smart_plan` does and keeps nothing.

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `list_fit_targets` | — | `{targets: [VideoTarget & {source, file?}], problems: [...]}` | the same as `rmedia clip --list-fit-targets --json` |
| `fit_plan` (new) | `path`, `span`, `options: PartialOptions` | `{fit: FitSummary \| null, notices, issues}` | the panel's prediction: the core's plan of the one clip, arithmetic only, so it is instant (research R1). Refusals are `issues`. A newer call cancels the older; nothing is kept |
| `plan` (existing) | manifest with `fit` at any level | `Plan` with `jobs[].fit` (`FitSummary`, the CLI's `fit` block) | |
| `run` (existing) | — | outcomes with `fit` | progress stages `pass1`, `pass2`, `verify` |

## Clip panel

- **Video target picker.** "None" (default), the targets, and "custom limits…" with size and
  length fields, which become `max_size`/`max_length`. There is also an "Allow trimming" toggle.
- **Before queueing**, the panel shows `fit.cap`, `fit.max_length` and `fit.predicted_bytes`, plus
  any `FIT_LOWERED` or `FIT_TRIMMED` notice. A `RANGE_TOO_LONG` or `BUDGET_UNREACHABLE` refusal
  disables "Add to queue" and offers "Allow trimming".
- **Controls the target overrides:** choosing a target disables GIF export for the clip, and
  switches Smart or Fast mode to accurate, with the core's note.

## Queue

- Each fit job shows its target, its cap, and the result `bytes of cap` (and "trimmed").
- An unverified target shows the "limits unverified" badge used for GIF targets.

## Settings

The user's `video-targets` folder is shown, with problems from `doctor`, under
**Settings › Targets**.
