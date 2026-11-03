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
- Added a user-level TOML setting for refresh interval with safe 100 ms–60 s bounds.
- Added dynamic DRM GPU inventory and optional driver-exported load and VRAM readings.
- Added optional NVML GPU metrics loaded dynamically without a build-time NVIDIA dependency.
- Added a rate-limited `nvidia-smi` fallback with bounded execution time and validated CSV parsing.
- Added dynamically discovered Linux powercap/RAPL CPU package power sampling from energy counter deltas.

The Rust migration is not a stable release. The archived Bash implementation remains available as a comparison baseline.


## Unreleased – Rust dashboard

- Reworked the overview into four metric-focused cards for CPU, GPU, memory and network telemetry.
- Promoted primary values (CPU/GPU utilization, RAM percentage and network throughput) and kept supporting readings in compact detail sections.
- Added utilization progress indicators and expanded the default window to give the dashboard more room.
- Kept telemetry collection in the background sampler and GTK updates on the main thread.
- Added rolling 60-sample sparkline histories for CPU, GPU and memory utilization, drawn directly by GTK without a charting dependency.
- Added Linux uptime and load-average readings from procfs.
- Added dynamic hwmon discovery for temperature and fan RPM readings, preserving labels exported by the kernel.
- Grouped duplicate exported RAPL package labels in the overview so repeated sysfs aliases do not flood the CPU card.
- This remains an evolving dashboard; dedicated device pages, navigation and richer time-series controls remain planned.
