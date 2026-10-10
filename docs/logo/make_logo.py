#!/usr/bin/env python3
# Copyright (c) 2026 The SeedLock authors. All rights reserved.
# Like the logo it draws, this file is NOT covered by the MIT license of the rest of SeedLock: see README.md here.
"""Makes SeedLock's logo files: a round bird cage (porcelain white, with a thin slate line along the middle of every
white line) holding a striped sunflower seed, on slate, and the word SeedLock in IBM Plex Sans Bold ("Seed" in copper,
"Lock" in porcelain white).

The letters are written as outlines, not as text: GitHub shows a README's SVG as an image, and an image cannot load
a font, so live text would fall back to whatever font the viewer has.

Needs: pip install fonttools uharfbuzz, the font IBM Plex Sans Bold (SIL Open Font License; on Debian and Ubuntu:
sudo apt install fonts-ibm-plex), and for the PNG files rsvg-convert (sudo apt install librsvg2-bin).
Run:   python3 docs/logo/make_logo.py [OUT_DIR, by default this folder] [--font path/to/IBMPlexSans-Bold.ttf]
"""
import argparse
import math
import os
import shutil
import subprocess
import sys
import zlib

import uharfbuzz as hb
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

SLATE, PORCELAIN, COPPER = "#34454c", "#fffefc", "#bb7125"
FONT_PLACES = ["/usr/share/fonts/truetype/ibm-plex/IBMPlexSans-Bold.ttf", "/usr/share/fonts/opentype/ibm-plex/IBMPlexSans-Bold.otf"]


def num(x, places=2):
    return f"{x:.{places}f}".rstrip("0").rstrip(".")


# ---------------------------------------------------------------- the cage
# Drawn after the cage the project's author sketched, measured from the sketch. Every line is a porcelain line LINE
# wide with a slate line INLINE wide along its middle. Units: the sketch's pixels, x = 0 on the cage's middle line.
LINE, INLINE = 22, 5.2
OUTER, INNER = 203.5, 69.5                       # the bars' middle lines: the outer pair and the inner pair (x = -/+)
BAND_TOP, BAND_LOW = 474.5, 683.5                # the two bands' middle lines (y)
BAND_DARK, BAND_WHITE = 218.5, 215.5             # how far the bands reach: their slate line, and their porcelain line (round ends)
APEX = 281                                       # where the dome's arcs meet, under the neck
DOME_CY = APEX + OUTER                           # the outer dome: a circle of radius OUTER, centred here
ARCH_CY = 499.3                                  # the inner arch: half an ellipse INNER wide, from here up to the APEX
RING_CY, RING_R = 198.5, 33.5                    # the ring the cage hangs from (its middle line)
BASE_X, BASE_Y, BASE_W, BASE_H = -236.5, 729, 473, 41.5   # the base: a pill
BASE_LINES, BASE_LINE_X = (741, 759), 225.5      # its two slate lines (y), reaching from -BASE_LINE_X to BASE_LINE_X
SEED_SCALE = 1.642                               # the seed is drawn at 1/1.642 of these units
STRIPES, STRIPE_WIDTH = 3, 4.2                   # the seed's stripes: how many, and how wide at their widest
# What the cage covers (with the line widths): from the ring's top to the base's bottom.
CAGE_BOX = (BASE_X, RING_CY - RING_R - LINE / 2, BASE_X + BASE_W, BASE_Y + BASE_H)


def bezier(p0, p1, p2, p3, t):
    return tuple((1 - t) ** 3 * a + 3 * (1 - t) ** 2 * t * b + 3 * (1 - t) * t ** 2 * c + t ** 3 * d for a, b, c, d in zip(p0, p1, p2, p3))


def sunflower_seed(cx, cy, w=25.5, h=55, angle=-14):
    """A sunflower seed: round at the top, pointed at the bottom, with stripes along it (cut out in slate) that follow
    its outline, taper at both ends and meet towards the tip. Tilted by `angle` and placed so that its actual outline,
    not its middle point, is centred on (cx, cy)."""
    # the outline: four curves from the tip at the bottom, round over the top, back to the tip
    segments = [
        [(0, h), (w * .7, h * .6), (w * 1.02, h * .02), (w * .95, -h * .38)],
        [(w * .95, -h * .38), (w * .86, -h * .82), (w * .45, -h), (0, -h)],
        [(0, -h), (-w * .45, -h), (-w * .86, -h * .82), (-w * .95, -h * .38)],
        [(-w * .95, -h * .38), (-w * 1.02, h * .02), (-w * .7, h * .6), (0, h)],
    ]
    outline = [bezier(*seg, i / 60) for seg in segments for i in range(61)]
    rad = math.radians(angle)
    turned = [(x * math.cos(rad) - y * math.sin(rad), x * math.sin(rad) + y * math.cos(rad)) for x, y in outline]
    xs, ys = [p[0] for p in turned], [p[1] for p in turned]
    tx, ty = cx - (min(xs) + max(xs)) / 2, cy - (min(ys) + max(ys)) / 2
    body = f"M{num(segments[0][0][0])} {num(segments[0][0][1])}" + "".join(
        "C" + " ".join(f"{num(x)} {num(y)}" for x, y in seg[1:]) for seg in segments) + "Z"

    # the seed's half width at height y, from its right side (the first two curves, tip to top)
    right = sorted((y, x) for x, y in outline[:122])

    def half_width(y):
        for (y0, x0), (y1, x1) in zip(right, right[1:]):
            if y0 <= y <= y1:
                return x0 + (x1 - x0) * (y - y0) / (y1 - y0) if y1 > y0 else x0
        return 0.0

    stripes = []
    for k in range(STRIPES):
        f = (k + 0.5) / STRIPES * 2 - 1                         # its place across the seed, -1 (left edge) .. 1 (right edge)
        f *= 0.9
        top, bottom = -h * (0.84 - 0.06 * abs(f)), h * (0.76 - 0.16 * abs(f))   # the outer ones a little shorter
        left_side, right_side = [], []
        for i in range(33):
            u = i / 32
            y = top + (bottom - top) * u
            x = f * half_width(y)
            half = STRIPE_WIDTH / 2 * math.sin(math.pi * u) ** 0.7  # thin at both ends, fullest in the middle
            left_side.append((x - half, y))
            right_side.append((x + half, y))
        points = left_side + right_side[::-1]
        stripes.append("M" + "L".join(f"{num(x)} {num(y)}" for x, y in points) + "Z")
    return (f'<g transform="translate({num(tx)} {num(ty)}) rotate({angle})">'
            f'<path d="{body}" fill="{COPPER}"/>'
            f'<path d="{"".join(stripes)}" fill="{SLATE}"/></g>')


def cage():
    """The cage with the seed inside: four bars rising into the dome (the outer pair as a circle, the inner pair as a
    tall arch), meeting under a short neck that holds the ring; two bands; a base. All the porcelain lines are drawn
    first and all their slate middle lines after them, so the middle lines cross where the bands meet the bars and
    meet under the neck; the base comes last, over the bars' ends."""
    bars = []
    for side in (-1, 1):
        sweep = 1 if side < 0 else 0
        bars.append(f"M{num(side * OUTER)} {BASE_Y}V{num(DOME_CY)}A{num(OUTER)} {num(OUTER)} 0 0 {sweep} 0 {APEX}")
        bars.append(f"M{num(side * INNER)} {BASE_Y}V{num(ARCH_CY)}A{num(INNER)} {num(ARCH_CY - APEX)} 0 0 {sweep} 0 {APEX}")
    lines = " ".join(bars) + f" M0 {APEX}V{num(RING_CY + RING_R)}"
    bands_white = " ".join(f"M{num(-BAND_WHITE)} {y}H{num(BAND_WHITE)}" for y in (BAND_TOP, BAND_LOW))
    bands_dark = " ".join(f"M{num(-BAND_DARK)} {y}H{num(BAND_DARK)}" for y in (BAND_TOP, BAND_LOW))
    stroke = 'fill="none" stroke-linejoin="round"'
    seed = sunflower_seed(0, 0)
    return "\n".join([
        f'<g transform="translate(0 {num((BAND_TOP + BAND_LOW) / 2)}) scale({SEED_SCALE})">{seed}</g>',
        # the porcelain lines
        f'<path d="{lines}" {stroke} stroke="{PORCELAIN}" stroke-width="{LINE}"/>',
        f'<circle cx="0" cy="{RING_CY}" r="{RING_R}" {stroke} stroke="{PORCELAIN}" stroke-width="{LINE}"/>',
        f'<path d="{bands_white}" {stroke} stroke="{PORCELAIN}" stroke-width="{LINE}" stroke-linecap="round"/>',
        # their slate middle lines
        f'<path d="{lines}" {stroke} stroke="{SLATE}" stroke-width="{num(INLINE)}"/>',
        f'<circle cx="0" cy="{RING_CY}" r="{RING_R}" {stroke} stroke="{SLATE}" stroke-width="{num(INLINE)}"/>',
        f'<path d="{bands_dark}" {stroke} stroke="{SLATE}" stroke-width="{num(INLINE)}"/>',
        # the base, over the bars' ends, with its two slate lines
        f'<rect x="{BASE_X}" y="{BASE_Y}" width="{BASE_W}" height="{BASE_H}" rx="{num(BASE_H / 2)}" fill="{PORCELAIN}"/>',
        f'<path d="' + " ".join(f"M{num(-BASE_LINE_X)} {y}H{num(BASE_LINE_X)}" for y in BASE_LINES) + f'" {stroke} stroke="{SLATE}" stroke-width="{num(INLINE)}"/>',
    ])


# ---------------------------------------------------------------- the word, as outlines
def wordmark(font_path, size):
    """'SeedLock' at `size` px with the font's own spacing (kerning included), its left edge at 0 and baseline at 0.
    Returns (svg, width, top, bottom): top and bottom of the letters, relative to the baseline."""
    tt = TTFont(font_path)
    glyph_set, order, upem = tt.getGlyphSet(), tt.getGlyphOrder(), tt["head"].unitsPerEm
    font = hb.Font(hb.Face(hb.Blob.from_file_path(font_path)))
    buf = hb.Buffer()
    buf.add_str("SeedLock")
    buf.guess_segment_properties()
    hb.shape(font, buf, {"kern": True, "liga": True})
    scale = size / upem
    paths = {"Seed": [], "Lock": []}
    pen_x, box = 0, [None, None, None, None]
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        name = order[info.codepoint]
        dx, dy = (pen_x + pos.x_offset) * scale, -pos.y_offset * scale
        pen = SVGPathPen(glyph_set, ntos=num)
        glyph_set[name].draw(TransformPen(pen, (scale, 0, 0, -scale, dx, dy)))
        paths["Seed" if info.cluster < 4 else "Lock"].append(pen.getCommands())
        bounds = BoundsPen(glyph_set)
        glyph_set[name].draw(bounds)
        if bounds.bounds:
            x0, y0, x1, y1 = bounds.bounds
            edges = (dx + x0 * scale, dy - y1 * scale, dx + x1 * scale, dy - y0 * scale)
            box = [min(b, e) if b is not None else e for b, e in zip(box[:2], edges[:2])] + \
                  [max(b, e) if b is not None else e for b, e in zip(box[2:], edges[2:])]
        pen_x += pos.x_advance
    svg = (f'<path d="{"".join(paths["Seed"])}" fill="{COPPER}"/>\n'
           f'<path d="{"".join(paths["Lock"])}" fill="{PORCELAIN}"/>')
    return svg, box[2], box[1], box[3]


RIGHTS = ("SeedLock logo. Copyright (c) 2026 The SeedLock authors. All rights reserved: not covered by the MIT license "
          "of the SeedLock repository; see docs/logo/README.md there.")


def svg_doc(width, height, body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{num(width)}" height="{num(height)}" '
            f'viewBox="0 0 {num(width)} {num(height)}" role="img" aria-label="SeedLock">\n'
            f'<!-- {RIGHTS} -->\n<title>SeedLock</title>\n{body}\n</svg>\n')


def add_png_copyright(path):
    """Write the rights notice into a PNG file as its standard "Copyright" text field (a tEXt chunk after IHDR)."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR", "not a PNG file"
    ihdr_end = 8 + 12 + int.from_bytes(data[8:12], "big")
    text = b"Copyright\0" + RIGHTS.encode("latin-1")
    chunk = len(text).to_bytes(4, "big") + b"tEXt" + text + zlib.crc32(b"tEXt" + text).to_bytes(4, "big")
    open(path, "wb").write(data[:ihdr_end] + chunk + data[ihdr_end:])


def lockup(font_path, cage_height):
    """The cage and the word side by side, the cage `cage_height` px tall: (svg, width, height), origin at its top left."""
    x0, y0, x1, y1 = CAGE_BOX
    s = cage_height / (y1 - y0)
    cage_w = (x1 - x0) * s
    word, word_w, top, bottom = wordmark(font_path, cage_height * 0.52)
    # the word is centred on the cage itself (its dome and body), not on the ring it hangs from
    body_mid = ((APEX - LINE / 2 + y1) / 2 - y0) * s
    gap = cage_height * 0.12
    svg = (f'<g transform="scale({num(s)}) translate({num(-x0)} {num(-y0)})">\n{cage()}\n</g>\n'
           f'<g transform="translate({num(cage_w + gap)} {num(body_mid - (top + bottom) / 2)})">\n{word}\n</g>')
    return svg, cage_w + gap + word_w, cage_height


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out", nargs="?", default=os.path.dirname(os.path.abspath(__file__)))
    ap.add_argument("--font", help="IBMPlexSans-Bold.ttf (or .otf)")
    a = ap.parse_args()
    font_path = a.font or next((p for p in FONT_PLACES if os.path.isfile(p)), None)
    if not font_path or not os.path.isfile(font_path):
        sys.exit("IBM Plex Sans Bold was not found: install it (sudo apt install fonts-ibm-plex) or give it with --font")
    os.makedirs(a.out, exist_ok=True)

    # the icon: the cage alone on a full square, as large as it can be while staying inside the circle GitHub crops
    # avatars to: the base's bottom corners are its farthest points, kept 232 px from the middle (of 256)
    x0, y0, x1, y1 = CAGE_BOX
    mid_y = (y0 + y1) / 2
    corner = math.hypot(BASE_W / 2 - BASE_H / 2 * (1 - math.sqrt(0.5)), y1 - BASE_H / 2 * (1 - math.sqrt(0.5)) - mid_y)
    s = 232 / corner
    icon = (f'<rect width="512" height="512" fill="{SLATE}"/>\n'
            f'<g transform="translate({num(256 - (x0 + x1) / 2 * s)} {num(256 - mid_y * s)}) scale({num(s, 4)})">\n{cage()}\n</g>')
    files = {"seedlock-icon.svg": svg_doc(512, 512, icon)}

    # the logo for the README: the lockup on a rounded slate card, the same margin all round
    art, w, h = lockup(font_path, 300)
    pad = 54
    files["seedlock-logo.svg"] = svg_doc(w + 2 * pad, h + 2 * pad, f'<rect width="{num(w + 2 * pad)}" height="{num(h + 2 * pad)}" rx="44" fill="{SLATE}"/>\n'
                                                                    f'<g transform="translate({pad} {pad})">\n{art}\n</g>')

    # GitHub's social preview (Settings > General > Social preview): 1280 x 640, the lockup in the middle
    art, w, h = lockup(font_path, 300)
    files["seedlock-social-preview.svg"] = svg_doc(1280, 640, f'<rect width="1280" height="640" fill="{SLATE}"/>\n'
                                                              f'<g transform="translate({num((1280 - w) / 2)} {num((640 - h) / 2)})">\n{art}\n</g>')
    for name, text in files.items():
        with open(os.path.join(a.out, name), "w") as f:
            f.write(text)

    rsvg = shutil.which("rsvg-convert")
    if not rsvg:
        print("SVG files written; rsvg-convert was not found, so no PNG files (sudo apt install librsvg2-bin)")
        return
    for svg, png in (("seedlock-icon.svg", "seedlock-icon.png"), ("seedlock-social-preview.svg", "seedlock-social-preview.png")):
        subprocess.run([rsvg, "-o", os.path.join(a.out, png), os.path.join(a.out, svg)], check=True)
        add_png_copyright(os.path.join(a.out, png))
    print("written to", os.path.normpath(a.out) + ":", ", ".join(sorted(os.listdir(a.out))))


if __name__ == "__main__":
    main()
