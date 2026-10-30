# GAMEMON

GAMEMON is a terminal-based hardware monitor written in Bash, initially developed for **Red Hat Enterprise Linux 10**.

The current development baseline targets:

- Intel Core i9-13900K (hybrid P-core/E-core CPU)
- ASRock Z790 LiveMixer motherboard with Nuvoton NCT6798D monitoring chip
- NVIDIA GeForce RTX 5070

It reads Linux kernel interfaces directly and uses `nvidia-smi` for NVIDIA telemetry.

## Current features

- Overall CPU utilization and per-logical-CPU utilization
- CPU package and selected P-core/E-core temperatures via `coretemp`
- Motherboard temperatures, fan RPM and a candidate Vcore reading via `nct6798`
- Average CPU frequency from `/proc/cpuinfo`
- NVIDIA GPU utilization, temperature, graphics/memory clocks, power and VRAM
- RAM and swap usage
- Full-screen terminal display with one-second refresh and Ctrl+C exit

## Project status

This repository is the initial source and documentation baseline. It is **not yet a stable release**. The monitor currently contains hardware-specific core mapping for the i9-13900K and NCT6798D. Dynamic topology discovery, broader sensor support, configurable refresh intervals, improved error handling and `turbostat` integration are planned.

The NCT6798D `in0_input` voltage is displayed as a board-reported Vcore candidate. Sensor scaling and calibration can vary by motherboard; treat it as an indicative reading, not a calibrated measurement.

## Requirements

- RHEL 10 or a compatible Linux distribution
- Bash 4 or newer
- `awk`, `cat`, `date`, `hostname`, `tput` and standard core utilities
- Linux hwmon interfaces exposed under `/sys/class/hwmon`
- Optional: NVIDIA driver utilities providing `nvidia-smi`

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
