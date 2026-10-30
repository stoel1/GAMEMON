# Rust architecture

## Layers
- Application/UI: GTK4 and libadwaita. Displays state; it does not read hardware files directly.
- Core: normalized sensor readings, status, timestamps, configuration and polling coordination.
- Collectors: independent modules for CPU, memory, GPU, storage, network, hwmon and device inventory.
- Platform interfaces: Linux procfs, sysfs, hwmon, DRM and vendor APIs such as NVML.

## Collector contract
Collectors discover hardware at runtime, distinguish unavailable/unsupported/invalid/permission-denied data, return source metadata and timestamps, avoid blocking the UI thread, never write to hardware controls, and never infer undocumented sensor labels or calibration from channel numbers.

## Initial implementation
The initial Rust shell reads CPU model/online CPU range from procfs/sysfs and RAM capacity/availability from /proc/meminfo. Sampling history, topology classification and background polling will be introduced separately.

## Permissions
GAMEMON is designed for ordinary-user operation. Optional privileged telemetry must be isolated and documented; the main application must never require unrestricted root execution.
