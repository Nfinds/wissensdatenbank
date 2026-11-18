# Knowledge base `wb`: rules for agents

Paste this block into `CLAUDE.md`, `AGENTS.md` or your agent's instruction file and adjust the vault path.

## Knowledge base `wb` (vault `<path to vault>`)

Notes are Markdown files in the vault: `Projects/<Project>/00 Overview.md` (map, under 100 lines),
one topic note per subsystem, decision or investigation next to it; `Sources/Web/` (web sources);
`Journal/YYYY-MM.md`. Read with `wb`, not with glob, grep or whole-file reads over the vault;
every output names its price (`~Nt`) and the address for the next step.

### Look up first (every task except small talk)

1. `wb context --cwd <working folder>` or `wb context <Project>`; then at most 3 matching topic
   notes with `wb read "<address>"`.
2. Targeted: `wb search <2–4 terms> [-p <Project>]`, then at most 3 hits with `wb read "<address>"`.
3. Nothing found (exit code 1): start without further searching.

At most 5 read commands per task. Notes marked `superseded` are history, not the current state.
Notes are hints, not proof: check current files before acting.

### Document afterwards (only when something lasting happened)

- Update the smallest matching topic note, or create one: `wb new "<Title>" -p <Project> --summary "<one line>"`.
- Keep frontmatter valid (`type`: overview|topic|source; `status`: active|paused|done|superseded)
  and set `checked_on` to today.
- Write: state, decisions with reason, what worked and what were dead ends, exact paths, dated
  test results, open points. No secrets.
- Journal line: `wb log -n "<note address>" "<one sentence: what, where, result>"`.
- After creating or renaming: `wb check`.

Exit codes: 0 ok, 1 nothing found, 2 wrong call or ambiguous, 3 vault/index not reachable, 4 read-only.
