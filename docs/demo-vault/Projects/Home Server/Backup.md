---
type: topic
status: active
checked_on: 2026-11-17
---
# Backup

Back to [[Projects/Home Server/00 Overview]].

## Status

- Restic at 02:30 to the external drive, 30 daily and 12 monthly snapshots.

## Restore

- `restic restore latest --target /restore`, then compare checksums of the photo library.

## Tests

- 2026-11-16 full restore of the photo library (212 GB) in 3 h 10 min, 0 checksum errors.

## Dead ends

- rsync with hard links: fast, but no encryption and no pruning.

## Sources

- [[Sources/Web/Restic Documentation]]
