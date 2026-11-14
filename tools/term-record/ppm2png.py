#!/usr/bin/env python3
"""Converts a binary PPM (P6) to PNG with the standard library: ppm2png.py IN.ppm OUT.png"""
import struct
import sys
import zlib

data = open(sys.argv[1], "rb").read()
parts = data.split(b"\n", 3)
assert parts[0] == b"P6"
width, height = (int(v) for v in parts[1].split())
pixels = parts[3]
rows = b"".join(b"\x00" + pixels[y * width * 3 : (y + 1) * width * 3] for y in range(height))


def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
open(sys.argv[2], "wb").write(png)
