"""Build docs/demo-vault: invented projects for README screenshots. Deterministic."""
import datetime as dt
import os
import pathlib
import random
import sys

OUT = pathlib.Path(sys.argv[1])
TODAY = dt.date(2026, 11, 18)
rnd = random.Random(7)


def fm(**kv):
    lines = ["---"]
    for k, v in kv.items():
        lines.append(f"{k}: {v}")
    lines.append("---")
    return "\n".join(lines) + "\n"


def write(rel, text):
    p = OUT / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8", newline="\n")
    # file time = checked_on at noon, so ages in `wb ui` look like a real vault (git does not keep it)
    for line in text.splitlines()[1:8]:
        if line.startswith("checked_on: "):
            day = dt.datetime.fromisoformat(line[12:] + "T12:00:00")
            os.utime(p, (day.timestamp(), day.timestamp()))


def d(days_ago):
    return (TODAY - dt.timedelta(days=days_ago)).isoformat()


# project -> (paths, status, state lines, open lines, topics)
# topic: (title, summary, days_ago, status, body sections)
P = {
    "Weather App": ("C:/Code/weather-app", "active",
        ["Beta 0.9 on TestFlight, 41 testers.", "Forecast API moved to the paid tier (see API Rate Limits)."],
        ["Crash on cold start without network (iOS 18).", "Widget refresh every 15 min drains battery."],
        [
            ("API Rate Limits", "Why we cache forecasts for 10 minutes", 2, "active", {
                "Status": "- Free tier allows 1,000 calls per day; the beta used 3,400 on the first weekend.\n- Forecasts are cached for 10 minutes per grid cell.",
                "Decisions": "- Paid tier (60,000 calls) instead of a second free key: keys are per account, rotating them breaks the terms.",
                "Dead ends": "- Caching per city name: the same coordinates arrived with 14 different spellings.",
                "Open": "- Alert when 80 % of the daily quota is used.",
            }),
            ("Offline Cache", "SQLite cache, last 48 hours of forecasts", 5, "active", {
                "Status": "- Last 48 hours are stored in SQLite and shown greyed out when offline.",
                "Tests": "- 2026-11-12 airplane mode on iPhone 15: app opens in 0.8 s with cached data.",
                "Open": "- Cold start without network still crashes, see [[Projects/Weather App/00 Overview]].",
                "Sources": "- [[Sources/Web/SQLite FTS5]]",
            }),
            ("Release Checklist", "Steps before every TestFlight build", 9, "active", {
                "Steps": "1. Bump build number.\n2. Run UI tests on the smallest screen.\n3. Check the privacy manifest.\n4. Upload and write the tester note.",
                "Lessons": "- Forgetting the privacy manifest cost a two-day review delay in October.",
            }),
            ("Push Notifications", "First approach with a polling server", 40, "superseded", {
                "Status": "- Replaced by background fetch; the polling server cost more than the API.",
            }),
        ]),
    "Home Server": ("C:/Code/homelab", "active",
        ["Proxmox host with 6 containers, 2 TB mirror.", "Nightly backup to external drive works since November."],
        ["Second disk reports 12 reallocated sectors.", "TLS renewal for the media container failed once."],
        [
            ("Backup", "How the nightly backup works and how to restore", 1, "active", {
                "Status": "- Restic at 02:30 to the external drive, 30 daily and 12 monthly snapshots.",
                "Restore": "- `restic restore latest --target /restore`, then compare checksums of the photo library.",
                "Tests": "- 2026-11-16 full restore of the photo library (212 GB) in 3 h 10 min, 0 checksum errors.",
                "Dead ends": "- rsync with hard links: fast, but no encryption and no pruning.",
                "Sources": "- [[Sources/Web/Restic Documentation]]",
            }),
            ("Reverse Proxy", "Caddy in front of all web services", 12, "active", {
                "Status": "- Caddy with automatic TLS, one block per service.",
                "Lessons": "- Wildcard certificates need the DNS challenge; HTTP challenge fails behind the router.",
            }),
            ("Disk Health", "SMART values and when to replace a disk", 3, "active", {
                "Status": "- Disk 2: 12 reallocated sectors, up from 4 in October.",
                "Decisions": "- Replace at 50 sectors or any pending sector, whichever comes first.",
                "Open": "- Order a spare disk of the same size.",
            }),
            ("Docker Updates", "Update containers without surprises", 20, "paused", {
                "Status": "- Watchtower disabled; updates by hand on Sundays.",
            }),
        ]),
    "Invoice Tool": ("C:/Code/invoice-tool", "active",
        ["PDF export done, tax rules for DE and AT.", "Used for real invoices since September."],
        ["Reverse charge for EU customers.", "Numbering must not reuse cancelled numbers."],
        [
            ("PDF Export", "Typst template, fonts embedded", 7, "done", {
                "Status": "- Typst template, fonts embedded, archive copy as PDF/A-3.",
                "Dead ends": "- HTML to PDF via headless browser: page breaks inside tables.",
            }),
            ("Tax Rules", "VAT rates and small business rule", 4, "active", {
                "Status": "- Germany 19 % and 7 %, Austria 20 % and 10 %.",
                "Open": "- Reverse charge text for EU business customers.",
            }),
            ("Database Schema", "Invoices, items, customers, numbering", 15, "active", {
                "Decisions": "- Invoice numbers come from a sequence table, never from MAX()+1.",
            }),
        ]),
    "Game Jam 2026": ("C:/Code/jam-2026", "done",
        ["Submitted on time, rank 38 of 412.", "Post-mortem written."],
        [],
        [
            ("Level Design", "Three levels, one mechanic each", 25, "done", {
                "Lessons": "- Playtest after every level, not at the end: level 3 was too hard for everyone.",
            }),
            ("Audio Pipeline", "Sounds from a free library, normalised", 27, "done", {
                "Status": "- All effects normalised to -16 LUFS in one batch.",
            }),
            ("Controls", "Keyboard and gamepad", 26, "done", {
                "Dead ends": "- Remapping menu: two days for a feature nobody used during the jam.",
            }),
        ]),
    "Blog": ("C:/Code/blog", "active",
        ["Static site, 23 posts, builds in 4 s."],
        ["Images are too large on mobile."],
        [
            ("Static Site Build", "Build and deploy in one command", 30, "active", {
                "Status": "- `make deploy` builds and uploads; previews per branch.",
            }),
            ("Image Optimisation", "AVIF with JPEG fallback", 6, "active", {
                "Open": "- Generate 3 sizes per image; today every phone loads the 2400 px version.",
                "Sources": "- [[Sources/Web/Responsive Images Guide]]",
            }),
        ]),
}

SOURCES = [
    ("Responsive Images Guide", "https://developer.mozilla.org/en-US/docs/Web/HTML/Guides/Responsive_images", "srcset and sizes, art direction with picture."),
    ("Restic Documentation", "https://restic.readthedocs.io/", "Snapshots, forget and prune, restore."),
    ("SQLite FTS5", "https://www.sqlite.org/fts5.html", "Full text search extension used by the cache search."),
]

journal = []
for name, (path, status, state, open_, topics) in P.items():
    links = "\n".join(f"- [[Projects/{name}/{t[0]}|{t[1]}]]" for t in topics)
    body = fm(type="overview", status=status, checked_on=d(min(t[2] for t in topics)), paths=f'["{path}"]')
    body += f"# {name}\n\n## Status\n\n" + "\n".join(f"- {s}" for s in state) + "\n\n## Open\n\n"
    body += ("\n".join(f"- {s}" for s in open_) if open_ else "- Nothing open.") + "\n\n## Topics\n\n" + links + "\n"
    write(f"Projects/{name}/00 Overview.md", body)
    for title, summary, ago, st, sections in topics:
        t = fm(type="topic", status=st, checked_on=d(ago)) + f"# {title}\n\nBack to [[Projects/{name}/00 Overview]].\n\n"
        t += "\n\n".join(f"## {h}\n\n{b}" for h, b in sections.items()) + "\n"
        write(f"Projects/{name}/{title}.md", t)
        # journal: the last change plus some earlier work
        journal.append((ago, f"Projects/{name}/{title}", f"{summary}."))
        for _ in range(rnd.randint(1, 4)):
            journal.append((ago + rnd.randint(3, 70), f"Projects/{name}/{title}", rnd.choice(["Notes cleaned up.", "Test results added.", "Open points updated.", "Decision recorded with reason.", "Dead end documented."])))

for title, url, text in SOURCES:
    write(f"Sources/Web/{title}.md", fm(type="source", checked_on=d(30), source_url=url) + f"# {title}\n\n## Summary\n\n{text}\n")

by_month = {}
for ago, note, text in sorted(journal, key=lambda j: -j[0]):
    day = TODAY - dt.timedelta(days=min(ago, 80))
    by_month.setdefault(day.strftime("%Y-%m"), []).append(f"- {day.isoformat()} [[{note}]] {text}")
for month, lines in by_month.items():
    write(f"Journal/{month}.md", "\n".join(lines) + "\n")
print(f"{sum(1 for _ in OUT.rglob('*.md'))} notes")
