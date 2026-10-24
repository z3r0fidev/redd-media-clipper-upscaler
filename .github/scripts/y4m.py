"""Writes a tiny Y4M clip for the gif-tools.yml smoke tests: 10 frames of 64x48 at 10 fps.

Usage: python -I y4m.py 420|444 OUT.y4m
"""

import sys

W, H, FRAMES = 64, 48, 10


def main() -> None:
    chroma, out = sys.argv[1], sys.argv[2]
    sub = {"420": (W // 2) * (H // 2), "444": W * H}[chroma]
    tag = {"420": "C420jpeg", "444": "C444"}[chroma]
    with open(out, "wb") as f:
        f.write(f"YUV4MPEG2 W{W} H{H} F10:1 Ip A1:1 {tag}\n".encode())
        for i in range(FRAMES):
            f.write(b"FRAME\n")
            # A bright bar moving across a dark frame, so frames differ.
            luma = bytearray(16 for _ in range(W * H))
            for y in range(H):
                for x in range(i * 6, min(W, i * 6 + 8)):
                    luma[y * W + x] = 235
            f.write(bytes(luma))
            f.write(bytes([128]) * (2 * sub))


if __name__ == "__main__":
    main()
