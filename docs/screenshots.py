"""Frame terminal snapshots (HTML from `readme_shots` or `cli_shots`) as a window and crop the PNGs.

    python docs/screenshots.py frame <in.html> <out.html> <window title>
    python docs/screenshots.py crop <png>
    python docs/screenshots.py cli <out.html> <wb binary> <vault> <index dir> <file with one command per line>

The PNG itself is rendered with headless Chrome by docs/screenshots.ps1.
"""
import re
import sys

INK = (11, 11, 12)

FRAME_CSS = """
<style>
  html, body { margin: 0; background: #0B0B0C; }
  body { padding: 28px; display: inline-block; }
  .cap { display: none; }
  .win { display: inline-block; border-radius: 12px; overflow: hidden;
         border: 1px solid #2b3038; background: #11141a; }
  .bar { height: 36px; display: flex; align-items: center; gap: 8px; padding: 0 14px;
         background: #181c23; border-bottom: 1px solid #2b3038;
         font: 13px "Segoe UI", sans-serif; color: #9aa3b2; }
  .dot { width: 12px; height: 12px; border-radius: 50%; background: #3C3C3D; }
  .ttl { margin-left: 10px; }
  .ring { margin-left: auto; width: 14px; height: 14px; border-radius: 50%;
          box-sizing: border-box; border: 2px solid #10936B; }
  pre { border-radius: 0 !important; padding: 14px 16px !important; }
</style>
"""


def frame(src, dst, title):
    html = open(src, encoding="utf-8").read()
    html = html.replace("</head>", FRAME_CSS + "</head>", 1)
    bar = (f'<div class="win"><div class="bar"><span class="dot"></span><span class="dot"></span>'
           f'<span class="dot"></span><span class="ttl">{title}</span><span class="ring"></span></div>')
    html = re.sub(r"<pre>", bar + "<pre>", html, count=1)
    html = html.replace("</pre>", "</pre></div>", 1)
    open(dst, "w", encoding="utf-8", newline="\n").write(html)


def cli(dst, wb, vault, index, cmdfile):
    """Run wb commands against the demo vault and write a terminal page (agent output, no colours of its own)."""
    import html as h
    import os
    import shlex
    import subprocess
    commands = [c for c in open(cmdfile, encoding="utf-8").read().splitlines() if c.strip()]
    env = dict(os.environ, WB_VAULT=vault, WB_INDEX=index, PYTHONIOENCODING="utf-8")
    lines = []
    for cmd in commands:
        args = shlex.split(cmd)
        out = subprocess.run([wb, *args[1:]], env=env, capture_output=True, text=True, encoding="utf-8").stdout
        lines.append(f'<span class="p">$</span> <span class="c">{h.escape(cmd)}</span>')
        for line in out.rstrip("\n").split("\n"):
            lines.append(paint(h.escape(line)))
        lines.append("")
    page = ('<!doctype html><html lang="en"><head><meta charset="utf-8"><title>wb</title><style>'
            'pre{display:inline-block;margin:0;background:#11141a;color:#c9d1dc;'
            'font:14px/1.45 "Cascadia Mono","Cascadia Code",Consolas,monospace;white-space:pre}'
            '.p{color:#6b7380}.c{color:#ffffff;font-weight:700}.a{color:#ffffff;font-weight:600}'
            '.m{color:#7d8694}.k{color:#e6b450}.t{color:#7d8694}'
            '</style></head><body><pre>' + "\n".join(lines).rstrip() + "</pre></body></html>")
    open(dst, "w", encoding="utf-8", newline="\n").write(page)


def paint(line):
    """Light highlighting: addresses bold, token prices and metadata dim, `more:`/`note:` keys amber."""
    line = re.sub(r"(~\d+t)", r'<span class="t">\1</span>', line)
    line = re.sub(r"^(more:|note:|next:|linked:|hint:)", r'<span class="k">\1</span>', line)
    line = re.sub(r"^(\d+ )((?:Projects|Sources)/[^#]+(?:#[^ ]+(?: [^ L][^ ]*)*)?)", r'\1<span class="a">\2</span>', line)
    line = re.sub(r"^((?:overview|state|open|paths|topics|journal|q|project)[=: ])", r'<span class="m">\1</span>', line)
    return line


def crop(png):
    from PIL import Image, ImageChops
    im = Image.open(png).convert("RGB")
    bg = Image.new("RGB", im.size, INK)
    box = ImageChops.difference(im, bg).getbbox()
    if box:
        pad = 28
        l, t, r, b = box
        im = im.crop((max(l - pad, 0), max(t - pad, 0), min(r + pad, im.width), min(b + pad, im.height)))
    im.save(png, optimize=True)
    print(f"{png} {im.width}x{im.height}")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "frame":
        frame(sys.argv[2], sys.argv[3], sys.argv[4])
    elif cmd == "crop":
        crop(sys.argv[2])
    elif cmd == "cli":
        cli(*sys.argv[2:])
