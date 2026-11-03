# GAMEMON roadmap

The Rust application is the primary development target. The former Bash monitor is archived under legacy/bash and remains available as a comparison baseline until feature parity and validation are complete.

## Migration
- [x] Select archive migration strategy.
- [x] Preserve the original Bash monitor under legacy/bash/gamemon.
- [x] Establish Rust application and modular collector layout.
- [ ] Validate build against RHEL 10 development packages.
- [ ] Complete hardware inventory and approve final data model.

## Telemetry
- [x] CPU identity, online CPU range, aggregate/per-logical utilization, package/core topology and exported current frequency.
- [ ] Hybrid core classification, temperature, throttling and idle states.
- [x] CPU package power from dynamically discovered powercap/RAPL energy counters.
- [x] RAM capacity, available/free memory, buffers and cache.
- [x] Swap, memory PSI and HugePages when exported by the kernel.
- [x] Dynamic DRM device inventory and exported busy/VRAM attributes.
- [x] NVIDIA NVML telemetry for utilization, VRAM, temperature, power, clocks, fan and media engines.
- [x] NVIDIA `nvidia-smi` fallback when NVML cannot initialize.
- [ ] GPU PCIe link data and per-process GPU telemetry.
- [ ] AMD and Intel GPU telemetry through supported DRM interfaces.
- [x] Mounted filesystem capacity and block-device read/write throughput from procfs/statvfs.
- [ ] NVMe health, temperature, wear and SMART telemetry.
- [x] Network interface status/counters, throughput rates, errors and drops.
- [x] Dynamic hwmon temperature and fan RPM discovery when exported by the kernel.
- [ ] PCIe, USB, audio and display inventory.

## Application
- [x] One-second default refresh interval.
- [x] Background sampler and bounded snapshot delivery to keep collector reads off the GTK thread.
- [x] Configurable sampling interval.
- [x] Bounded process overview with CPU-time delta and resident memory.
- [ ] Sensor selection.
- [x] Eerste metricgerichte dashboard met CPU-, GPU-, geheugen- en netwerkkaarten.
- [x] Primaire waarden, ondersteunende details en gebruiksindicatoren in de overzichtskaarten.
- [x] Rolling 60-sample utilization charts for CPU, GPU and memory with autoscaled, labeled axes and time context.
- [x] System uptime and load average overview panel.
- [x] Temperature and fan overview panel backed by hwmon.
- [x] Storage filesystem usage and block I/O overview panel.
- [x] Per-logical-CPU utilization and frequency overview.
- [x] Two independent dashboard columns prevent equal-height row gaps.
- [x] Overview detail text is line-limited to stabilize card heights.
- [ ] Responsive switch to a single column on narrow windows.
- [x] Reduce default information density in CPU, storage and process overview cards.
- [x] Smooth interpolation of CPU/GPU/memory chart traces and progress indicators between hardware samples.
- [ ] Dashboard verder verfijnen met statusindicatoren, tijdschaal en lege-/foutstatussen.
- [ ] Afzonderlijke apparaatweergaven en navigatie.
- [ ] Hardware discovery and hotplug handling.
- [ ] RHEL 10 RPM packaging and lifecycle tests.

## Quality
- [x] Parser unit tests for initial CPU and memory collectors.
- [ ] Unit and integration tests for remaining collectors.
- [ ] Missing/unsupported/permission-denied sensor tests.
- [ ] Compare readings against Linux tools and archived Bash monitor.
- [ ] Measure CPU and memory overhead.
