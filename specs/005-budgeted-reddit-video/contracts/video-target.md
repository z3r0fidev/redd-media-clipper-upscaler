# File contract: video targets

- **Location:** `<config>/video-targets/<anything>.json`, at most 64 files, each at most 64 KiB.
  `<config>` is the settings folder (`docs/administration/configuration.md`).
- **Format:** JSON, schema 1. Fields and rules are in [data-model.md](../data-model.md) ›
  VideoTarget.
- **Errors:** an invalid file is reported (`doctor`, `--list-fit-targets`, **Settings ›
  Targets**) and ignored; the other targets stay usable. Two user files with the same id: the
  first, by file name, wins, and the second is reported.

The built-in `reddit-video` target (Reddit's limits as of 2026-11-13; `research/10`):

```json
{
  "schema": 1,
  "id": "reddit-video",
  "version": 1,
  "name": "Reddit video post",
  "description": "Reddit's video post limits as of 2026-11-13 (unverified)",
  "verified": false,
  "max_bytes": 1073741824,
  "max_duration": 900,
  "long_side": 1920,
  "floor_short_side": 480,
  "max_fps": 60,
  "quality": { "crf": 20, "max_bpp": 0.12, "min_bpp": 0.03 },
  "audio": { "kbps": 128, "floor_kbps": 64 }
}
```

An example user target for a subreddit rule:

```json
{
  "schema": 1, "id": "sub-50mb", "version": 1, "name": "r/example rule", "verified": false,
  "max_bytes": 50000000, "max_duration": 300, "long_side": 1920, "floor_short_side": 480,
  "max_fps": 60, "quality": { "crf": 20, "max_bpp": 0.12, "min_bpp": 0.03 },
  "audio": { "kbps": 128, "floor_kbps": 64 }
}
```
