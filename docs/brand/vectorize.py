"""Turn the black-on-white elephant (elephant-source.jpg) into the mascot SVG.

    python docs/brand/vectorize.py docs/brand/elephant-source.jpg docs/brand/elephant.svg docs/brand/elephant-mark.svg

elephant.svg       white elephant on ink inside the Software ring (NansiFinds brand: ring 6 % of the diameter)
elephant-mark.svg  the bare silhouette in ink, for light backgrounds
"""
import sys

import cv2
import numpy as np

INK, WHITE, RING = "#0B0B0C", "#FFFFFF", "#10936B"


def contours(path):
    img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
    img = cv2.resize(img, None, fx=2, fy=2, interpolation=cv2.INTER_CUBIC)
    img = cv2.GaussianBlur(img, (5, 5), 0)
    _, mask = cv2.threshold(img, 128, 255, cv2.THRESH_BINARY_INV)  # elephant = 255
    cs, _ = cv2.findContours(mask, cv2.RETR_CCOMP, cv2.CHAIN_APPROX_NONE)
    cs = [c for c in cs if cv2.contourArea(c) > 40]  # drop specks
    pts = np.vstack(cs).reshape(-1, 2)
    # optical centre: the bounding box of the solid mass, without thin tusks and tail
    solid = cv2.morphologyEx(mask, cv2.MORPH_OPEN, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (41, 41)))
    sy, sx = np.nonzero(solid)
    optical = np.array([(sx.min() + sx.max()) / 2, (sy.min() + sy.max()) / 2])
    return cs, pts.min(axis=0), pts.max(axis=0), optical, pts


def smooth_path(c, scale, off):
    """Closed contour -> quadratic Bezier path through the midpoints (smooth, no corners from pixels)."""
    p = cv2.approxPolyDP(c, 1.2, True).reshape(-1, 2).astype(float)
    if len(p) < 3:
        return ""
    p = (p - off) * scale
    mids = (p + np.roll(p, -1, axis=0)) / 2
    d = [f"M{mids[-1][0]:.1f} {mids[-1][1]:.1f}"]
    for ctrl, end in zip(p, mids):
        d.append(f"Q{ctrl[0]:.1f} {ctrl[1]:.1f} {end[0]:.1f} {end[1]:.1f}")
    return "".join(d) + "Z"


def build(src):
    return contours(src)


def svg_badge(cs, lo, hi, optical, pts):
    """Optical centring: thin tusks and tail stretch the bounding box and the heavy body pulls the
    area centroid, both look off-centre. Horizontally the bounding box of the solid mass (tusks and
    tail opened away) goes to the middle, vertically the full bounding box (back and feet are clean
    edges). The silhouette shrinks until every point keeps clear of the ring."""
    w, h = hi - lo
    centre = np.array([optical[0], (lo[1] + hi[1]) / 2])
    scale = 372.0 / max(w, h)
    while True:
        off = centre - 256 / scale
        r = np.hypot(*((pts - off) * scale - 256).T).max()
        if r <= 198 or scale < 0.1:  # inner disc radius 225: the tusk tips keep ~27 px of air
            break
        scale *= 0.99
    d = "".join(smooth_path(c, scale, off) for c in cs)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-labelledby="t">
  <title id="t">wb mascot: an elephant, because elephants never forget</title>
  <!-- NansiFinds brand: ink and white; the Software colour (Smaragd {RING}) only as the outer ring, 6 % of the diameter.
       Built by docs/brand/vectorize.py from docs/brand/elephant-source.jpg (Google Flow, Nano Banana 2.1). -->
  <circle cx="256" cy="256" r="256" fill="{INK}"/>
  <circle cx="256" cy="256" r="240.64" fill="none" stroke="{RING}" stroke-width="30.72"/>
  <path fill="{WHITE}" fill-rule="evenodd" d="{d}"/>
</svg>
"""


def svg_mark(cs, lo, hi, optical, pts):
    w, h = hi - lo
    pad = 16
    scale = 480.0 / max(w, h)
    vw, vh = w * scale + 2 * pad, h * scale + 2 * pad
    off = lo - pad / scale
    d = "".join(smooth_path(c, scale, off) for c in cs)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {vw:.0f} {vh:.0f}" role="img" aria-labelledby="t">
  <title id="t">wb elephant</title>
  <path fill="{INK}" fill-rule="evenodd" d="{d}"/>
</svg>
"""


if __name__ == "__main__":
    src, badge, mark = sys.argv[1:4]
    parts = build(src)
    open(badge, "w", encoding="utf-8", newline="\n").write(svg_badge(*parts))
    open(mark, "w", encoding="utf-8", newline="\n").write(svg_mark(*parts))
    print(f"{len(parts[0])} contours")
