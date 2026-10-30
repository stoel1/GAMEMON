# Rust architecture

## Layers
- Application/UI: GTK4 and libadwaita. Displays state; it does not read hardware files directly.
- Core: normalized sensor readings, status, timestamps, configuration and polling coordination.
- Collectors: independent modules for CPU, memory, GPU, storage, network, hwmon and device inventory.
- Platform interfaces: Linux procfs, sysfs, hwmon, DRM and vendor APIs such as NVML.

## Collector contract
Collectors discover hardware at runtime, distinguish unavailable/unsupported/invalid/permission-denied data, return source metadata and timestamps, avoid blocking the UI thread, never write to hardware controls, and never infer undocumented sensor labels or calibration from channel numbers.

## Initial implementation
The initial collectors read CPU model and online CPU range from procfs/sysfs, aggregate and per-logical CPU counters from `/proc/stat`, package/core identifiers from each online CPU's sysfs topology, and the driver's reported current frequency from cpufreq sysfs when exported. `scaling_cur_freq` may report the last requested P-state rather than the exact instantaneous hardware frequency. RAM, swap, buffers/cache and HugePages come from `/proc/meminfo`; memory pressure comes from `/proc/pressure/memory` when available. CPU utilization is computed from deltas between successful counter samples; per-CPU baselines are discarded for CPUs that go offline. The first sample establishes a baseline. The GTK application polls these collectors on a one-second GLib timer and displays missing or malformed readings as unavailable. Static topology values are cached and retried only when unavailable. The timer holds weak widget references and ends when the window widgets are gone. Hybrid-core classification, temperature, power, throttling, sampling history and remaining collectors are follow-up work.

## Permissions
GAMEMON is designed for ordinary-user operation. Optional privileged telemetry must be isolated and documented; the main application must never require unrestricted root execution.
