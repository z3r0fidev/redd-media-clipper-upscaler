# CSV Manifest Format (`*.csv`)

RFC 4180 CSV, UTF-8 (BOM tolerated). A header row is **required**; column order is free; unknown
columns are an error. Lines starting with `#` are comments.

| Column | Required | Meaning |
|---|---|---|
| `input` | yes | video path (relative → manifest directory) |
| `start` | yes | time |
| `end` | no | time, `end`, or `+DURATION` |
| `duration` | no | time (`+` optional); not together with `end` |
| `name` | no | clip name |
| `mode` | no | `fast` \| `accurate` |
| `output` | no | output directory for this clip |

Without `end` and `duration` a clip runs to the end of the video (as in JSON and `clip -s`).
`start` never takes a `+`. Rows sharing an `input` are grouped into one source in order of first appearance. CLI flags act as
defaults. Equivalent JSON and CSV manifests MUST produce identical plans (FR-007).

```csv
input,start,end,duration,name,mode,output
match12.mkv,0:45,1:10,,opening goal,,
match13.mp4,30,,12.5,,accurate,
match13.mp4,12:00,end,,,accurate,
```

Errors: `<file>:<line>: <reason>`.
