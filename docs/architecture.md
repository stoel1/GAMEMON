# Rust architecture

## Layers
- Application/UI: GTK4 and libadwaita. Displays state; it does not read hardware files directly.
- Core: normalized sensor readings, status, timestamps, configuration and polling coordination.
- Collectors: independent modules for CPU, memory, GPU, storage, network, hwmon and device inventory.
- Platform interfaces: Linux procfs, sysfs, hwmon, DRM and vendor APIs such as NVML.

## Collector contract
Collectors discover hardware at runtime, distinguish unavailable/unsupported/invalid/permission-denied data, return source metadata and timestamps, avoid blocking the UI thread, never write to hardware controls, and never infer undocumented sensor labels or calibration from channel numbers.

## Initial implementation
The initial collectors read CPU model and online CPU range from procfs/sysfs, aggregate and per-logical CPU counters from `/proc/stat`, package/core identifiers from each online CPU's sysfs topology, and the driver's reported current frequency from cpufreq sysfs when exported. `scaling_cur_freq` may report the last requested P-state rather than the exact instantaneous hardware frequency. RAM, swap, buffers/cache and HugePages come from `/proc/meminfo`; memory pressure comes from `/proc/pressure/memory` when available. The network collector enumerates `/sys/class/net` every five seconds and samples interface counters and optional link attributes each second. CPU/network rates are computed from counter deltas; CPU per-core baselines are discarded when CPUs go offline, and network baselines are discarded when interfaces disappear. The first sample establishes a baseline. Missing or malformed readings stay unavailable. Static CPU topology values are cached and retried only when unavailable.

The sampler owns collectors on a background thread and sends snapshots through a one-item synchronous channel. If the UI falls behind, an obsolete snapshot is dropped instead of allowing an unbounded queue. GTK polls the channel and updates labels on the UI thread. The sampler exits after the receiver is dropped. Sampling history and remaining collectors are follow-up work.

The refresh interval is loaded from `$XDG_CONFIG_HOME/gamemon/config.toml`, falling back to `~/.config/gamemon/config.toml`. The only current setting is `refresh_interval_ms`; values are clamped to 100–60000 ms. Missing or invalid configuration uses the one-second default, and GAMEMON does not write configuration files.

The GPU collector discovers DRM `cardN` devices dynamically every five seconds and reads PCI IDs, driver/product strings, and optional GPU busy/VRAM attributes from the device's sysfs directory. It shows only readings actually exported by the driver. NVIDIA NVML and a controlled fallback, GPU temperatures and additional vendor metrics remain future work.

## Permissions
GAMEMON is designed for ordinary-user operation. Optional privileged telemetry must be isolated and documented; the main application must never require unrestricted root execution.
