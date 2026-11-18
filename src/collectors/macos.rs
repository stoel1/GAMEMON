//! Native macOS collector backend.
//!
//! Uses Mach, sysctl, libproc and system_profiler for platform metrics, with
//! one optional macmon sampler shared across Apple Silicon collectors. Unsupported
//! readings remain unavailable rather than being represented as fabricated zeroes.

use std::ffi::{c_char, c_void, CString};

#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default)]
pub(crate) struct AppleSiliconMetrics {
    pub(crate) gpu_frequency_mhz: Option<u32>,
    pub(crate) gpu_active_ratio: Option<f32>,
    pub(crate) gpu_power_watts: Option<f64>,
    pub(crate) gpu_memory_in_use_bytes: Option<u64>,
    pub(crate) gpu_temperature_celsius: Option<f64>,
    pub(crate) cpu_power_watts: Option<f64>,
    pub(crate) cpu_temperature_celsius: Option<f64>,
    pub(crate) fans: Vec<(String, u64)>,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
pub(crate) struct AppleSiliconCollector {
    #[cfg(target_arch = "aarch64")]
    sampler: Option<macmon::Sampler>,
    #[cfg(target_arch = "aarch64")]
    last_sampler_attempt: Option<Instant>,
}

#[cfg(target_os = "macos")]
impl AppleSiliconCollector {
    pub(crate) fn sample(&mut self) -> Option<AppleSiliconMetrics> {
        #[cfg(target_arch = "aarch64")]
        {
            let now = Instant::now();
            if self.sampler.is_none()
                && self.last_sampler_attempt.is_none_or(|last| now.duration_since(last) >= Duration::from_secs(30))
            {
                self.last_sampler_attempt = Some(now);
                self.sampler = macmon::Sampler::new().ok();
            }

            let sampler = self.sampler.as_mut()?;
            let metrics = sampler.get_metrics_now().ok()?;

            let gpu_frequency_mhz = (metrics.gpu_freq_mhz > 0).then_some(metrics.gpu_freq_mhz);
            let cpu_has_frequency = metrics.cpu_tiers.iter().any(|tier| tier.freq_mhz > 0);
            let gpu_temperature_celsius = metrics.temp.gpu_temp_avg
                .filter(|value| value.is_finite() && (10.0..=150.0).contains(value))
                .map(f64::from);
            let cpu_temperature_celsius = metrics.temp.cpu_temp_avg
                .filter(|value| value.is_finite() && (10.0..=150.0).contains(value))
                .map(f64::from);
            let cpu_power_watts = (metrics.cpu_power.is_finite() && metrics.cpu_power > 0.0)
                .then_some(f64::from(metrics.cpu_power));
            let gpu_power_watts = (metrics.gpu_power.is_finite() && metrics.gpu_power > 0.0)
                .then_some(f64::from(metrics.gpu_power));
            let gpu_active_ratio = gpu_frequency_mhz
                .and_then(|_| valid_ratio(metrics.gpu_active_ratio));
            let fans = metrics.fans.iter()
                .filter(|fan| fan.rpm <= 100_000)
                .map(|fan| (fan.name.clone(), u64::from(fan.rpm)))
                .collect::<Vec<_>>();

            let sample = AppleSiliconMetrics {
                gpu_frequency_mhz,
                gpu_active_ratio,
                gpu_power_watts,
                gpu_memory_in_use_bytes: metrics.gpu_mem_in_use,
                gpu_temperature_celsius,
                cpu_power_watts,
                cpu_temperature_celsius,
                fans,
            };

            let has_data = sample.gpu_frequency_mhz.is_some()
                || sample.gpu_power_watts.is_some()
                || sample.gpu_memory_in_use_bytes.is_some()
                || sample.gpu_temperature_celsius.is_some()
                || sample.cpu_power_watts.is_some()
                || sample.cpu_temperature_celsius.is_some()
                || !sample.fans.is_empty()
                || (cpu_has_frequency && valid_ratio(metrics.cpu_active_ratio).is_some());

            return has_data.then_some(sample);
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            None
        }
    }
}

#[cfg(target_os = "macos")]
fn valid_ratio(value: f32) -> Option<f32> {
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(value)
}

pub mod cpu {
    const PROCESSOR_CPU_LOAD_INFO: i32 = 2;
    const CPU_STATE_USER: usize = 0;
    const CPU_STATE_SYSTEM: usize = 1;
    const CPU_STATE_IDLE: usize = 2;
    const CPU_STATE_NICE: usize = 3;
    const CPU_STATE_COUNT: usize = 4;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum CpuUtilization { Available(f64), WarmingUp, Unavailable }

    #[derive(Debug, Clone, Copy)]
    pub struct LogicalCpuSnapshot {
        pub id: u32,
        pub utilization: CpuUtilization,
        pub package_id: Option<u32>,
        pub core_id: Option<u32>,
        pub core_type: Option<u32>,
        pub current_frequency_hz: Option<u64>,
    }

    #[derive(Debug, Clone)]
    pub struct PowerDomainSnapshot { pub name: String, pub power_watts: Option<f64> }

    #[derive(Debug, Clone)]
    pub struct CpuSnapshot {
        pub model: Option<String>,
        pub online: Option<String>,
        pub utilization: CpuUtilization,
        pub logical_cpus: Vec<LogicalCpuSnapshot>,
        pub package_power_domains: Vec<PowerDomainSnapshot>,
    }

    #[derive(Debug, Default)]
    pub struct CpuCollector {
        previous: Vec<[u32; CPU_STATE_COUNT]>,
    }

    impl CpuCollector {
        pub fn sample(&mut self) -> CpuSnapshot {
            let model = super::sysctl_string("machdep.cpu.brand_string")
                .or_else(|| super::sysctl_string("hw.model"));
            let reported_count = super::sysctl_u32("hw.logicalcpu").filter(|n| *n > 0);
            let counters = read_processor_counters();
            let (utilization, logical_cpus) = match (reported_count, counters) {
                (Some(count), Some(current)) if !current.is_empty() => {
                    let baseline_matches = self.previous.len() == current.len();
                    let mut busy_total = 0u64;
                    let mut total_total = 0u64;
                    let mut cores = Vec::with_capacity(current.len());
                    for (id, values) in current.iter().enumerate() {
                        let core_util = if baseline_matches {
                            let old = self.previous[id];
                            let deltas = std::array::from_fn::<u32, CPU_STATE_COUNT, _>(|i| values[i].wrapping_sub(old[i]));
                            let total = deltas.iter().map(|v| u64::from(*v)).sum::<u64>();
                            let idle = u64::from(deltas[CPU_STATE_IDLE]);
                            let busy = total.saturating_sub(idle);
                            busy_total += busy;
                            total_total += total;
                            if total > 0 { CpuUtilization::Available(busy as f64 * 100.0 / total as f64) }
                            else { CpuUtilization::WarmingUp }
                        } else { CpuUtilization::WarmingUp };
                        cores.push(LogicalCpuSnapshot {
                            id: id as u32,
                            utilization: core_util,
                            package_id: Some(0),
                            core_id: Some(id as u32),
                            core_type: None,
                            current_frequency_hz: None,
                        });
                    }
                    self.previous = current;
                    let aggregate = if baseline_matches && total_total > 0 {
                        CpuUtilization::Available(busy_total as f64 * 100.0 / total_total as f64)
                    } else { CpuUtilization::WarmingUp };
                    let _ = count;
                    (aggregate, cores)
                }
                _ => {
                    self.previous.clear();
                    (CpuUtilization::Unavailable, Vec::new())
                }
            };
            let online_count = reported_count.unwrap_or(logical_cpus.len() as u32);
            CpuSnapshot {
                model,
                online: (online_count > 0).then(|| format!("0-{}", online_count - 1)),
                utilization,
                logical_cpus,
                package_power_domains: Vec::new(),
            }
        }
    }


    pub fn apply_apple_metrics(
        snapshot: &mut CpuSnapshot,
        metrics: Option<&super::AppleSiliconMetrics>,
    ) {
        if let Some(power_watts) = metrics.and_then(|value| value.cpu_power_watts) {
            if power_watts.is_finite() && power_watts > 0.0 {
                snapshot.package_power_domains.push(PowerDomainSnapshot {
                    name: "CPU package".to_owned(),
                    power_watts: Some(power_watts),
                });
            }
        }
    }

    #[cfg(test)]
    mod apple_metrics_tests {
        use super::*;

        #[test]
        fn cpu_package_power_is_only_added_for_a_valid_positive_reading() {
            let mut snapshot = CpuSnapshot {
                model: None,
                online: None,
                utilization: CpuUtilization::Unavailable,
                logical_cpus: Vec::new(),
                package_power_domains: Vec::new(),
            };
            apply_apple_metrics(&mut snapshot, Some(&super::super::AppleSiliconMetrics {
                cpu_power_watts: Some(0.0),
                ..Default::default()
            }));
            assert!(snapshot.package_power_domains.is_empty());

            apply_apple_metrics(&mut snapshot, Some(&super::super::AppleSiliconMetrics {
                cpu_power_watts: Some(3.75),
                ..Default::default()
            }));
            assert_eq!(snapshot.package_power_domains.len(), 1);
            assert_eq!(snapshot.package_power_domains[0].name, "CPU package");
            assert_eq!(snapshot.package_power_domains[0].power_watts, Some(3.75));
        }
    }

    fn read_processor_counters() -> Option<Vec<[u32; CPU_STATE_COUNT]>> {
        let mut processor_count = 0u32;
        let mut info: *mut i32 = std::ptr::null_mut();
        let mut info_count = 0u32;
        let host = unsafe { mach_host_self() };
        let result = unsafe {
            host_processor_info(
                host,
                PROCESSOR_CPU_LOAD_INFO,
                &mut processor_count,
                &mut info,
                &mut info_count,
            )
        };
        unsafe {
            let _ = mach_port_deallocate(mach_task_self_, host);
        }
        if result != 0 || info.is_null() {
            return None;
        }
        if processor_count == 0
            || info_count < processor_count.saturating_mul(CPU_STATE_COUNT as u32)
        {
            unsafe {
                let byte_count = (info_count as usize).saturating_mul(std::mem::size_of::<i32>());
                let _ = vm_deallocate(mach_task_self_, info as usize, byte_count);
            }
            return None;
        }
        let values = unsafe { std::slice::from_raw_parts(info, info_count as usize) };
        let counters = values.chunks_exact(CPU_STATE_COUNT)
            .take(processor_count as usize)
            .map(|row| [
                row[CPU_STATE_USER] as u32,
                row[CPU_STATE_SYSTEM] as u32,
                row[CPU_STATE_IDLE] as u32,
                row[CPU_STATE_NICE] as u32,
            ])
            .collect::<Vec<_>>();
        unsafe {
            let byte_count = (info_count as usize).saturating_mul(std::mem::size_of::<i32>());
            let _ = vm_deallocate(mach_task_self_, info as usize, byte_count);
        }
        (!counters.is_empty()).then_some(counters)
    }

    unsafe extern "C" {
        fn mach_host_self() -> u32;
        fn mach_port_deallocate(task: u32, name: u32) -> i32;
        static mut mach_task_self_: u32;
        fn host_processor_info(host: u32, flavor: i32, out_processor_count: *mut u32,
            out_processor_info: *mut *mut i32, out_processor_info_count: *mut u32) -> i32;
        fn vm_deallocate(target_task: u32, address: usize, size: usize) -> i32;
    }
}

pub mod gpu {
    use std::{process::Command, time::{Duration, Instant}};

    #[derive(Debug, Clone, Default)]
    pub struct GpuSnapshot { pub devices: Vec<GpuDeviceSnapshot> }
    #[derive(Debug, Clone)]
    pub struct GpuDeviceSnapshot {
        pub drm_card: Option<String>, pub telemetry_source: Option<&'static str>,
        pub telemetry_status: Option<&'static str>, pub uuid: Option<String>,
        pub pci_address: Option<String>, pub vendor_id: Option<u16>, pub device_id: Option<u16>,
        pub driver: Option<String>, pub model: Option<String>, pub gpu_busy_percent: Option<u8>,
        pub vram_used_bytes: Option<u64>, pub vram_total_bytes: Option<u64>,
        pub memory_busy_percent: Option<u8>, pub temperature_celsius: Option<u32>,
        pub power_milliwatts: Option<u32>, pub power_limit_milliwatts: Option<u32>,
        pub graphics_clock_mhz: Option<u32>, pub memory_clock_mhz: Option<u32>,
        pub fan_percent: Option<u8>, pub encoder_busy_percent: Option<u8>,
        pub decoder_busy_percent: Option<u8>,
    }

    fn ratio_to_percent(ratio: f32) -> Option<u8> {
        (ratio.is_finite() && (0.0..=1.0).contains(&ratio))
            .then_some((ratio * 100.0).round() as u8)
    }

    fn watts_to_milliwatts(watts: f32) -> Option<u32> {
        (watts.is_finite() && watts > 0.0 && watts <= 10_000.0)
            .then_some((watts * 1000.0).round() as u32)
    }

    fn celsius_to_u32(value: f32) -> Option<u32> {
        (value.is_finite() && (0.0..=150.0).contains(&value))
            .then_some(value.round() as u32)
    }

    #[cfg(test)]
    mod tests {
        use super::{celsius_to_u32, ratio_to_percent, watts_to_milliwatts};

        #[test]
        fn gpu_utilization_requires_a_valid_ratio() {
            assert_eq!(ratio_to_percent(0.0), Some(0));
            assert_eq!(ratio_to_percent(0.456), Some(46));
            assert_eq!(ratio_to_percent(1.0), Some(100));
            assert_eq!(ratio_to_percent(-0.1), None);
            assert_eq!(ratio_to_percent(1.1), None);
            assert_eq!(ratio_to_percent(f32::NAN), None);
        }

        #[test]
        fn gpu_power_does_not_turn_zero_or_invalid_values_into_readings() {
            assert_eq!(watts_to_milliwatts(0.0), None);
            assert_eq!(watts_to_milliwatts(1.25), Some(1250));
            assert_eq!(watts_to_milliwatts(-1.0), None);
            assert_eq!(watts_to_milliwatts(f32::INFINITY), None);
        }

        #[test]
        fn gpu_temperature_is_bounded_and_finite() {
            assert_eq!(celsius_to_u32(42.6), Some(43));
            assert_eq!(celsius_to_u32(0.0), Some(0));
            assert_eq!(celsius_to_u32(151.0), None);
            assert_eq!(celsius_to_u32(f32::NAN), None);
        }
    }

    #[derive(Default)]
    pub struct GpuCollector {
        last_scan: Option<Instant>,
        cached: Option<GpuSnapshot>,
    }

    impl GpuCollector {
        pub fn sample(&mut self) -> Option<GpuSnapshot> {
            self.sample_with_apple_metrics(None)
        }

        pub fn sample_with_apple_metrics(
            &mut self,
            apple_metrics: Option<&super::AppleSiliconMetrics>,
        ) -> Option<GpuSnapshot> {
            let now = Instant::now();
            if self.last_scan.is_none_or(|last| now.duration_since(last) >= Duration::from_secs(30)) {
                self.last_scan = Some(now);
                self.cached = read_gpu_identity();
            }

            if let (Some(metrics), Some(snapshot)) = (
                apple_metrics,
                self.cached.as_mut().and_then(|snapshot| snapshot.devices.first_mut()),
            ) {
                let frequency = metrics.gpu_frequency_mhz;
                let power = metrics.gpu_power_watts.and_then(|watts| {
                    (watts.is_finite() && watts > 0.0 && watts <= 10_000.0)
                        .then_some((watts * 1000.0).round() as u32)
                });
                let temperature = metrics.gpu_temperature_celsius.and_then(|value| {
                    (value.is_finite() && (0.0..=150.0).contains(&value))
                        .then_some(value.round() as u32)
                });
                let has_gpu_telemetry = frequency.is_some() || power.is_some()
                    || metrics.gpu_memory_in_use_bytes.is_some() || temperature.is_some();
                if has_gpu_telemetry {
                    snapshot.telemetry_source = Some("IOReport/AppleSMC");
                    snapshot.telemetry_status = Some("live");
                    snapshot.gpu_busy_percent = metrics.gpu_active_ratio
                        .and_then(|ratio| (ratio.is_finite() && (0.0..=1.0).contains(&ratio))
                            .then_some((ratio * 100.0).round() as u8));
                    snapshot.graphics_clock_mhz = frequency;
                    snapshot.power_milliwatts = power;
                    // Apple Silicon uses unified memory; this is GPU-attributed memory, not discrete VRAM.
                    snapshot.vram_used_bytes = metrics.gpu_memory_in_use_bytes;
                    snapshot.temperature_celsius = temperature;
                }
            }

            self.cached.clone()
        }
    }

    fn read_gpu_identity() -> Option<GpuSnapshot> {
        let output = Command::new("/usr/sbin/system_profiler")
            .args(["SPDisplaysDataType", "-detailLevel", "mini"])
            .env("LC_ALL", "C")
            .output().ok()?;
        if !output.status.success() { return None; }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut model = None;
        for line in text.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("Chipset Model:") {
                let value = value.trim();
                if !value.is_empty() { model = Some(value.to_owned()); break; }
            }
        }
        let model = model?;
        Some(GpuSnapshot {
            devices: vec![GpuDeviceSnapshot {
                drm_card: None,
                telemetry_source: Some("system_profiler"),
                telemetry_status: Some("identification-only"),
                uuid: None, pci_address: None, vendor_id: None, device_id: None,
                driver: Some("Metal".to_owned()), model: Some(model),
                gpu_busy_percent: None, vram_used_bytes: None, vram_total_bytes: None,
                memory_busy_percent: None, temperature_celsius: None, power_milliwatts: None,
                power_limit_milliwatts: None, graphics_clock_mhz: None, memory_clock_mhz: None,
                fan_percent: None, encoder_busy_percent: None, decoder_busy_percent: None,
            }]
        })
    }
}

pub mod network {
    use std::{collections::HashMap, process::Command, time::Instant};

    #[derive(Debug, Clone)]
    pub struct NetworkSnapshot { pub interfaces: Vec<NetworkInterfaceSnapshot> }
    #[derive(Debug, Clone)]
    pub struct NetworkInterfaceSnapshot {
        pub name: String, pub operational_state: Option<String>, pub carrier: Option<bool>,
        pub speed_mbps: Option<u64>, pub duplex: Option<String>, pub rx_bytes: Option<u64>,
        pub tx_bytes: Option<u64>, pub rx_bytes_per_second: Option<f64>,
        pub tx_bytes_per_second: Option<f64>, pub rx_errors: Option<u64>,
        pub tx_errors: Option<u64>, pub rx_dropped: Option<u64>, pub tx_dropped: Option<u64>,
    }
    #[derive(Debug, Clone, Copy)]
    struct Baseline { rx: u64, tx: u64, sampled_at: Instant }

    #[derive(Debug, Default)]
    pub struct NetworkCollector { previous: HashMap<String, Baseline> }

    impl NetworkCollector {
        pub fn sample(&mut self) -> Option<NetworkSnapshot> {
            let output = Command::new("/usr/sbin/netstat")
                .args(["-ibn"])
                .env("LC_ALL", "C")
                .output().ok()?;
            if !output.status.success() { return None; }
            let text = String::from_utf8_lossy(&output.stdout);
            let now = Instant::now();
            let mut interfaces = Vec::new();
            let mut next = HashMap::new();
            for line in text.lines().skip(1) {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.len() < 11 || !fields[2].starts_with("<Link#") { continue; }
                let name = fields[0];
                if name.is_empty() || name.len() > 32
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) { continue; }
                let (Some(rx), Some(rx_errors), Some(tx), Some(tx_errors)) = (
                    fields[6].parse::<u64>().ok(), fields[5].parse::<u64>().ok(),
                    fields[9].parse::<u64>().ok(), fields[8].parse::<u64>().ok()
                ) else { continue; };
                let baseline = self.previous.get(name).copied();
                let (rx_rate, tx_rate) = baseline.map(|old| {
                    let elapsed = now.duration_since(old.sampled_at).as_secs_f64();
                    if elapsed <= 0.0 { return (None, None); }
                    let rx_rate = rx.checked_sub(old.rx).map(|delta| delta as f64 / elapsed);
                    let tx_rate = tx.checked_sub(old.tx).map(|delta| delta as f64 / elapsed);
                    (rx_rate, tx_rate)
                }).unwrap_or((None, None));
                next.insert(name.to_owned(), Baseline { rx, tx, sampled_at: now });
                interfaces.push(NetworkInterfaceSnapshot {
                    name: name.to_owned(), operational_state: None, carrier: None,
                    speed_mbps: None, duplex: None, rx_bytes: Some(rx), tx_bytes: Some(tx),
                    rx_bytes_per_second: rx_rate, tx_bytes_per_second: tx_rate,
                    rx_errors: Some(rx_errors), tx_errors: Some(tx_errors),
                    rx_dropped: None, tx_dropped: None,
                });
            }
            self.previous = next;
            interfaces.sort_by(|a, b| a.name.cmp(&b.name));
            Some(NetworkSnapshot { interfaces })
        }
    }
}

pub mod system {
    #[derive(Debug, Clone, Default)]
    pub struct SystemSnapshot {
        pub uptime_seconds: Option<u64>, pub load_average: Option<[f64; 3]>,
        pub temperatures: Vec<TemperatureReading>, pub fans: Vec<FanReading>,
    }
    #[derive(Debug, Clone)]
    pub struct TemperatureReading { pub label: String, pub celsius: f64 }
    #[derive(Debug, Clone)]
    pub struct FanReading { pub label: String, pub rpm: u64 }
    pub fn sample() -> SystemSnapshot {
        sample_with_apple_metrics(None)
    }

    pub fn sample_with_apple_metrics(
        apple_metrics: Option<&super::AppleSiliconMetrics>,
    ) -> SystemSnapshot {
        let mut values = [0.0_f64; 3];
        let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
        let boot_time = super::sysctl_timeval("kern.boottime");
        let uptime_seconds = boot_time.and_then(|boot| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?;
            let boot = std::time::Duration::new(boot.tv_sec.max(0) as u64, boot.tv_usec.max(0) as u32 * 1000);
            now.checked_sub(boot).map(|duration| duration.as_secs())
        });
        let temperatures = apple_metrics.into_iter().flat_map(|metrics| {
            [
                metrics.cpu_temperature_celsius.map(|celsius| TemperatureReading {
                    label: "CPU".to_owned(),
                    celsius,
                }),
                metrics.gpu_temperature_celsius.map(|celsius| TemperatureReading {
                    label: "GPU".to_owned(),
                    celsius,
                }),
            ].into_iter().flatten()
        }).collect();
        let fans = apple_metrics.into_iter().flat_map(|metrics| {
            metrics.fans.iter().map(|(label, rpm)| FanReading {
                label: label.clone(),
                rpm: *rpm,
            })
        }).collect();

        SystemSnapshot {
            uptime_seconds,
            load_average: (count == 3 && values.iter().all(|v| v.is_finite() && *v >= 0.0)).then_some(values),
            temperatures,
            fans,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_available_apple_temperatures_and_fans() {
            let metrics = super::super::AppleSiliconMetrics {
                cpu_temperature_celsius: Some(51.5),
                gpu_temperature_celsius: Some(47.0),
                fans: vec![("fan0".to_owned(), 1200)],
                ..Default::default()
            };
            let snapshot = sample_with_apple_metrics(Some(&metrics));
            assert_eq!(snapshot.temperatures.len(), 2);
            assert_eq!(snapshot.temperatures[0].label, "CPU");
            assert_eq!(snapshot.temperatures[0].celsius, 51.5);
            assert_eq!(snapshot.temperatures[1].label, "GPU");
            assert_eq!(snapshot.temperatures[1].celsius, 47.0);
            assert_eq!(snapshot.fans.len(), 1);
            assert_eq!(snapshot.fans[0].label, "fan0");
            assert_eq!(snapshot.fans[0].rpm, 1200);
        }
    }
}

pub mod process {
    use std::{collections::HashMap, ffi::c_void, time::Instant};

    const PROC_PIDTASKINFO: i32 = 4;
    const PROCESS_LIMIT: usize = 4096;

    #[derive(Debug, Clone)]
    pub struct ProcessSnapshot {
        pub pid: u32,
        pub name: String,
        pub cpu_percent: Option<f64>,
        pub memory_bytes: Option<u64>,
        pub state: Option<char>,
    }
    #[derive(Debug, Clone, Default)]
    pub struct ProcessListSnapshot { pub processes: Vec<ProcessSnapshot> }

    #[derive(Debug, Clone, Copy)]
    struct PreviousSample { cpu_time_ns: u64, sampled_at: Instant }

    #[repr(C)]
    #[derive(Default)]
    struct ProcTaskInfo {
        virtual_size: u64,
        resident_size: u64,
        total_user: u64,
        total_system: u64,
        threads_user: u64,
        threads_system: u64,
        policy: i32,
        faults: i32,
        pageins: i32,
        cow_faults: i32,
        messages_sent: i32,
        messages_received: i32,
        syscalls_mach: i32,
        syscalls_unix: i32,
        context_switches: i32,
        threadnum: i32,
        num_running: i32,
        priority: i32,
    }

    #[derive(Debug, Default)]
    pub struct ProcessCollector {
        previous: HashMap<u32, PreviousSample>,
        logical_cpus: f64,
    }

    impl ProcessCollector {
        pub fn sample(&mut self) -> ProcessListSnapshot {
            let now = Instant::now();
            if self.logical_cpus <= 0.0 {
                self.logical_cpus = super::sysctl_u32("hw.logicalcpu").unwrap_or(1).max(1) as f64;
            }
            let mut pids = vec![0_i32; PROCESS_LIMIT];
            let count = unsafe {
                proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * std::mem::size_of::<i32>()) as i32)
            };
            if count <= 0 {
                return ProcessListSnapshot::default();
            }
            pids.truncate((count as usize).min(PROCESS_LIMIT));
            let mut next = HashMap::new();
            let mut processes = Vec::new();
            for raw_pid in pids {
                if raw_pid <= 0 { continue; }
                let pid = raw_pid as u32;
                let mut info = ProcTaskInfo::default();
                let bytes = unsafe {
                    proc_pidinfo(raw_pid, PROC_PIDTASKINFO, 0, (&mut info as *mut ProcTaskInfo).cast(),
                        std::mem::size_of::<ProcTaskInfo>() as i32)
                };
                if bytes != std::mem::size_of::<ProcTaskInfo>() as i32 { continue; }
                let cpu_time_ns = info.total_user.saturating_add(info.total_system);
                let cpu_percent = self.previous.get(&pid).and_then(|old| {
                    let delta = cpu_time_ns.checked_sub(old.cpu_time_ns)?;
                    let elapsed_ns = now.duration_since(old.sampled_at).as_nanos() as f64;
                    (elapsed_ns > 0.0).then_some((delta as f64 * 100.0 / elapsed_ns)
                        .min(100.0 * self.logical_cpus))
                });
                next.insert(pid, PreviousSample { cpu_time_ns, sampled_at: now });
                let mut name = [0_i8; 256];
                let name_len = unsafe { proc_name(raw_pid, name.as_mut_ptr().cast(), name.len() as u32) };
                let process_name = if name_len > 0 {
                    let bytes = unsafe { std::slice::from_raw_parts(name.as_ptr().cast::<u8>(), name_len as usize) };
                    String::from_utf8_lossy(bytes).trim_matches(char::from(0)).to_owned()
                } else {
                    format!("PID {pid}")
                };
                processes.push(ProcessSnapshot {
                    pid,
                    name: process_name,
                    cpu_percent,
                    memory_bytes: Some(info.resident_size),
                    state: None,
                });
            }
            self.previous = next;
            processes.sort_by(|a, b| b.cpu_percent.unwrap_or(0.0).total_cmp(&a.cpu_percent.unwrap_or(0.0))
                .then_with(|| b.memory_bytes.cmp(&a.memory_bytes)));
            processes.truncate(30);
            ProcessListSnapshot { processes }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::ProcTaskInfo;

        #[test]
        fn proc_taskinfo_matches_the_xnu_abi_size() {
            assert_eq!(std::mem::size_of::<ProcTaskInfo>(), 96);
        }
    }

    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_name(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }
}

pub mod storage {
    use std::{process::Command, time::{Duration, Instant}};

    #[derive(Debug, Clone)]
    pub struct StorageSnapshot { pub filesystems: Vec<FilesystemSnapshot>, pub devices: Vec<BlockDeviceSnapshot> }
    #[derive(Debug, Clone)]
    pub struct FilesystemSnapshot {
        pub device: String, pub mountpoint: String, pub filesystem: String,
        pub total_bytes: Option<u64>, pub used_bytes: Option<u64>, pub available_bytes: Option<u64>,
    }
    #[derive(Debug, Clone)]
    pub struct BlockDeviceSnapshot {
        pub name: String, pub read_bytes_per_second: Option<f64>, pub write_bytes_per_second: Option<f64>,
        pub reads_completed: Option<u64>, pub writes_completed: Option<u64>,
    }
    #[derive(Debug, Default)]
    pub struct StorageCollector {
        last_filesystem_scan: Option<Instant>,
        filesystems: Vec<FilesystemSnapshot>,
    }
    impl StorageCollector {
        pub fn sample(&mut self) -> StorageSnapshot {
            let now = Instant::now();
            if self.last_filesystem_scan.is_none_or(|last| now.duration_since(last) >= Duration::from_secs(10)) {
                if let Some(filesystems) = read_filesystems() {
                    self.filesystems = filesystems;
                    self.last_filesystem_scan = Some(now);
                }
            }
            StorageSnapshot { filesystems: self.filesystems.clone(), devices: Vec::new() }
        }
    }

    fn read_filesystems() -> Option<Vec<FilesystemSnapshot>> {
        let output = Command::new("/bin/df").args(["-kP"]).env("LC_ALL", "C").output().ok()?;
        if !output.status.success() { return None; }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut filesystems = Vec::new();
        for line in text.lines().skip(1) {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 6 { continue; }
            let (Some(total), Some(used), Some(available)) = (
                fields[1].parse::<u64>().ok(), fields[2].parse::<u64>().ok(),
                fields[3].parse::<u64>().ok()
            ) else { continue; };
            let mountpoint = fields[5..].join(" ");
            if !mountpoint.starts_with('/') { continue; }
            let scale = 1024u64;
            filesystems.push(FilesystemSnapshot {
                device: fields[0].to_owned(),
                mountpoint,
                filesystem: "unknown".to_owned(),
                total_bytes: total.checked_mul(scale),
                used_bytes: used.checked_mul(scale),
                available_bytes: available.checked_mul(scale),
            });
        }
        filesystems.sort_by(|a, b| a.mountpoint.cmp(&b.mountpoint));
        filesystems.dedup_by(|a, b| a.mountpoint == b.mountpoint);
        Some(filesystems)
    }
}

unsafe extern "C" {
    fn sysctlbyname(name: *const c_char, oldp: *mut c_void, oldlenp: *mut usize,
                    newp: *mut c_void, newlen: usize) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Timeval { tv_sec: i64, tv_usec: i32 }

fn sysctl_timeval(name: &str) -> Option<Timeval> {
    let name = CString::new(name).ok()?;
    let mut value = Timeval { tv_sec: 0, tv_usec: 0 };
    let mut length = std::mem::size_of::<Timeval>();
    (unsafe { sysctlbyname(name.as_ptr(), (&mut value as *mut Timeval).cast(), &mut length, std::ptr::null_mut(), 0) } == 0
        && length == std::mem::size_of::<Timeval>()).then_some(value)
}

fn sysctl_string(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut length = 0usize;
    if unsafe { sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut length, std::ptr::null_mut(), 0) } != 0 || length == 0 {
        return None;
    }
    let mut bytes = vec![0u8; length];
    if unsafe { sysctlbyname(name.as_ptr(), bytes.as_mut_ptr().cast(), &mut length, std::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    bytes.truncate(length);
    while bytes.last() == Some(&0) { bytes.pop(); }
    String::from_utf8(bytes).ok().filter(|s| !s.trim().is_empty())
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let name = CString::new(name).ok()?;
    let mut value = 0u64;
    let mut length = std::mem::size_of::<u64>();
    (unsafe { sysctlbyname(name.as_ptr(), (&mut value as *mut u64).cast(), &mut length, std::ptr::null_mut(), 0) } == 0
        && length == std::mem::size_of::<u64>()).then_some(value)
}

fn sysctl_u32(name: &str) -> Option<u32> {
    let name = CString::new(name).ok()?;
    let mut value = 0u32;
    let mut length = std::mem::size_of::<u32>();
    (unsafe { sysctlbyname(name.as_ptr(), (&mut value as *mut u32).cast(), &mut length, std::ptr::null_mut(), 0) } == 0
        && length == std::mem::size_of::<u32>()).then_some(value)
}

pub mod memory {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct MemorySnapshot {
        pub total_bytes: Option<u64>, pub available_bytes: Option<u64>,
        pub free_bytes: Option<u64>, pub buffers_bytes: Option<u64>,
        pub cached_bytes: Option<u64>, pub swap_total_bytes: Option<u64>,
        pub swap_free_bytes: Option<u64>, pub huge_pages_total: Option<u64>,
        pub huge_pages_free: Option<u64>, pub huge_page_size_bytes: Option<u64>,
        pub pressure: Option<MemoryPressure>,
    }
    impl MemorySnapshot {
        pub fn used_bytes(self) -> Option<u64> {
            Some(self.total_bytes?.saturating_sub(self.available_bytes?))
        }
        pub fn swap_used_bytes(self) -> Option<u64> {
            Some(self.swap_total_bytes?.saturating_sub(self.swap_free_bytes?))
        }
    }
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct MemoryPressure { pub some: PressureMetrics, pub full: Option<PressureMetrics> }
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct PressureMetrics { pub avg10: f64, pub avg60: f64, pub avg300: f64, pub total_us: Option<u64> }

    pub fn sample() -> MemorySnapshot {
        let page_size = super::sysctl_u64("hw.pagesize").filter(|v| *v > 0);
        let free_pages = super::sysctl_u64("vm.page_free_count");
        let inactive_pages = super::sysctl_u64("vm.page_inactive_count");
        let speculative_pages = super::sysctl_u64("vm.page_speculative_count");
        let available_bytes = page_size.zip(free_pages)
            .and_then(|(size, free)| free.checked_add(inactive_pages.unwrap_or(0))?
                .checked_add(speculative_pages.unwrap_or(0))?.checked_mul(size));
        let free_bytes = page_size.zip(free_pages).and_then(|(size, pages)| pages.checked_mul(size));
        let total_bytes = super::sysctl_u64("hw.memsize");
        let available_bytes = available_bytes.zip(total_bytes)
            .map(|(available, total)| available.min(total));
        MemorySnapshot {
            total_bytes, available_bytes, free_bytes, ..Default::default()
        }
    }
}
