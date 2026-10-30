# GAMEMON

GAMEMON is a Linux hardware-monitoring application migrating from its original Bash terminal monitor to a modular Rust application with a GTK4/libadwaita desktop interface.

## Status

The Rust application is under active development. This initial migration establishes the application structure and basic CPU identity/online-CPU and memory discovery. Telemetry will be added incrementally and validated across different hardware and kernel/driver combinations.

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

On RHEL, install matching GTK4 and libadwaita development packages from enabled repositories before building.

## Hardware validation

The original development system is an Intel Core i9-13900K, ASRock Z790 LiveMixer, 48 GB DDR5 and NVIDIA GeForce RTX 5070. This hardware must not be hardcoded into collectors.

## License

No redistribution license has been selected. Until a license is explicitly chosen, standard copyright restrictions apply.
