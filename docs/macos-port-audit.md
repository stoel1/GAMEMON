# macOS port audit

Audit of the `rust-rewrite` branch (2026-11-18). This document records the current source-level blockers before platform code is changed.

## Summary

The Rust application is a useful starting point for a macOS port: the snapshot types, sampler thread, rate calculations, configuration and most UI presentation are separable from Linux. However, the current collectors are not cross-platform. Several modules directly read Linux procfs/sysfs paths, and the current application entry point depends on libadwaita.

The first macOS milestone should be a compile-time platform split that preserves the existing Linux collector implementation unchanged. macOS collectors should implement the same public snapshot contracts. Unsupported metrics must remain `None`/unavailable rather than being filled with guessed values.

## Source audit

| Source | Current Linux dependency | Porting direction |
|---|---|---|
| `src/collectors/cpu/mod.rs` | `/proc/cpuinfo`, `/proc/stat`, `/sys/devices/system/cpu/*`, cpufreq topology | macOS CPU identity via `sysctl`; utilization via Mach host processor load counters; discover logical CPUs with supported system APIs. Linux topology and frequency details remain Linux-only. |
| `src/collectors/cpu/rapl.rs` | `/sys/class/powercap` energy counters | Keep RAPL Linux-only. macOS package power is a separate investigation and must not be presented as RAPL. |
| `src/collectors/memory/mod.rs` | `/proc/meminfo`, `/proc/pressure/memory` | Use Mach VM statistics and `sysctl` for memory/swap values. Linux PSI and HugePages fields remain unavailable unless an actual macOS equivalent is found. |
| `src/collectors/process.rs` | `/proc/<pid>/stat`, Linux CPU online list | Use `libproc` / `proc_pidinfo` for process identity, resident memory and CPU-time counters. Keep the existing cross-sample rate calculation where semantics match. |
| `src/collectors/network/mod.rs` | `/sys/class/net/*` counters and link attributes | Enumerate interfaces with `getifaddrs` and obtain counters through supported Darwin interfaces. Keep the counter-delta/rate logic. |
| `src/collectors/storage.rs` | `/proc/diskstats`, `/proc/self/mounts` | `statvfs` is already a portable POSIX call; replace mount enumeration and block I/O discovery with Darwin APIs. |
| `src/collectors/system.rs` | `/proc/uptime`, `/proc/loadavg`, `/sys/class/hwmon` | Use `sysctl` / `getloadavg` for uptime and load. Temperature and fan data require a separate, hardware-dependent sensor provider; do not assume Linux hwmon exists. |
| `src/collectors/gpu/mod.rs` | `/sys/class/drm`, DRM sysfs attributes, NVML and `nvidia-smi` | Create a distinct macOS GPU provider. Investigate Metal/IOKit capabilities on Apple Silicon; only publish metrics that have a reliable source. Linux DRM/NVML code remains Linux-only. |
| `src/core/sampler.rs` | No direct Linux paths | Platform-neutral sampler can remain shared once all collector modules preserve their public interfaces. |
| `src/app.rs` / `src/main.rs` | GTK4 plus libadwaita application/window/header/toolbar types | Test the actual GTK4/libadwaita dependency chain on macOS early. The UI currently uses libadwaita `Application`, `ApplicationWindow`, `HeaderBar` and `ToolbarView`; if libadwaita is not viable on macOS, isolate the UI rather than rewriting collectors around it. |

## Important implementation observations

- `src/collectors/storage.rs` already uses `libc::statvfs` for filesystem capacity. That POSIX portion can likely be shared, while Linux mount and disk-stat enumeration cannot.
- `src/collectors/process.rs` uses `libc::sysconf` for clock ticks and page size. These calls are not the main blocker; reading Linux `/proc` files is.
- `src/core/sampler.rs` owns collectors on a background thread and sends bounded snapshots to the GTK thread. This is a good platform-independent boundary.
- The GPU collector contains an Apple AGX special case for the Aurora Linux driver (`driver=aurora`, `apple,agx-t8140`). This is specifically Linux-on-Apple-Silicon support and is not macOS GPU telemetry.
- `Cargo.toml` currently describes the package as a Linux hardware-monitoring dashboard and has unconditional GTK4/libadwaita dependencies. The package metadata and dependency strategy need to be updated as the port becomes real.

## Recommended sequence

1. Introduce `src/collectors/linux/` and `src/collectors/macos/` behind `cfg(target_os)`, preserving the existing Linux implementations and public collector contracts.
2. Implement the first macOS vertical slice: CPU identity/utilization, memory, uptime/load and filesystem capacity.
3. Add process and network providers using Darwin APIs.
4. Investigate Apple Silicon GPU telemetry separately from Linux DRM and Aurora AGX telemetry.
5. Validate GTK4/libadwaita on a real macOS build. If it blocks, split the UI from the shared monitoring core and select a supported macOS frontend.
6. Add macOS CI for `aarch64-apple-darwin` and `x86_64-apple-darwin`; keep Linux CI and tests passing throughout.

## Non-goals for the first macOS milestone

- No guessed CPU frequency, power, temperature, fan or GPU utilization values.
- No use of private undocumented sensor interfaces as if they were stable public APIs.
- No change to the Linux collector behavior merely to make macOS compile.
