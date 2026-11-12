# GAMEMON

GAMEMON is a Linux hardware-monitoring application migrating from its original Bash terminal monitor to a modular Rust application with a GTK4/libadwaita desktop interface.

## Status

The Rust application is under active development. It currently discovers CPU identity and online logical CPUs, samples aggregate and per-logical-CPU utilization, reads exported package/core topology, driver-reported CPU frequencies and package power through Linux powercap/RAPL, inventories DRM graphics devices, and reads NVIDIA telemetry through NVML or a rate-limited `nvidia-smi` fallback. It reports RAM/swap/HugePages/memory pressure and monitors network interface counters and throughput. A background sampler refreshes readings every second and sends bounded snapshots to the GTK interface. The overview currently presents metric cards with rolling utilization charts for CPU, GPU and memory with autoscaled percentage axes and time labels, plus Linux uptime, load average, dynamically discovered hwmon temperatures and fan speeds, filesystem capacity, block-device I/O rates, per-logical-CPU utilization, and a top-process list. Missing or malformed procfs/sysfs data is displayed as unavailable; counter-based rates appear after the first interval.

The original Bash implementation is preserved at legacy/bash/gamemon as a historical reference and comparison baseline. It is no longer the primary application target.

## Design principles

- Run as an ordinary user; do not require unrestricted root access.
- Discover hardware dynamically; do not assume a particular PC.
- Keep collectors independent of the graphical interface.
- Distinguish unavailable, unsupported, invalid and permission-denied readings.
- Never invent sensor values or assume undocumented motherboard voltage mappings.
- Keep monitoring read-only; GAMEMON is not a hardware-tuning utility.

## Build

Requirements: Rust/Cargo, GTK4 development files and libadwaita development files.

```bash
cargo build
cargo run
```

Run the collector unit tests with `cargo test`.

## Configuration

GAMEMON reads `gamemon/config.toml` from `$XDG_CONFIG_HOME`, or from `~/.config` when that variable is unset. The Settings page lets you change the hardware sampling interval (100 ms–60 s), applies changes immediately, and saves them to this file for subsequent launches. Invalid or out-of-range values fall back to the supported interval bounds.

On RHEL, install matching GTK4 and libadwaita development packages from enabled repositories before building.

### Linux ARM64 (Apple Silicon / aarch64)

The `rust-rewrite` branch includes a GitHub Actions workflow that builds and tests
a native Linux ARM64 binary on an ARM runner. Push changes to this branch or run
the **Linux ARM64 build** workflow manually from the Actions tab. The resulting
`gamemon-linux-aarch64` artifact contains the `gamemon` executable and this README.

On Fedora/Aurora Linux, install the GTK runtime libraries before launching the
binary (GTK4 and libadwaita). The build is for **Linux ARM64**, not macOS.

CPU utilization and memory statistics use Linux procfs/sysfs interfaces. CPU
model identification also checks the device tree, which is useful on ARM systems.
Intel-specific RAPL package-power readings and GPU telemetry depend on kernel
and driver support and may be unavailable on Apple Silicon Linux.

## Hardware validation

The original development system is an Intel Core i9-13900K, ASRock Z790 LiveMixer, 48 GB DDR5 and NVIDIA GeForce RTX 5070. This hardware must not be hardcoded into collectors.

## License

No redistribution license has been selected. Until a license is explicitly chosen, standard copyright restrictions apply.
