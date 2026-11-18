---
type: topic
status: active
checked_on: 2026-11-13
---
# Offline Cache

Back to [[Projects/Weather App/00 Overview]].

## Status

- Last 48 hours are stored in SQLite and shown greyed out when offline.

## Tests

- 2026-11-12 airplane mode on iPhone 15: app opens in 0.8 s with cached data.

## Open

- Cold start without network still crashes, see [[Projects/Weather App/00 Overview]].

## Sources

- [[Sources/Web/SQLite FTS5]]
