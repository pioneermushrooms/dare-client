"""Make a 1024x1024 app icon PNG from a ship frame in an 8-bit BMP sprite sheet.

usage: make_icon.py ships.bm2 out.png [ship_index]
Pixel art is scaled with nearest-neighbour onto a dark rounded tile.
"""
import struct, sys, zlib

src, out = sys.argv[1], sys.argv[2]
ship = int(sys.argv[3]) if len(sys.argv) > 3 else 0
data = open(src, "rb").read()
off = struct.unpack_from("<I", data, 10)[0]
hdr = struct.unpack_from("<I", data, 14)[0]
w, h = struct.unpack_from("<ii", data, 18)
bpp = struct.unpack_from("<H", data, 28)[0]
assert bpp == 8, bpp
ncol = struct.unpack_from("<I", data, 46)[0] or 256
pal_off = 14 + hdr
pal = [data[pal_off + i * 4: pal_off + i * 4 + 3][::-1] for i in range(ncol)]  # BGR -> RGB
row = (w + 3) & ~3
bottom_up = h > 0
h = abs(h)


def px(x, y):
    yy = (h - 1 - y) if bottom_up else y
    return data[off + yy * row + x]


F = 36
fx, fy = 0, ship * 144  # frame 0 of the chosen ship
frame = [[px(fx + x, fy + y) for x in range(F)] for y in range(F)]

S = 1024
scale = 24  # 36*24 = 864
pad = (S - F * scale) // 2
bg = (14, 22, 14)
radius = 180


def inside_round(x, y):
    m = 40  # margin like macOS icons
    if x < m or y < m or x >= S - m or y >= S - m:
        return False
    cx = min(max(x, m + radius), S - m - radius - 1)
    cy = min(max(y, m + radius), S - m - radius - 1)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius * radius


rows = []
for y in range(S):
    line = bytearray([0])
    for x in range(S):
        if not inside_round(x, y):
            line += b"\0\0\0\0"
            continue
        r, g, b = bg
        fx_, fy_ = (x - pad) // scale, (y - pad) // scale
        if 0 <= fx_ < F and 0 <= fy_ < F:
            i = frame[fy_][fx_]
            if i != 0:
                r, g, b = pal[i]
        line += bytes((r, g, b, 255))
    rows.append(bytes(line))


def chunk(t, d):
    c = struct.pack(">I", len(d)) + t + d
    return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
open(out, "wb").write(png)
print("wrote", out)
