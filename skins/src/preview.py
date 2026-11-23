"""Mock-up of a skin as the client will show it: ships in the bays, sample profile
and zone text, ship 1 selected and Play hovered. For looking at, not shipping.

usage: python3 -I preview.py "Dare" ships.bm2 out.png [zoom]
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_skin as m  # noqa: E402


def ship_frames(path):
    d = open(path, "rb").read()
    off = struct.unpack_from("<I", d, 10)[0]
    hdr = struct.unpack_from("<I", d, 14)[0]
    w, h = struct.unpack_from("<ii", d, 18)
    pal_off = 14 + hdr
    pal = [tuple(d[pal_off + i * 4: pal_off + i * 4 + 3][::-1]) for i in range(256)]
    row = (w + 3) & ~3

    def px(x, y):
        return d[off + (abs(h) - 1 - y) * row + x] if h > 0 else d[off + y * row + x]

    return [[[(pal[px(x, k * 144 + y)] if px(x, k * 144 + y) else None) for x in range(36)]
             for y in range(36)] for k in range(8)]


def main():
    name, ships_path, out = sys.argv[1:4]
    zoom = int(sys.argv[4]) if len(sys.argv) > 4 else 2
    t = m.THEMES[name]
    img = m.draw_main(t)
    frames = ship_frames(ships_path)

    def paste(frame, cx, cy):
        for y in range(36):
            for x in range(36):
                if frame[y][x]:
                    img.set(cx - 18 + x, cy - 18 + y, frame[y][x])

    m.bay(img, *m.BAYS[0], t, True)
    m.text(img, m.BAYS[0][0] + 2, m.BAYS[0][1] + 2, "1", t["accent"])
    for i, b in enumerate(m.BAYS):
        paste(frames[i], (b[0] + b[2]) // 2, (b[1] + b[3]) // 2)
    m.button(img, *m.PLAY, "PLAY", t, 1, hot=True)

    p = t["profile"]
    x, y = m.PROFTEXT[0] + 4, m.PROFTEXT[1] + 4
    for i, line in enumerate(["DEFAULT", "CHRIS", "DEFAULT", "DEFAULT"]):
        m.text(img, x, y + i * 16, line, p[i])

    zones = [("Dare", 4, 0), ("ISOJ QS Playground", 69, 1), ("SSCU Trench Wars", 51, 1), ("SSCE Hyperspace", 35, 1),
             ("SSCJ Devastation", 9, 1), ("Subspace Reloaded", 7, 2), ("SSCU Death Star", 6, 1),
             ("ISOJ PowerBall", 5, 1), ("SSCU Extreme Games", 4, 1), ("SSCX Omega Fire", 4, 2),
             ("T3 The Gauntlet", 2, 2), ("SSCJ MetalGear CTF", 1, 1)]
    zx, zy = m.ZONETEXT[0] + 2, m.ZONETEXT[1] + 3
    sel = t["accent_dim"]
    img.rect(m.ZONETEXT[0], zy - 2, m.ZONETEXT[2], zy + 10, sel)
    for i, (zn, n, band) in enumerate(zones):
        ping = (48, 122, 310)[band]
        col = t["zone"][band]
        m.text(img, zx, zy + i * 14, "%3d" % ping, col)
        m.text(img, zx + 40, zy + i * 14, zn.upper(), (255, 255, 255) if i == 0 else t["text"])
        m.text(img, m.ZONETEXT[2] - 20, zy + i * 14, "%3d" % n, col)
    m.text(img, m.INFOBOX[0] + 4, m.INFOBOX[1] + 4, "PLAY: JOIN THE SELECTED", t["text"])
    m.text(img, m.INFOBOX[0] + 4, m.INFOBOX[1] + 16, "ZONE AS CHRIS IN A", t["text"])
    m.text(img, m.INFOBOX[0] + 4, m.INFOBOX[1] + 28, "WARBIRD.", t["text"])

    z = m.Canvas(img.w * zoom, img.h * zoom)
    for yy in range(z.h):
        r = img.px[yy // zoom]
        z.px[yy] = [r[3 * (xx // zoom) + k] for xx in range(z.w) for k in range(3)]
    m.png(z, out)
    print("wrote", out)


if __name__ == "__main__":
    main()
