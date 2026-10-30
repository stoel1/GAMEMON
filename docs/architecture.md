# Architecture and data sources

GAMEMON is currently a single Bash process. It samples Linux kernel interfaces and renders a full-screen terminal dashboard.

## Data sources

| Displayed metric | Current source | Notes |
|---|---|---|
| Overall/per-CPU usage | `/proc/stat` | Calculated from deltas between samples |
| CPU model | `/proc/cpuinfo` | First reported model name |
| Average CPU frequency | `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq`, fallback `/proc/cpuinfo` | Average of readable per-CPU cpufreq values; not necessarily effective frequency |
| CPU package/core temperatures | `coretemp`, `k10temp`, `zenpower`, `cpu_thermal`, `soc_dts` hwmon | Uses package/die/core labels exposed by the active driver; coverage varies |
| Board temperatures/fans | `nct6798` hwmon | Label selection is motherboard/driver dependent |
| Vcore candidate | `nct6798` `in0_input` | Board-reported value; calibration/scaling is not guaranteed |
| GPU telemetry | `nvidia-smi`; `/sys/class/drm/card*/device` | Full NVIDIA query; best-effort AMD/Intel DRM sysfs fields where exposed |
| RAM/swap | `/proc/meminfo` | Dynamically reads total/available RAM and swap; RAM used is MemTotal minus MemAvailable |
| C-state residency | `/sys/devices/system/cpu/cpu*/cpuidle/state*/time` | Per-logical-CPU deltas averaged across CPUs; only exposed states are reported |

## Dynamic hardware discovery

Online logical CPUs are discovered from `/sys/devices/system/cpu/online`, with `/proc/stat` as a fallback. CPU usage rows and core-temperature labels are generated from discovered data rather than fixed CPU IDs. The displayed core-temperature labels are generic; P-core/E-core classification is intentionally not inferred from fixed numbering.

Memory and swap capacity are read dynamically from `/proc/meminfo`. NVIDIA adapters are enumerated through `nvidia-smi`; when that is unavailable, DRM devices are inspected for AMD and Intel vendor IDs and supported sysfs telemetry files.

DRM and hwmon interfaces vary by kernel and driver. Missing metrics remain unavailable rather than being guessed. Motherboard fan and voltage mappings are still specific to the Nuvoton NCT6798D driver and need a generic sensor-discovery pass.

## Privileges

The monitor is designed to run as an ordinary user. Read access to hwmon files is controlled by kernel drivers and system permissions. Avoid recommending unrestricted root execution as a default workaround.

## Known limitations

- CPU temperature and GPU telemetry availability varies by driver
- No generic motherboard fan/voltage mapping yet
- Average frequency is based on cpufreq sysfs when available, not guaranteed effective frequency
- Sensor scaling and board labels vary by hardware
- No configuration file or command-line options yet
- Terminal layout assumes enough rows for the complete dashboard
