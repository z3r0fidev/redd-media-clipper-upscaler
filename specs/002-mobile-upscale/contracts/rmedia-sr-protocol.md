# `rmedia-sr` worker protocol (US6)

`rmedia-sr` is a frame filter. It makes no media decisions: the core picks the model, scale, GPU,
tile size and frame geometry. The core always starts it with an argv, as one process of the job's
pipeline (Principle IV).

## Invocation

```
rmedia-sr --model <DIR>/<name> --scale {2|3|4} --gpu <INDEX> --width W --height H
          [--tile N] [--fp16 auto|on|off] [--threads N]
rmedia-sr --list-gpus                    # JSON on stdout, software devices marked
                                         # {"devices":[{"index","name","kind","software","fp16","heap_budget_mb"}]}
rmedia-sr --selftest --model … --scale … --gpu … --width W --height H [--frames 8]
rmedia-sr --version
```

- `--model` names the ncnn files `<name>.param` and `<name>.bin`. Paths are plain OS paths in the
  argv, never parsed by a shell.
- `--gpu` is required. The test-only value `none` is accepted only with a `builtin:*` model
  (`builtin:nearest` does CPU nearest-neighbour ×s, so the pipeline can be tested without a GPU). A software Vulkan device (llvmpipe, lavapipe, SwiftShader, CPU type) is
  rejected with exit code 3.
- `--tile 0` (default) picks the input tile side from the device's memory budget (200/100/64/32,
  as the reference tool); tiles overlap by 10 source pixels so seams match whole-frame output.
- `--fp16 auto` keeps ncnn's choice for the device. Winograd convolution is turned off on
  integrated GPUs (about 2× faster on Intel Xe, research R13).

## Data

- **stdin:** packed `rgb24` frames of exactly `W×H×3` bytes, back to back.
- **stdout:** packed `rgb24` frames of `(W·s)×(H·s)×3` bytes, in input order. There is one output
  frame per input frame.
- **End of stream:** EOF on stdin. The worker flushes, writes everything, then exits 0.
- **Partial input:** a short final frame is an error (exit 4).

## stderr: JSON lines

```
{"type":"device","index":0,"name":"…","kind":"discrete","fp16":true,"tile":200}
{"type":"progress","frames":120,"fps":7.9}
{"type":"selftest","frames":8,"width":480,"height":270,"scale":2,"sec_per_frame":0.127,"sec_per_mpx":0.98}
{"type":"error","code":"SHORT_FRAME","message":"…"}
```

`progress` is emitted at most every 250 ms, plus once at the end. Error codes: `USAGE` (2);
`NO_VULKAN`, `NO_DEVICE`, `SOFTWARE_DEVICE` (3); `SHORT_FRAME`, `READ`, `WRITE` (4); `MODEL_LOAD`
(5); `GPU_RUNTIME` (6). ncnn's own diagnostics are discarded (stderr is redirected to the null
device after the worker keeps a copy for its JSON lines), so every stderr line is JSON.
`--selftest` runs one untimed warm-up frame (pipeline creation), then times `--frames` frames.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 2 | usage |
| 3 | GPU unavailable or software device |
| 4 | input/output error (short final frame, closed pipe) |
| 5 | model load failure |
| 6 | GPU runtime error |
| 130/137 | killed |

## Cancellation

The core kills the whole pipeline (process group or Job Object). The worker does no cleanup of
its own and writes no files.

## Determinism

Output may differ between GPUs and drivers (fp16), but not between runs on the same device.
