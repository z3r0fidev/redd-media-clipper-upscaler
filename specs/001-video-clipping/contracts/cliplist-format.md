# Clip List Format (`*.txt`)

UTF-8 (BOM tolerated), LF or CRLF. One clip per line:

```
START  END        [NAME...]
START  +DURATION  [NAME...]
```

- Fields are separated by whitespace or a comma (+ optional whitespace).
- **Decimal comma**: a comma directly between `:SS` and exactly 3 digits (`00:01:30,500`) is a
  decimal separator, not a field separator.
- `END` may be `end`. NAME is the rest of the line (trimmed), may contain spaces; it is sanitized
  for filenames.
- Comments: a line starting with `#`, or ` # ` (a `#` with whitespace on both sides) to the end
  of the line. A `#` inside a name such as `goal #3` is kept. Blank lines are ignored.
- Time grammar: see data-model.md `Ts`.

```
# highlights for match 12
0:45      1:10       opening goal
00:03:15.250  +20    save
12:00,500 end
```

Errors are reported as `<file>:<line>: <reason>` and all are reported before anything runs
(exit 3). Example reasons: `invalid time "1:7x"`,
`missing end after "0:45" (use END, +DURATION or end)`. An end that is not after its start is
reported when the plan is built: `end (0:40) is not after start (0:45)`.
