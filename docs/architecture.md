# Architecture and data sources

GAMEMON is currently a single Bash process. It samples Linux kernel interfaces and renders a full-screen terminal dashboard.

## Data sources

| Displayed metric | Current source | Notes |
|---|---|---|
| Overall/per-CPU usage | `/proc/stat` | Calculated from deltas between samples |
| CPU model | `/proc/cpuinfo` | First reported model name |
| Average CPU frequency | `/proc/cpuinfo` | Arithmetic average of reported `cpu MHz` values; not an effective-frequency measurement |
| CPU package/core temperatures | `coretemp` hwmon | Uses labels exposed below `/sys/class/hwmon` |
| Board temperatures/fans | `nct6798` hwmon | Label selection is motherboard/driver dependent |
| Vcore candidate | `nct6798` `in0_input` | Board-reported value; calibration/scaling is not guaranteed |
| NVIDIA GPU telemetry | `nvidia-smi` | NVIDIA driver utility is optional |
| RAM/swap | `/proc/meminfo` | RAM used is calculated as MemTotal minus MemAvailable |

## Hardware assumptions in the current baseline

The current code contains fixed mappings for the i9-13900K temperature labels:

- P-core labels: 0, 4, 8, 12, 16, 20, 24 and 28
- E-core labels: 32 through 47
- Logical CPU display: CPU 0 through CPU 31

These assumptions are specific to the development machine and must be replaced with topology discovery before claiming broad CPU support.

## Privileges

The monitor is designed to run as an ordinary user. Read access to hwmon files is controlled by kernel drivers and system permissions. Avoid recommending unrestricted root execution as a default workaround.

## Known limitations

- Fixed CPU topology and temperature-label mapping
- No `turbostat` integration yet
- Average frequency is based on `/proc/cpuinfo`, not measured effective frequency
- Sensor scaling and board labels vary by hardware
- No configuration file or command-line options yet
- Terminal layout assumes enough rows for the complete dashboard
