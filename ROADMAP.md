# GAMEMON roadmap

The Rust application is the primary development target. The former Bash monitor is archived under legacy/bash and remains available as a comparison baseline until feature parity and validation are complete.

## Migration
- [x] Select archive migration strategy.
- [x] Preserve the original Bash monitor under legacy/bash/gamemon.
- [x] Establish Rust application and modular collector layout.
- [ ] Validate build against RHEL 10 development packages.
- [ ] Complete hardware inventory and approve final data model.

## Telemetry
- [ ] CPU topology, utilization, frequency, temperature, power and idle states.
- [ ] Memory, swap and PSI.
- [ ] NVIDIA telemetry through NVML, with controlled fallback.
- [ ] AMD and Intel GPU telemetry through supported DRM interfaces.
- [ ] Storage and NVMe telemetry.
- [ ] Network throughput and interface counters.
- [ ] Motherboard temperatures and fan telemetry through hwmon.
- [ ] PCIe, USB, audio and display inventory.

## Application
- [ ] Configurable sampling intervals and sensor selection.
- [ ] Dashboard and per-device views.
- [ ] Historical graphs and status presentation.
- [ ] Hardware discovery and hotplug handling.
- [ ] RHEL 10 RPM packaging and lifecycle tests.

## Quality
- [ ] Unit and integration tests for collectors.
- [ ] Missing/unsupported/permission-denied sensor tests.
- [ ] Compare readings against Linux tools and archived Bash monitor.
- [ ] Measure CPU and memory overhead.
