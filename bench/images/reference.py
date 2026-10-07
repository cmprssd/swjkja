"""Reference decodes for swjkja-image, using Pillow (libjpeg-turbo, libpng).

    python reference.py <GameData folder>      one line per texture, same format as
                                               `cargo run --example decode_all`
    python reference.py <GameData> --diff DIR  for each path on stdin, compare the
                                               reference pixels with DIR/<path>.rgba

Files resolve like the engine for base only: the later .pk3 (by name) wins. The
acceptance rules mirror the original loaders (see crates/image/src/lib.rs); a file
they reject prints "reject" here and must fail to decode in swjkja-image too.
"""
import io
import pathlib
import sys
import zipfile
import zlib

import numpy as np
from PIL import Image

EXTS = (".jpg", ".png", ".tga")


def files(gamedata):
    found = {}
    for pk3 in sorted((pathlib.Path(gamedata) / "base").glob("*.pk3"), key=lambda p: p.name.lower()):
        z = zipfile.ZipFile(pk3)
        for info in z.infolist():
            name = info.filename.replace("\\", "/").lower()
            if name.endswith(EXTS):
                found[name] = (z, info)
    return found


def rgba(name, data):
    """RGBA rows top first, or None where the original loader rejects the file."""
    if name.endswith(".png"):
        # IHDR: bit depth at 24, colour type at 25; only 8-bit RGB/RGBA, power-of-two.
        w, h = int.from_bytes(data[16:20], "big"), int.from_bytes(data[20:24], "big")
        if data[24] != 8 or data[25] not in (2, 6) or w & (w - 1) or h & (h - 1):
            return None
    if name.endswith(".tga"):
        return tga(data)
    img = Image.open(io.BytesIO(data))
    return np.asarray(img.convert("RGBA"))


def tga(data):
    """The original's TGA rules, decoded by hand: Pillow mirrors right-to-left images,
    which the original does not."""
    id_len, cmap_type, kind = data[0], data[1], data[2]
    w, h = int.from_bytes(data[12:14], "little"), int.from_bytes(data[14:16], "little")
    bits, desc = data[16], data[17]
    if cmap_type or kind not in (2, 3, 10) or bits not in (8, 24, 32) or (kind == 10 and (bits == 8 or desc & 0x30)):
        return None
    ch = bits // 8
    src = memoryview(data)[18 + id_len:]
    if kind == 10:
        out, i = bytearray(), 0
        while len(out) < w * h * ch:
            head = src[i]; i += 1
            n = (head & 0x7F) + 1
            if head & 0x80:
                out += bytes(src[i:i + ch]) * n; i += ch
            else:
                out += src[i:i + n * ch]; i += n * ch
        px = np.frombuffer(bytes(out[:w * h * ch]), np.uint8)
    else:
        px = np.frombuffer(src[:w * h * ch], np.uint8)
    px = px.reshape(h, w, ch)
    if ch == 1:
        px = np.concatenate([px, px, px, np.full_like(px, 255)], axis=2)
    elif ch == 3:
        px = np.concatenate([px[..., 2::-1], np.full((h, w, 1), 255, np.uint8)], axis=2)
    else:
        px = np.concatenate([px[..., 2::-1], px[..., 3:]], axis=2)
    return px if desc & 0x20 else px[::-1]


def main():
    gamedata = sys.argv[1]
    found = files(gamedata)
    if len(sys.argv) > 2 and sys.argv[2] == "--diff":
        dump = pathlib.Path(sys.argv[3])
        worst = {}
        for line in sys.stdin:
            name = line.strip()
            z, info = found[name]
            ref = rgba(name, z.read(info)).astype(np.int16)
            ours = np.fromfile(dump / f"{name}.rgba", np.uint8).reshape(ref.shape).astype(np.int16)
            d = np.abs(ref - ours)
            print(f"{name}\tmax {d.max()}\tmean {d.mean():.4f}\tdiffering {np.count_nonzero(d.max(axis=2))}/{d.shape[0] * d.shape[1]} pixels")
        return
    for name in sorted(found):
        z, info = found[name]
        try:
            px = rgba(name, z.read(info))
        except Exception as e:  # Pillow could not decode it
            print(f"{name}\terror: {e}\t\t\t")
            continue
        if px is None:
            print(f"{name}\treject\t\t\t")
        else:
            px = np.ascontiguousarray(px)
            print(f"{name}\tok\t{px.shape[1]}\t{px.shape[0]}\t{zlib.crc32(px.tobytes()):08x}")


if __name__ == "__main__":
    main()
