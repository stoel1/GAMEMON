# Changelog

## 0.2.0 — Rust migration in progress
- Archived the original Bash monitor under legacy/bash/gamemon.
- Established the Rust application structure with GTK4/libadwaita.
- Added initial CPU identity/online-CPU and RAM discovery.
- Added a typed sensor reading model with explicit availability status.
- Added periodic CPU utilization and RAM refresh, with unavailable readings kept explicit.
- Added unit coverage for CPU counter and memory parsers.
- Expanded memory telemetry to free memory, buffers/cache, swap, PSI and HugePages.
- Kept missing, malformed and overflowing memory readings unavailable and added parser tests.
- Added per-logical-CPU utilization and kernel-exported package/core topology with hotplug-aware counter baselines.
- Set the dashboard's default refresh interval to one second.
- Added per-logical-CPU current frequency when exported through cpufreq sysfs.
- Added read-only network interface status, link attributes, byte rates and RX/TX error/drop counters.
- Moved hardware polling to a background sampler with bounded snapshot delivery to the UI.

The Rust migration is not a stable release. The archived Bash implementation remains available as a comparison baseline.
