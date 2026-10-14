# `rmedia-sr` worker protocol (US6)

`rmedia-sr` is a frame filter. It makes no media decisions: the core picks the model, scale, GPU,
tile size and frame geometry. The core always starts it with an argv, as one process of the job's
pipeline (Principle IV).

## Invocation

```
rmedia-sr --model <DIR>/<name> --scale {2|3|4} --gpu <INDEX> --width W --height H
          [--tile N] [--fp16 auto|on|off] [--threads N]
rmedia-sr --list-gpus                    # JSON on stdout, software devices marked
rmedia-sr --selftest --model … --scale … --gpu … --width W --height H [--frames 8]
rmedia-sr --version
```

- `--model` names the ncnn files `<name>.param` and `<name>.bin`. Paths are plain OS paths in the
  argv, never parsed by a shell.
- `--gpu` is required. The test-only value `none` is accepted only with a `builtin:*` model
  (`builtin:nearest` does CPU nearest-neighbour ×s, so the pipeline can be tested without a GPU). A software Vulkan device (llvmpipe, lavapipe, SwiftShader, CPU type) is
  rejected with exit code 3.

## Data

- **stdin:** packed `rgb24` frames of exactly `W×H×3` bytes, back to back.
- **stdout:** packed `rgb24` frames of `(W·s)×(H·s)×3` bytes, in input order. There is one output
  frame per input frame.
- **End of stream:** EOF on stdin. The worker flushes, writes everything, then exits 0.
- **Partial input:** a short final frame is an error (exit 4).

## stderr: JSON lines

```
{"type":"device","index":0,"name":"…","kind":"discrete","fp16":true}
{"type":"progress","frames":120,"fps":7.9}
{"type":"selftest","frames":8,"sec_per_frame":0.127,"sec_per_mpx":0.31}
{"type":"error","code":"VK_OUT_OF_MEMORY","message":"…"}
```

`progress` is emitted at most every 250 ms.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 2 | usage |
| 3 | GPU unavailable or software device |
| 4 | input error |
| 5 | model load failure |
| 6 | GPU runtime error |
| 130/137 | killed |

## Cancellation

The core kills the whole pipeline (process group or Job Object). The worker does no cleanup of
its own and writes no files.

## Determinism

Output may differ between GPUs and drivers (fp16), but not between runs on the same device.
