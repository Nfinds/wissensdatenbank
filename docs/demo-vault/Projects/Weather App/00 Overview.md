---
type: overview
status: active
checked_on: 2026-11-16
paths: ["C:/Code/weather-app"]
---
# Weather App

## Status

- Beta 0.9 on TestFlight, 41 testers.
- Forecast API moved to the paid tier (see API Rate Limits).

## Open

- Crash on cold start without network (iOS 18).
- Widget refresh every 15 min drains battery.

## Topics

- [[Projects/Weather App/API Rate Limits|Why we cache forecasts for 10 minutes]]
- [[Projects/Weather App/Offline Cache|SQLite cache, last 48 hours of forecasts]]
- [[Projects/Weather App/Release Checklist|Steps before every TestFlight build]]
- [[Projects/Weather App/Push Notifications|First approach with a polling server]]
