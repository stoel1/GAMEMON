# macOS port plan

## Starting point

This branch is based on `rust-rewrite`. The existing application is a Rust/GTK4/libadwaita desktop app, but its collectors currently read Linux-specific interfaces such as `/proc`, `/sys`, hwmon, DRM sysfs and Linux powercap/RAPL. The application therefore does not currently build as a native macOS application.

## Current audit

A source-level audit of every collector and the GUI dependency boundary is documented in [macOS port audit](macos-port-audit.md). It identifies the Linux-only paths, reusable POSIX/core components, and the GTK4/libadwaita build gate.

## Porting strategy

Keep the existing collector snapshot types and sampler/UI contract where practical. The current implementation selects platform-specific collector modules in `src/collectors/mod.rs`:

- `src/collectors/`: retain the existing Linux implementations unchanged.
- `src/collectors/macos.rs`: native macOS implementations selected with `cfg(target_os = "macos")`.
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
- Apple Silicon sensors: use the MIT-licensed `macmon` Rust library behind an AArch64-only dependency. A single sampler is read once per monitoring cycle and its result is shared by the CPU, GPU and system collectors. The current mapping includes GPU residency/frequency, GPU power, GPU-attributed unified memory, CPU package power, CPU/GPU temperatures and available fan RPM readings. Unsupported or zero-fallback channels remain unavailable rather than being displayed as fabricated zeroes. Some readings depend on private Apple interfaces and require validation on real hardware. GPU memory is unified system memory, not discrete VRAM.
- MacBook Neo/A18 Pro compatibility evidence: upstream `macmon` issue [#57](https://github.com/vladkens/macmon/issues/57) contains a real MacBook Neo/A18 Pro debug report, including CPU and GPU residency, energy and SMC temperature channels. The A-series frequency-unit correction was included in macmon 0.7.2; the integrated 0.12 dependency includes the A1x Hz scaling path. For macOS 27, macmon 0.12 also includes an explicit `AppleT8140CLPC` energy-counter mapping. This is strong upstream support evidence, not a substitute for testing this exact Neo build.
- CPU logical-processor utilization remains sourced from Mach host processor counters. Per-core frequency and hybrid-core identity are not inferred from macmon's IOReport core IDs yet; the mapping needs validation before it can be exposed.

## GUI decision

The current UI depends on GTK4 and libadwaita. GTK4 has macOS support, but libadwaita is GNOME-oriented and should not be assumed to provide a supported native macOS experience. Before committing to a UI migration, test a minimal macOS build. If libadwaita blocks the build or results in a poor macOS experience, separate the UI from the monitoring core and select a cross-platform or native macOS frontend.

## Target architectures

Build and test both:

- `aarch64-apple-darwin` (Apple Silicon, including the MacBook Neo).
- `x86_64-apple-darwin` (Intel Macs).

## Milestones

1. Make the crate compile on macOS by isolating Linux-only modules and dependencies.
2. Implement CPU, memory, uptime/load, process, network and filesystem collectors.
3. Integrate Apple Silicon telemetry behind a target-specific dependency and share one sampler across collectors; validate IOReport/SMC availability on real hardware and keep unsupported readings unavailable.
4. Map per-core CPU frequencies and core classes only after verifying IOReport-to-Mach CPU identity on supported Apple Silicon models.
5. Validate the GUI and decide whether a separate macOS frontend is needed.
6. Add macOS CI builds and package a signed/notarized app only after the build is stable.

## Acceptance criteria

- Existing Linux functionality remains intact.
- `cargo test` and CI pass on Linux and macOS.
- No Linux-specific path is accessed on macOS.
- Missing permissions or unsupported metrics are represented as unavailable, not as zero.
- Both Apple Silicon and Intel macOS targets are checked.
