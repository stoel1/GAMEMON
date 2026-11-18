# macOS port plan

## Starting point

This branch is based on `rust-rewrite`. The existing application is a Rust/GTK4/libadwaita desktop app, but its collectors currently read Linux-specific interfaces such as `/proc`, `/sys`, hwmon, DRM sysfs and Linux powercap/RAPL. The application therefore does not currently build as a native macOS application.

## Porting strategy

Keep the existing collector snapshot types and sampler/UI contract where practical, and introduce platform-specific collector implementations behind a shared interface:

- `src/platform/linux/`: retain the existing Linux implementations.
- `src/platform/macos/`: implement macOS data collection.
- `src/platform/mod.rs`: select the implementation with `cfg(target_os = ...)`.
- Keep common parsing, rate calculations, history and presentation logic platform-independent.

Do not report Linux-only metrics as zero on macOS. Return unavailable/unsupported values where macOS does not expose an equivalent.

## macOS data sources

Initial implementation should use documented system interfaces:

- CPU identity/topology and CPU time: `sysctl` and Mach host processor APIs.
- Memory and VM pressure: Mach host statistics and `sysctl`.
- Process list and per-process resource use: libproc / proc_pidinfo, with permission failures represented explicitly.
- Filesystem capacity: `statvfs`; mounted volumes via the appropriate macOS APIs.
- Network interfaces and byte counters: `getifaddrs` plus supported interface statistics.
- Uptime and load averages: `sysctl` / `getloadavg`.
- Apple Silicon GPU: investigate Metal / IOKit telemetry; expose only metrics that can be read reliably.
- Temperature, fan speed and power: investigate public IOKit/SMC interfaces. These may be unavailable on some Macs and must not be fabricated.

## GUI decision

The current UI depends on GTK4 and libadwaita. GTK4 has macOS support, but libadwaita is GNOME-oriented and should not be assumed to provide a supported native macOS experience. Before committing to a UI migration, test a minimal macOS build. If libadwaita blocks the build or results in a poor macOS experience, separate the UI from the monitoring core and select a cross-platform or native macOS frontend.

## Target architectures

Build and test both:

- `aarch64-apple-darwin` (Apple Silicon, including the MacBook Neo).
- `x86_64-apple-darwin` (Intel Macs).

## Milestones

1. Make the crate compile on macOS by isolating Linux-only modules and dependencies.
2. Implement CPU, memory, uptime/load, process, network and filesystem collectors.
3. Add Apple Silicon GPU telemetry where reliable public APIs permit it.
4. Investigate sensor telemetry and clearly mark unavailable readings.
5. Validate the GUI and decide whether a separate macOS frontend is needed.
6. Add macOS CI builds and package a signed/notarized app only after the build is stable.

## Acceptance criteria

- Existing Linux functionality remains intact.
- `cargo test` and CI pass on Linux and macOS.
- No Linux-specific path is accessed on macOS.
- Missing permissions or unsupported metrics are represented as unavailable, not as zero.
- Both Apple Silicon and Intel macOS targets are checked.
