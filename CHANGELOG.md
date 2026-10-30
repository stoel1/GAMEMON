# Changelog

## Unreleased

- Added runtime discovery of online logical CPUs; removed fixed 32-thread display assumptions.
- Added generic CPU temperature label handling for Intel and common AMD/ARM hwmon drivers.
- Added dynamic multi-GPU NVIDIA reporting and best-effort AMD/Intel DRM sysfs telemetry.
- Kept RAM and swap capacity detection dynamic through `/proc/meminfo`.
- Replaced fixed P-core/E-core temperature numbering with discovered core labels.


All notable project changes will be recorded here.

## Unreleased — initial repository baseline

- Added the currently supplied GAMEMON Bash monitor as `src/gamemon`.
- Documented current telemetry, hardware assumptions and known limitations.
- Added an initial RPM packaging spec and packaging notes.
- Integrated optional unprivileged `turbostat` sampling with graceful fallback to `/proc` and hwmon data.
- Established a roadmap for sensor abstraction, `turbostat` integration and UI improvements.

This is a development baseline, not a tagged stable release.
