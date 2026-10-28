# ncnn notice (AI worker)

The AI upscaling worker `rmedia-sr` (a separate program run by rmedia; feature 002 US6) is
statically linked with these libraries:

- **ncnn** `20260526` by Tencent: BSD 3-Clause, with third-party parts under zlib, BSD 2-Clause
  and BSD 3-Clause terms. Full text: `NCNN-LICENSE.txt` (from
  https://github.com/Tencent/ncnn/blob/20260526/LICENSE.txt). On Linux, Tencent's prebuilt
  `ncnn-20260526-ubuntu-2204.zip`. On Windows, `ncnn-20260526-windows-msvc-static.zip`, built
  by this project from Tencent's unmodified `ncnn-20260526-full-source.zip` (MSVC, static C
  runtime, Vulkan, no OpenMP; `.github/workflows/ncnn-static.yml`), published with that source
  as the release `ncnn-static-2026-10-28`. Both are pinned by SHA-256 in `xtask/models.lock.json`.
- **glslang**, as bundled by ncnn 20260526 (https://github.com/nihui/glslang at
  `fe88f421038e1bb0a25cd5c1b2dfe505db82d08f`): BSD 3-Clause, MIT, Apache 2.0 and other permissive
  terms for its parts. Full text: `GLSLANG-LICENSE.txt`.
- On Linux, the GCC runtime libraries `libstdc++` and `libgomp`, under the GPL version 3 with the
  **GCC Runtime Library Exception 3.1**, which permits static linking into programs under any
  licence. On Windows, the Microsoft C and C++ runtime libraries, linked statically as Visual Studio's
  licence allows for distributable code; no OpenMP runtime.

The model weights the worker runs (Real-ESRGAN animevideov3, BSD 3-Clause) are listed in
`THIRD_PARTY_NOTICES.md` with their licence (`licenses/models/real-esrgan-BSD-3.txt`).
