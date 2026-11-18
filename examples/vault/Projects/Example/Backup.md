---
type: topic
status: active
checked_on: 2026-01-01
---
# Backup

Back to [[Projects/Example/00 Overview]].

## Status

- Nightly copy of the vault to an external drive.

## Restore

- Copy the folder back with timestamps preserved (`robocopy /COPY:DAT /DCOPY:DAT` or `cp -a`),
  then run `wb sync --full`.

## Open

- Test a restore once a month.
