# CSV Manifest Format (`*.csv`)

RFC 4180 CSV, UTF-8 (BOM tolerated). A header row is **required**; column order is free; unknown
columns are an error. Lines starting with `#` are comments.

| Column | Required | Meaning |
|---|---|---|
| `input` | yes | video path (relative → manifest directory) |
| `start` | yes | time |
| `end` | one of end/duration | time or `end` |
| `duration` | one of end/duration | time (no `+` needed) |
| `name` | no | clip name |
| `mode` | no | `fast` \| `accurate` |
| `output` | no | output directory for this clip |

Rows sharing an `input` are grouped into one source in order of first appearance. CLI flags act as
defaults. Equivalent JSON and CSV manifests MUST produce identical plans (FR-007).

```csv
input,start,end,duration,name,mode,output
match12.mkv,0:45,1:10,,opening goal,,
match13.mp4,30,,12.5,,accurate,
match13.mp4,12:00,end,,,accurate,
```

Errors: `<file>:<line>: <reason>`.
