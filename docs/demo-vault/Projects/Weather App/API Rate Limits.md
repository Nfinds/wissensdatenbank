---
type: topic
status: active
checked_on: 2026-11-16
---
# API Rate Limits

Back to [[Projects/Weather App/00 Overview]].

## Status

- Free tier allows 1,000 calls per day; the beta used 3,400 on the first weekend.
- Forecasts are cached for 10 minutes per grid cell.

## Decisions

- Paid tier (60,000 calls) instead of a second free key: keys are per account, rotating them breaks the terms.

## Dead ends

- Caching per city name: the same coordinates arrived with 14 different spellings.

## Open

- Alert when 80 % of the daily quota is used.
