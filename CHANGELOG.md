# Changelog

## 0.2.0 — Rust migration in progress
- Archived the original Bash monitor under legacy/bash/gamemon.
- Established the Rust application structure with GTK4/libadwaita.
- Added initial CPU identity/online-CPU and RAM discovery.
- Added a typed sensor reading model with explicit availability status.
- Added two-second CPU utilization and RAM refresh, with unavailable readings kept explicit.
- Added unit coverage for CPU counter and memory parsers.
- Expanded memory telemetry to free memory, buffers/cache, swap, PSI and HugePages.
- Kept missing, malformed and overflowing memory readings unavailable and added parser tests.

The Rust migration is not a stable release. The archived Bash implementation remains available as a comparison baseline.
