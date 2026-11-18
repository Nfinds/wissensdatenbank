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
    return cs, pts.min(axis=0), pts.max(axis=0)


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
    cs, lo, hi = contours(src)
    w, h = hi - lo
    return cs, lo, w, h


def svg_badge(cs, lo, w, h):
    # fit the silhouette into the inner disc (radius 225 of 256) with some air
    target = 372.0
    scale = target / max(w, h)
    off = lo - np.array([(512 - w * scale) / 2 / scale, (512 - h * scale) / 2 / scale]) + np.array([0, 8 / scale])
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


def svg_mark(cs, lo, w, h):
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
