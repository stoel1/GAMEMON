# Roadmap

## Phase 1 — correctness and sensor reliability

- [ ] Validate sensor mappings against documented hwmon labels.
- [ ] Make CPU topology discovery dynamic instead of relying on fixed i9-13900K core IDs.
- [ ] Handle missing, unreadable and malformed sensor values consistently.
- [ ] Avoid assuming every system exposes 32 logical CPUs or 48 core temperature labels.
- [ ] Add shell syntax checks and a lightweight test harness.

## Phase 2 — CPU telemetry

- [ ] Integrate `turbostat` where supported.
- [ ] Add package/core power and effective frequency reporting.
- [ ] Add C-state residency reporting.
- [ ] Keep a graceful fallback when turbostat or required permissions are unavailable.

## Phase 3 — terminal interface

- [ ] Improve layout, alignment and terminal-size handling.
- [ ] Add configurable refresh interval and display options.
- [ ] Add clear unavailable/unsupported sensor states.
- [ ] Consider color and compact modes.

## Phase 4 — RHEL RPM

- [ ] Validate RPM build on RHEL 10.
- [ ] Add package-owned configuration and documentation.
- [ ] Verify install, upgrade and erase behavior.
- [ ] Add package checks and clean build instructions.

## Phase 5 — release quality

- [ ] Add automated static checks.
- [ ] Document supported hardware and tested configurations.
- [ ] Add versioned releases and release notes.
