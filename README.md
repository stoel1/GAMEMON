# GAMEMON

GAMEMON is a terminal-based hardware monitor written in Bash, initially developed for **Red Hat Enterprise Linux 10**.

GAMEMON is being generalized to run across different Linux PCs. CPU topology and online logical processors are discovered at runtime, memory and swap are read from `/proc/meminfo`, and GPU detection supports NVIDIA through `nvidia-smi` plus best-effort AMD/Intel DRM sysfs telemetry.

The original development and validation machine is an Intel Core i9-13900K, ASRock Z790 LiveMixer and NVIDIA GeForce RTX 5070. Other hardware combinations still need testing.

## Current features

- Overall CPU utilization and per-logical-CPU utilization
- Dynamic online logical-CPU discovery, physical-core counting and per-logical-CPU utilization
- Intel hybrid P-core/E-core classification through Linux `topology/core_type` when the kernel exposes it
- CPU package/core temperatures from supported hwmon labels (including Intel `coretemp` and AMD `k10temp` where exposed)
- CPU frequency from cpufreq sysfs with `/proc/cpuinfo` fallback
- C1/C2/C3 idle residency from cpuidle sysfs where exposed
- Motherboard temperatures, fan RPM and board-reported voltage from supported hwmon devices (currently Nuvoton NCT6798D-specific mappings)
- NVIDIA GPU telemetry through `nvidia-smi`, including multiple NVIDIA adapters
- Best-effort AMD/Intel GPU utilization, temperature, power and VRAM telemetry through DRM sysfs where the driver exposes those files
- RAM and swap usage from `/proc/meminfo`
- Full-screen terminal display with one-second refresh and Ctrl+C exit

## Project status

This repository is under active development and is **not yet a stable release**. CPU topology and GPU discovery are now dynamic, but sensor coverage varies by kernel, driver and hardware. Motherboard fan/voltage mappings remain board-specific. Broader hardware testing, improved error handling, configurable refresh intervals and a test harness are still planned.

The NCT6798D `in0_input` voltage is displayed as a board-reported Vcore candidate. Sensor scaling and calibration can vary by motherboard; treat it as an indicative reading, not a calibrated measurement.

## Requirements

- RHEL 10 or a compatible Linux distribution
- Bash 4 or newer
- `awk`, `cat`, `date`, `hostname`, `tput` and standard core utilities
- Linux hwmon interfaces exposed under `/sys/class/hwmon`
- Optional: NVIDIA driver utilities providing `nvidia-smi` for full NVIDIA telemetry
- Optional: DRM sysfs telemetry for AMD/Intel GPUs (availability depends on the kernel driver)

On RHEL, install the basic utilities if needed:

```bash
sudo dnf install bash coreutils gawk ncurses
```

For NVIDIA telemetry, install the NVIDIA driver utilities appropriate for your driver installation.

## Run from source

```bash
git clone https://github.com/stoel1/GAMEMON.git
cd GAMEMON
chmod +x src/gamemon
./src/gamemon
```

GAMEMON is designed to run as an ordinary user. Read access to hwmon files is controlled by the kernel drivers and system permissions. Avoid running the entire monitor as root by default.

Press **Ctrl+C** to exit.

## RPM packaging

An initial RPM spec is provided in `packaging/rpm/gamemon.spec`. Packaging is under development and has not yet been validated as a complete install/upgrade/uninstall workflow on RHEL 10.

See [RPM packaging](docs/rpm-packaging.md).

## Documentation

- [Architecture and data sources](docs/architecture.md)
- [RPM packaging](docs/rpm-packaging.md)
- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)

## Contributions and issue reports

Issues and pull requests are welcome. When reporting a hardware detection problem, include your RHEL version, kernel version, CPU, motherboard/sensor chip and relevant command output. Remove hostnames, serial numbers and other private information before posting logs.

## License

No license has been assigned yet. Until a license is added, the source is publicly viewable but standard copyright restrictions apply.
