# wb: a Markdown knowledge base for AI agents

`wb` is a local knowledge system for coding agents (Claude Code, Codex, …) and the people working
with them. Notes stay plain Markdown files in a folder (the *vault*); `wb` keeps a derived SQLite
index (FTS5 full text, trigrams, link graph, journal) and answers with **budgeted, line-oriented
output** so an agent can find the one section it needs instead of reading whole files.

> **Language note:** the CLI messages, help texts and most docs are currently in German.
> Commands, flags and the JSON output are language-neutral. Contributions for an English UI are welcome.

## Why

Agents that keep notes in Markdown usually look things up with glob + grep + "read the whole
file". That burns tokens and misses notes whose words differ slightly (`Größe` vs `groesse`,
compounds, typos). `wb` adds:

- **Section-level hits** with a stable address `Note#Section` and a token price on every line.
- **German and English stemming, umlaut folding, trigram matching and typo tolerance.**
- **A hard output budget** (`--budget`); whatever is cut off is named by a runnable `more: wb …` line.
- **Safe writes:** `wb new` never overwrites, `wb log` appends under a file lock, `wb mv` rewrites
  all incoming links and finishes an interrupted run on the next start.
- **Checks** (`wb check`): dead links, broken frontmatter, orphaned notes, oversized notes,
  suspected secrets (reports path, line and class, never the value).
- **A terminal UI for humans** (`wb ui`, `wb stats`).

In our own measurement on a real vault of ~1,000 notes, `wb search` + `wb read` found the answer
in the top 3 for 27–28 of 28 questions, against 23–26 for ranked `ripgrep`, and needed about
35–45 % fewer tokens until the answer.

## Install

Requires Rust 1.91 or newer.

```bash
cargo build --release -p wb-cli
```

Copy `target/release/wb` (`wb.exe` on Windows) to a folder on your `PATH`.

## Set up a vault

```bash
# start from the empty template in this repo, or any folder of Markdown notes
cp -a examples/vault ~/notes
cd ~/notes
wb sync --init          # creates the index in <vault>/.wb/
```

`wb` finds the vault in this order: `--vault <folder>`, the environment variable `WB_VAULT`,
`vault = "…"` in `%APPDATA%\wb\config.toml` (Windows) or `~/.config/wb/config.toml` (elsewhere),
then the first folder with a `.wb/` index upwards from the current directory.
See [`examples/config.toml`](examples/config.toml).

## Vault layout

```
Projects/<Project>/00 Overview.md   map of the project (status, open points, topic list), < 100 lines
Projects/<Project>/<Topic>.md       one note per subsystem, decision or investigation
Sources/Web/<Source>.md             web sources (`wb new -t source`)
Journal/YYYY-MM.md                  one line per change (`wb log`)
```

Every note has YAML frontmatter, e.g.

```yaml
---
type: topic            # overview | topic | source
status: active         # active | paused | done | superseded
checked_on: 2026-01-31
---
```

## Quick start

```bash
wb context                          # list projects
wb context Example                  # status, open points, topics and recent journal of one project
wb search backup restore -p Example # section hits with token prices
wb read "Projects/Example/Backup#Restore"
wb new "Backup" -p Example --summary "How the nightly backup works"
wb log -n "Projects/Example/Backup" "Documented the restore test"
wb check
```

Exit codes: `0` ok, `1` nothing found, `2` wrong call or ambiguous, `3` vault or index not
reachable, `4` refused because of `--read-only`.

## Use it from an agent

Put the rules for your agent into its instruction file (`CLAUDE.md`, `AGENTS.md`, …).
[`Docs/AGENT-GUIDE.md`](Docs/AGENT-GUIDE.md) describes the output format (`more:`, `note:`,
`next:`, `linked:`), budgets and exit codes; [`examples/AGENT-RULES.md`](examples/AGENT-RULES.md)
is a short block you can paste.

## Repository layout

| Path | Content |
|---|---|
| `crates/wb-core` | model, parsers (frontmatter, links, sections, journal), index, sync, search, checks, write path |
| `crates/wb-cli` | the `wb` binary: `sync`, `search`, `read`, `context`, `check`, `new`, `log`, `mv`, `stats`, `ui` |
| `crates/wb-tui` | terminal views (`wb ui`, `wb stats`) |
| `Docs/` | specification (`CONTRACT.md`), agent guide, TUI design |
| `examples/` | empty vault template, config example, agent rules |

## Design decisions

- **Files are the truth, the index is derived.** Delete `<vault>/.wb/` at any time; `wb sync --full` rebuilds it.
- **SQLite FTS5 via bundled `rusqlite`, WAL, 30 s busy timeout:** one store for full text,
  trigrams, links and journal, with transactions and several processes at once, no own locking.
- **Stemming and folding in Rust before FTS5** (Snowball DE + EN, lowercase, no diacritics,
  ß→ss, ae/oe/ue→a/o/u), so no `unsafe` FFI tokenizer is needed.
- **Passage = section** (heading to next heading); snippets are cut at sentence boundaries.
- **Byte-identical output for the same state**; timings only with `--timing` on stderr.
- **Auto-sync before every read command** (~5 ms when nothing changed).

Rejected: tantivy (heavy, single writer, needs a second store for links and journal), an
in-memory index (would need own transactions), a custom FTS5 tokenizer (unsafe FFI), lines as
passages (too short, too many hits per note).

## Limits

- `wb` matches words, not meaning: a question that shares no word, stem or part of a word with
  the note will not find it.
- A change with identical size and a deliberately reset modification time goes unnoticed until `wb sync --full`.
- Copying a vault without preserving timestamps makes notes without `checked_on` look changed today.
- Code is recognised only in fenced blocks and backticks; indented code counts as text.
- New notes from `wb new` get German section headings (`Stand`, `Offen`). English headings
  (`Status`, `Open`, `Topics`, `Decisions` …) are recognised just as well; rename them freely.
- Developed and tested on Windows. On Linux and macOS the local time is currently taken as UTC
  (journal dates near midnight), and path comparison is case-insensitive like on Windows.
  CI runs the test suite on all three systems.

## License

MIT, see [LICENSE](LICENSE).
