"""Draws the Dare client's own start-screen skins as Continuum-format .skn files.

usage: python3 -I make_skin.py "Dare" out.skn [--preview preview.png]
       python3 -I make_skin.py "Dare Night" out.skn

Everything here is drawn procedurally: panels, bevels, the lettering (our own
5x7 pixel font) and the animations. No third-party art is used. The output is a
24-bit BMP of the main region plus overlay frames, with the skin description
text appended, which is the layout Continuum's makeskin tool produced.
"""
import struct
import sys
import zlib

W, H = 640, 420  # main region

THEMES = {
    "Dare": {
        "space": ((4, 7, 14), (10, 18, 34)),
        "panel": (24, 29, 37),
        "panel_hi": (58, 69, 82),
        "panel_lo": (8, 10, 14),
        "well": (7, 9, 13),
        "accent": (34, 211, 238),
        "accent_dim": (14, 79, 92),
        "hot": (255, 138, 31),
        "hot_dim": (120, 56, 10),
        "logo": ((255, 214, 92), (255, 120, 24)),
        "text": (198, 214, 230),
        "zone": ((74, 222, 128), (250, 204, 21), (248, 113, 113)),
        "profile": ((34, 211, 238), (240, 240, 240), (250, 204, 21), (160, 174, 190)),
        "tagline": "SUBSPACE  ZONE",
        "title": "DARE",
        "subtitle": "OPEN SUBSPACE CLIENT",
        "list_title": "ZONES",
        "columns": ("PING", "ZONE", "PLAYERS"),
        "labels": {"zones": "REFRESH", "trace": "TRACE"},
        "prof_labels": ("PROFILE", "NAME", "MACROS", "KEYS"),
        "footer": "DARE ZONE  -  OSS-CLIENT",
    },
    "Dare Night": {
        "space": ((6, 3, 14), (20, 8, 34)),
        "panel": (26, 20, 38),
        "panel_hi": (70, 54, 96),
        "panel_lo": (10, 6, 16),
        "well": (9, 6, 14),
        "accent": (45, 212, 191),
        "accent_dim": (16, 80, 72),
        "hot": (192, 132, 252),
        "hot_dim": (76, 40, 112),
        "logo": ((216, 180, 254), (147, 51, 234)),
        "text": (214, 204, 232),
        "zone": ((45, 212, 191), (244, 196, 92), (244, 114, 182)),
        "profile": ((192, 132, 252), (240, 240, 240), (45, 212, 191), (170, 160, 190)),
        "tagline": "NIGHT  SHIFT",
        "title": "DARE",
        "subtitle": "OPEN SUBSPACE CLIENT",
        "list_title": "ZONES",
        "columns": ("PING", "ZONE", "PLAYERS"),
        "labels": {"zones": "REFRESH", "trace": "TRACE"},
        "prof_labels": ("PROFILE", "NAME", "MACROS", "KEYS"),
        "footer": "DARE ZONE  -  OSS-CLIENT",
    },
    # SubSpace Foundation's own look, in the site's colours (navy #070c15, panel #0d1624,
    # cyan #71e4fa). The default in the browser build.
    "Foundation": {
        "space": ((5, 9, 17), (9, 20, 38)),
        "panel": (13, 22, 36),
        "panel_hi": (52, 71, 98),
        "panel_lo": (7, 12, 21),
        "well": (6, 10, 18),
        "accent": (113, 228, 250),
        "accent_dim": (30, 78, 96),
        "hot": (113, 228, 250),
        "hot_dim": (22, 70, 88),
        "logo": ((210, 248, 255), (60, 170, 210)),
        "text": (200, 212, 230),
        "zone": ((113, 228, 250), (241, 246, 255), (174, 188, 208)),
        "profile": ((113, 228, 250), (241, 246, 255), (113, 228, 250), (174, 188, 208)),
        "tagline": "",
        "brand": "SUBSPACE FOUNDATION",
        "subtitle": "",
        "list_title": "FOUNDATION ARENAS",
        "columns": ("SORT", "ARENA", "PILOTS"),
        "labels": {"zones": "REFRESH", "trace": "+ CREATE"},
        "prof_labels": ("PROFILE", "NAME", "ARENA", "KEYS"),
        "footer": "SUBSPACEFOUNDATION.COM",
    },
}

# --- a 5x7 pixel font of our own ---------------------------------------------------
FONT = {
    "A": "01110 10001 10001 11111 10001 10001 10001",
    "B": "11110 10001 10001 11110 10001 10001 11110",
    "C": "01110 10001 10000 10000 10000 10001 01110",
    "D": "11110 10001 10001 10001 10001 10001 11110",
    "E": "11111 10000 10000 11110 10000 10000 11111",
    "F": "11111 10000 10000 11110 10000 10000 10000",
    "G": "01110 10001 10000 10111 10001 10001 01111",
    "H": "10001 10001 10001 11111 10001 10001 10001",
    "I": "01110 00100 00100 00100 00100 00100 01110",
    "J": "00111 00010 00010 00010 00010 10010 01100",
    "K": "10001 10010 10100 11000 10100 10010 10001",
    "L": "10000 10000 10000 10000 10000 10000 11111",
    "M": "10001 11011 10101 10101 10001 10001 10001",
    "N": "10001 11001 10101 10011 10001 10001 10001",
    "O": "01110 10001 10001 10001 10001 10001 01110",
    "P": "11110 10001 10001 11110 10000 10000 10000",
    "Q": "01110 10001 10001 10001 10101 10010 01101",
    "R": "11110 10001 10001 11110 10011 10001 10001",
    "S": "01111 10000 10000 01110 00001 00001 11110",
    "T": "11111 00100 00100 00100 00100 00100 00100",
    "U": "10001 10001 10001 10001 10001 10001 01110",
    "V": "10001 10001 10001 10001 10001 01010 00100",
    "W": "10001 10001 10001 10101 10101 10101 01010",
    "X": "10001 10001 01010 00100 01010 10001 10001",
    "Y": "10001 10001 01010 00100 00100 00100 00100",
    "Z": "11111 00001 00010 00100 01000 10000 11111",
    "0": "01110 10001 10011 10101 11001 10001 01110",
    "1": "00100 01100 00100 00100 00100 00100 01110",
    "2": "01110 10001 00001 00010 00100 01000 11111",
    "3": "11110 00001 00001 01110 00001 00001 11110",
    "4": "00010 00110 01010 10010 11111 00010 00010",
    "5": "11111 10000 11110 00001 00001 10001 01110",
    "6": "00110 01000 10000 11110 10001 10001 01110",
    "7": "11111 00001 00010 00100 01000 01000 01000",
    "8": "01110 10001 10001 01110 10001 10001 01110",
    "9": "01110 10001 10001 01111 00001 00010 01100",
    "+": "00000 00100 00100 11111 00100 00100 00000",
    " ": "00000 00000 00000 00000 00000 00000 00000",
    ".": "00000 00000 00000 00000 00000 01100 01100",
    "-": "00000 00000 00000 11111 00000 00000 00000",
    ":": "00000 01100 01100 00000 01100 01100 00000",
    "/": "00001 00010 00010 00100 01000 01000 10000",
    "<": "00010 00100 01000 10000 01000 00100 00010",
    ">": "01000 00100 00010 00001 00010 00100 01000",
}


class Canvas:
    def __init__(self, w, h, fill=(0, 0, 0)):
        self.w, self.h = w, h
        self.px = [list(fill) * w for _ in range(h)]

    def get(self, x, y):
        r = self.px[y]
        return (r[3 * x], r[3 * x + 1], r[3 * x + 2])

    def set(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            r = self.px[y]
            r[3 * x], r[3 * x + 1], r[3 * x + 2] = c

    def blend(self, x, y, c, a):
        if 0 <= x < self.w and 0 <= y < self.h and a > 0:
            o = self.get(x, y)
            self.set(x, y, tuple(int(o[i] + (c[i] - o[i]) * min(a, 1.0)) for i in range(3)))

    def rect(self, x0, y0, x1, y1, c):
        for y in range(max(y0, 0), min(y1 + 1, self.h)):
            for x in range(max(x0, 0), min(x1 + 1, self.w)):
                self.set(x, y, c)

    def copy_from(self, src, sx, sy, w, h, dx, dy):
        for y in range(h):
            for x in range(w):
                self.set(dx + x, dy + y, src.get(sx + x, sy + y))


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def text_width(s, scale=1):
    return len(s) * 6 * scale - scale


def text(c, x, y, s, color, scale=1):
    for i, ch in enumerate(s.upper()):
        g = FONT.get(ch, FONT[" "]).split()
        for gy, row in enumerate(g):
            for gx, bit in enumerate(row):
                if bit == "1":
                    c.rect(x + (i * 6 + gx) * scale, y + gy * scale,
                           x + (i * 6 + gx) * scale + scale - 1, y + gy * scale + scale - 1, color)


def text_center(c, cx, y, s, color, scale=1):
    text(c, cx - text_width(s, scale) // 2, y, s, color, scale)


def bevel(c, x0, y0, x1, y1, t, inset=False):
    hi, lo = (t["panel_lo"], t["panel_hi"]) if inset else (t["panel_hi"], t["panel_lo"])
    c.rect(x0, y0, x1, y0, hi)
    c.rect(x0, y0, x0, y1, hi)
    c.rect(x0, y1, x1, y1, lo)
    c.rect(x1, y0, x1, y1, lo)


def panel(c, x0, y0, x1, y1, t, title=None):
    for y in range(y0, y1 + 1):
        c.rect(x0, y, x1, y, mix(t["panel"], t["panel_lo"], (y - y0) / max(1, y1 - y0) * 0.5))
    bevel(c, x0, y0, x1, y1, t)
    c.rect(x0 + 2, y0 + 2, x1 - 2, y0 + 2, t["accent_dim"])
    for (rx, ry) in ((x0 + 4, y0 + 5), (x1 - 5, y0 + 5), (x0 + 4, y1 - 5), (x1 - 5, y1 - 5)):
        c.rect(rx, ry, rx + 1, ry + 1, t["panel_hi"])
        c.set(rx + 1, ry + 1, t["panel_lo"])
    if title:
        tw = text_width(title) + 10
        c.rect(x0 + 10, y0 - 4, x0 + 10 + tw, y0 + 5, t["panel_lo"])
        bevel(c, x0 + 10, y0 - 4, x0 + 10 + tw, y0 + 5, t)
        text(c, x0 + 15, y0 - 2, title, t["accent"])


def well(c, x0, y0, x1, y1, t, scan=False):
    c.rect(x0, y0, x1, y1, t["well"])
    bevel(c, x0, y0, x1, y1, t, inset=True)
    if scan:
        for y in range(y0 + 2, y1, 2):
            c.rect(x0 + 1, y, x1 - 1, y, mix(t["well"], t["panel"], 0.25))


def button(c, x0, y0, x1, y1, label, t, state, hot=False):
    """state: 0 = resting (main), 1 = lit (ALT), 2 = pulse (NORM blink)."""
    base = t["hot"] if hot else t["accent"]
    dim = t["hot_dim"] if hot else t["accent_dim"]
    for y in range(y0, y1 + 1):
        f = (y - y0) / max(1, y1 - y0)
        if state == 0:
            col = mix(mix(t["panel"], dim, 0.35), t["panel_lo"], f * 0.6)
        elif state == 2:
            col = mix(mix(dim, base, 0.45), dim, f)
        else:
            col = mix(mix(base, (255, 255, 255), 0.15), dim, f * 0.8)
        c.rect(x0, y, x1, y, col)
    edge = base if state else dim
    c.rect(x0, y0, x1, y0, edge)
    c.rect(x0, y1, x1, y1, t["panel_lo"])
    c.rect(x0, y0, x0, y1, edge)
    c.rect(x1, y0, x1, y1, t["panel_lo"])
    fg = (255, 255, 255) if state == 1 else (mix(base, (255, 255, 255), 0.5) if state == 2 else base)
    scale = 2 if (y1 - y0) >= 24 else 1
    text_center(c, (x0 + x1) // 2 + 1, (y0 + y1) // 2 - (7 * scale) // 2 + 1, label, fg, scale)


def arrow(c, x0, y0, x1, y1, up, t, state):
    """state 0: disabled (main), 1: available (NORM), 2: hovered (ALT)."""
    c.rect(x0, y0, x1, y1, t["panel_lo"] if state == 0 else mix(t["panel"], t["accent_dim"], 0.5))
    bevel(c, x0, y0, x1, y1, t)
    col = {0: t["panel_hi"], 1: t["accent"], 2: (255, 255, 255)}[state]
    cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
    for i in range(4):
        yy = cy - 2 + i if up else cy + 1 - i
        c.rect(cx - i, yy, cx + i, yy, col)


def side_arrow(c, x0, y0, x1, y1, left, t, state):
    c.rect(x0, y0, x1, y1, t["panel_lo"] if state == 0 else mix(t["panel"], t["accent_dim"], 0.5))
    bevel(c, x0, y0, x1, y1, t)
    col = {0: t["panel_hi"], 1: t["accent"], 2: (255, 255, 255)}[state]
    cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
    for i in range(4):
        xx = cx - 2 + i if left else cx + 1 - i
        c.rect(xx, cy - i, xx, cy + i, col)


def bay(c, x0, y0, x1, y1, t, lit):
    well(c, x0, y0, x1, y1, t)
    col = t["accent"] if lit else t["accent_dim"]
    for (cx, cy, dx, dy) in ((x0, y0, 1, 1), (x1, y0, -1, 1), (x0, y1, 1, -1), (x1, y1, -1, -1)):
        for i in range(5):
            c.set(cx + dx * i, cy, col)
            c.set(cx, cy + dy * i, col)
    if lit:
        for i in range(1, 4):
            a = 0.35 / i
            for x in range(x0 + i, x1 - i + 1):
                c.blend(x, y0 + i, t["accent"], a)
                c.blend(x, y1 - i, t["accent"], a)
            for y in range(y0 + i, y1 - i + 1):
                c.blend(x0 + i, y, t["accent"], a)
                c.blend(x1 - i, y, t["accent"], a)


def starfield(c, t, seed):
    top, bottom = t["space"]
    for y in range(c.h):
        c.rect(0, y, c.w - 1, y, mix(top, bottom, y / c.h))
    s = seed
    for _ in range(420):
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        x = s % c.w
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        y = s % c.h
        s = (s * 1103515245 + 12345) & 0x7FFFFFFF
        b = 60 + s % 160
        c.set(x, y, (b, b, min(255, b + 30)))


def logo(c, cx, y, word, t, scale=6):
    top, bottom = t["logo"]
    w = text_width(word, scale)
    x0 = cx - w // 2
    glyph = Canvas(w + 8, 7 * scale + 8)
    mask = [[0] * glyph.w for _ in range(glyph.h)]
    tmp = Canvas(w, 7 * scale, (0, 0, 0))
    text(tmp, 0, 0, word, (255, 255, 255), scale)
    for yy in range(tmp.h):
        for xx in range(tmp.w):
            if tmp.get(xx, yy) == (255, 255, 255):
                mask[yy + 4][xx + 4] = 1
    # glow, outline, fill
    for yy in range(glyph.h):
        for xx in range(glyph.w):
            if mask[yy][xx]:
                continue
            near = 0
            for dy in range(-4, 5):
                for dx in range(-4, 5):
                    ny, nx = yy + dy, xx + dx
                    if 0 <= ny < glyph.h and 0 <= nx < glyph.w and mask[ny][nx]:
                        near = max(near, 1 - (dx * dx + dy * dy) ** 0.5 / 5.5)
            if near > 0:
                c.blend(x0 - 4 + xx, y - 4 + yy, bottom, near * 0.6)
    def m(yy, xx):
        return 0 <= yy < glyph.h and 0 <= xx < glyph.w and mask[yy][xx]

    for yy in range(glyph.h):
        for xx in range(glyph.w):
            if not m(yy, xx):
                continue
            f = (yy - 4) / (7 * scale)
            col = mix(top, bottom, f)
            if not m(yy - 1, xx) or not m(yy, xx - 1):
                col = mix(col, (255, 255, 255), 0.55)  # lit top-left edge
            elif not m(yy + 1, xx) or not m(yy, xx + 1):
                col = mix(col, (60, 20, 0), 0.55)  # shaded bottom-right edge
            c.set(x0 - 4 + xx, y - 4 + yy, col)
    # a dark outline one pixel outside the letters
    for yy in range(glyph.h):
        for xx in range(glyph.w):
            if not m(yy, xx) and any(m(yy + dy, xx + dx) for dy in (-1, 0, 1) for dx in (-1, 0, 1)):
                c.set(x0 - 4 + xx, y - 4 + yy, (20, 10, 4))


def brand(c, t):
    """The Foundation title: a cyan diamond mark and the brand name in the logo style."""
    cx, cy, r = 34, 26, 15
    for yy in range(cy - r, cy + r + 1):
        for xx in range(cx - r, cx + r + 1):
            d = abs(xx - cx) + abs(yy - cy)
            if d <= r:
                edge = d >= r - 2
                inner = d <= r - 7
                if edge or inner:
                    c.set(xx, yy, t["accent"] if edge else mix(t["accent"], (255, 255, 255), 0.35))
    logo(c, 62 + text_width(t["brand"], 3) // 2, 14, t["brand"], t, scale=3)


# --- layout -------------------------------------------------------------------------
# Rectangles are inclusive (x0, y0, x1, y1), as the skin text format has them.
PROFBOX = (12, 62, 292, 160)
PROFILE = (22, 70, 112, 84)
PREVPROF = (240, 70, 256, 84)
NEXTPROF = (262, 70, 278, 84)
PROFTEXT = (96, 90, 279, 153)
SHIPBOX = (12, 170, 292, 330)
BAYS = [(26 + (i % 4) * 66, 182 + (i // 4) * 50, 26 + (i % 4) * 66 + 43, 182 + (i // 4) * 50 + 43) for i in range(8)]
SPECTATE = (130, 282, 173, 325)
ZONEBOX = (300, 62, 628, 330)
ZONES = (310, 70, 400, 84)
TRACE = (408, 70, 480, 84)
PING = (312, 92, 344, 102)
ZONETEXT = (314, 106, 601, 321)
ZONEUP = (606, 106, 622, 122)
ZONEDOWN = (606, 305, 622, 321)
INFOBOX = (308, 346, 516, 405)
OPTIONS = (12, 372, 292, 394)
KEYDEFS = (12, 344, 148, 366)
MACROS = (156, 344, 292, 366)
PLAY = (524, 342, 628, 380)
QUIT = (524, 386, 628, 408)
SWEEP = (20, 52, 619, 53)

SWEEP_FRAMES = 10


def draw_main(t):
    c = Canvas(W, H)
    starfield(c, t, 11 if t["tagline"].startswith("N") else 7)
    # title bar
    c.rect(0, 0, W - 1, 50, mix(t["panel_lo"], t["space"][0], 0.3))
    c.rect(0, 50, W - 1, 50, t["accent_dim"])
    c.rect(0, 54, W - 1, 54, t["panel_lo"])
    if "brand" in t:
        brand(c, t)
    else:
        logo(c, 150, 8, t["title"], t)
        text(c, 300, 14, t["tagline"], t["text"], 2)
        text(c, 302, 34, t["subtitle"], t["accent"])
    # sweep track
    c.rect(SWEEP[0], SWEEP[1], SWEEP[2], SWEEP[3], t["panel_lo"])
    # profile
    panel(c, *PROFBOX, t, "PILOT")
    button(c, *PROFILE, "PROFILE", t, 0)
    side_arrow(c, *PREVPROF, True, t, 0)
    side_arrow(c, *NEXTPROF, False, t, 0)
    well(c, 22, PROFTEXT[1] - 2, PROFTEXT[2] + 2, PROFTEXT[3] + 1, t, scan=True)
    for i, label in enumerate(t["prof_labels"]):
        text(c, 30, PROFTEXT[1] + 4 + i * 16, label, t["accent"])
    # ships
    panel(c, *SHIPBOX, t, "SHIP")
    for i, b in enumerate(BAYS):
        bay(c, *b, t, False)
        text(c, b[0] + 2, b[1] + 2, str(i + 1), t["accent_dim"])
    bay(c, *SPECTATE, t, False)
    text(c, SPECTATE[2] + 8, SPECTATE[1] + 18, "SPECTATE", t["text"])
    # zones
    panel(c, *ZONEBOX, t, t["list_title"])
    button(c, *ZONES, t["labels"]["zones"], t, 0)
    button(c, *TRACE, t["labels"]["trace"], t, 0)
    well(c, ZONETEXT[0] - 2, ZONETEXT[1] - 2, ZONETEXT[2] + 2, ZONETEXT[3] + 2, t, scan=True)
    ping, name, count = t["columns"]
    text(c, PING[0] + 2, PING[1] + 2, ping, t["accent"])
    text(c, 356, PING[1] + 2, name, t["accent"])
    text(c, 601 - text_width(count), PING[1] + 2, count, t["accent"])
    c.rect(ZONEUP[0] + 7, ZONEUP[3] + 2, ZONEUP[0] + 9, ZONEDOWN[1] - 2, t["panel_lo"])
    arrow(c, *ZONEUP, True, t, 0)
    arrow(c, *ZONEDOWN, False, t, 0)
    # bottom
    panel(c, 300, 338, 628, 412, t)
    well(c, INFOBOX[0] - 2, INFOBOX[1] - 2, INFOBOX[2] + 2, INFOBOX[3] + 2, t)
    panel(c, 12, 338, 292, 412, t)
    button(c, *KEYDEFS, "KEYS", t, 0)
    button(c, *MACROS, "MACROS", t, 0)
    button(c, *OPTIONS, "OPTIONS", t, 0)
    text(c, 26, 400, t["footer"], t["panel_hi"])
    button(c, *PLAY, "PLAY", t, 0, hot=True)
    button(c, *QUIT, "QUIT", t, 0)
    return c


class Atlas:
    """Shelf-packs overlay frames below the main region."""

    def __init__(self, width):
        self.width = width
        self.frames = []  # (canvas, x, y)
        self.x, self.y, self.row_h = 0, H + 1, 0

    def add(self, img):
        if self.x + img.w > self.width:
            self.x, self.y, self.row_h = 0, self.y + self.row_h + 1, 0
        pos = (self.x, self.y)
        self.frames.append((img, pos))
        self.x += img.w + 1
        self.row_h = max(self.row_h, img.h)
        return pos

    def height(self):
        return self.y + self.row_h + 1


def piece(main, rect, draw):
    """A copy of `rect` of the main image with `draw(canvas, offset)` painted on it."""
    x0, y0, x1, y1 = rect
    img = Canvas(x1 - x0 + 1, y1 - y0 + 1)
    img.copy_from(main, x0, y0, img.w, img.h, 0, 0)
    draw(img, -x0, -y0)
    return img


def shifted(fn, rect, *args, **kw):
    x0, y0, x1, y1 = rect
    return lambda img, dx, dy: fn(img, x0 + dx, y0 + dy, x1 + dx, y1 + dy, *args, **kw)


def build(name):
    t = THEMES[name]
    main = draw_main(t)
    atlas = Atlas(W)
    lines = [f"; {name} - a start-screen skin for the {'Foundation' if 'brand' in t else 'Dare'} client", "; Original art, drawn by make_skin.py.", "",
             f"main = (0,0,{W - 1},{H - 1})"]

    def r(rect):
        return "(%d,%d,%d,%d)" % rect

    def p(pos):
        return "(%d,%d)" % pos

    def btn(spot, rect, label, hot=False, blink=False):
        alt = atlas.add(piece(main, rect, shifted(button, rect, label, t, 1, hot=hot)))
        norm = ""
        if blink:
            pulse = atlas.add(piece(main, rect, shifted(button, rect, label, t, 2, hot=hot)))
            norm = f" 4{p(pulse)} 4(*)"
        lines.append(f"{spot} = {r(rect)}{norm} + {p(alt)}")

    btn("profile", PROFILE, "PROFILE")
    btn("zones", ZONES, t["labels"]["zones"])
    btn("trace", TRACE, t["labels"]["trace"])
    btn("keydefs", KEYDEFS, "KEYS")
    btn("macros", MACROS, "MACROS")
    btn("options", OPTIONS, "OPTIONS")
    btn("play", PLAY, "PLAY", hot=True, blink=True)
    btn("quit", QUIT, "QUIT")

    for spot, rect, up in (("zoneup", ZONEUP, True), ("zonedown", ZONEDOWN, False)):
        n = atlas.add(piece(main, rect, shifted(arrow, rect, up, t, 1)))
        a = atlas.add(piece(main, rect, shifted(arrow, rect, up, t, 2)))
        lines.append(f"{spot} = {r(rect)} {p(n)} + {p(a)}")
    for spot, rect, left in (("prevprof", PREVPROF, True), ("nextprof", NEXTPROF, False)):
        n = atlas.add(piece(main, rect, shifted(side_arrow, rect, left, t, 1)))
        a = atlas.add(piece(main, rect, shifted(side_arrow, rect, left, t, 2)))
        lines.append(f"{spot} = {r(rect)} {p(n)} + {p(a)}")

    lit_bays = {}
    for i, b in enumerate(BAYS):
        def lit(img, dx, dy, b=b, i=i):
            bay(img, b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy, t, True)
            text(img, b[0] + dx + 2, b[1] + dy + 2, str(i + 1), t["accent"])
        lit_bays[i] = atlas.add(piece(main, b, lit))
        lines.append(f"ship{i + 1} = {r(b)} + {p(lit_bays[i])}")
    spec_lit = atlas.add(piece(main, SPECTATE, shifted(bay, SPECTATE, t, True)))
    lines.append(f"spectate = {r(SPECTATE)} + {p(spec_lit)}")
    lines.append(f"spectate = {r((SPECTATE[2] + 1, SPECTATE[1], SPECTATE[2] + 60, SPECTATE[3]))}")

    def ping_lit(img, dx, dy):
        text(img, PING[0] + 2 + dx, PING[1] + 2 + dy, t["columns"][0], (255, 255, 255))
    lines.append(f"ping = {r(PING)} + {p(atlas.add(piece(main, PING, ping_lit)))}")

    # light sweep along the title bar: a bright pulse travelling left to right
    sx0, sy0, sx1, sy1 = SWEEP
    sweep = []
    for k in range(SWEEP_FRAMES):
        img = Canvas(sx1 - sx0 + 1, sy1 - sy0 + 1)
        img.copy_from(main, sx0, sy0, img.w, img.h, 0, 0)
        centre = int((k + 0.5) / SWEEP_FRAMES * img.w)
        for x in range(img.w):
            a = max(0.0, 1 - abs(x - centre) / 70)
            for y in range(img.h):
                img.blend(x, y, t["accent"], a)
                if a > 0.85:
                    img.blend(x, y, (255, 255, 255), (a - 0.85) * 4)
        sweep.append(atlas.add(img))
    lines.append("user1 = %s %s 6(*)" % (r(SWEEP), " ".join(p(s) for s in sweep)))

    lines += [
        f"proftext = {r(PROFTEXT)}",
        f"profbox = {r(PROFBOX)}",
        f"shipbox = {r(SHIPBOX)}",
        f"zonetext = {r(ZONETEXT)}",
        f"zonebox = {r(ZONEBOX)}",
        f"infobox = {r(INFOBOX)}",
        "",
    ]
    for i, col in enumerate(t["zone"]):
        lines.append("zonecolor%d = (%d,%d,%d)" % ((i + 1,) + col))
    sel = t["accent_dim"]
    lines.append("zoneselcolor1 = (%d,%d,%d)" % sel)
    lines.append("zoneselcolor2 = (%d,%d,%d)" % mix(sel, t["accent"], 0.4))
    for i, col in enumerate(t["profile"]):
        lines.append("profilecolor%d = (%d,%d,%d)" % ((i + 1,) + col))

    full = Canvas(W, atlas.height(), (255, 0, 255))
    full.copy_from(main, 0, 0, W, H, 0, 0)
    for img, (x, y) in atlas.frames:
        full.copy_from(img, 0, 0, img.w, img.h, x, y)
    return full, "\r\n".join(lines) + "\r\n", main


def bmp24(c):
    row = (c.w * 3 + 3) & ~3
    size = 54 + row * c.h
    out = bytearray(b"BM" + struct.pack("<IHHI", size, 0, 0, 54))
    out += struct.pack("<IiiHHIIiiII", 40, c.w, c.h, 1, 24, 0, row * c.h, 2835, 2835, 0, 0)
    for y in range(c.h - 1, -1, -1):
        r = c.px[y]
        line = bytearray()
        for x in range(c.w):
            line += bytes((r[3 * x + 2], r[3 * x + 1], r[3 * x]))
        line += b"\0" * (row - len(line))
        out += line
    return bytes(out)


def png(c, path):
    raw = b"".join(b"\0" + bytes(row) for row in c.px)

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", c.w, c.h, 8, 2, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)


def main():
    args = sys.argv[1:]
    preview = None
    if "--preview" in args:
        i = args.index("--preview")
        preview = args[i + 1]
        del args[i:i + 2]
    name, out = args
    full, txt, main_img = build(name)
    with open(out, "wb") as f:
        f.write(bmp24(full) + txt.encode("latin-1"))
    if preview:
        png(full, preview)
    print(f"wrote {out} ({full.w}x{full.h})")


if __name__ == "__main__":
    main()
