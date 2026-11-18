//! Initial native macOS collector backend.
//!
//! Metrics are deliberately left unavailable until they can be read from a
//! documented macOS interface. Returning None is preferable to displaying
//! guessed or Linux-specific values.

use std::ffi::{c_char, c_void, CString};

pub mod cpu {
    use super::*;
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
                            core_type: core_type_for(id as u32),
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

    fn core_type_for(id: u32) -> Option<u32> {
        let performance = super::sysctl_u32("hw.perflevel0.logicalcpu");
        let efficiency = super::sysctl_u32("hw.perflevel1.logicalcpu");
        match (performance, efficiency) {
            (Some(p), Some(e)) if p > 0 && e > 0 => {
                if id < p { Some(2) } else if id < p.saturating_add(e) { Some(1) } else { None }
            }
            (Some(p), _) if p > 0 => Some(2),
            _ => None,
        }
    }

    fn read_processor_counters() -> Option<Vec<[u32; CPU_STATE_COUNT]>> {
        let mut processor_count = 0u32;
        let mut info: *mut i32 = std::ptr::null_mut();
        let mut info_count = 0u32;
        let result = unsafe {
            host_processor_info(
                mach_host_self(),
                PROCESSOR_CPU_LOAD_INFO,
                &mut processor_count,
                &mut info,
                &mut info_count,
            )
        };
        if result != 0 || info.is_null() || processor_count == 0
            || info_count < processor_count.saturating_mul(CPU_STATE_COUNT as u32)
        {
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
            let _ = vm_deallocate(mach_task_self(), info as usize, byte_count);
        }
        (!counters.is_empty()).then_some(counters)
    }

    unsafe extern "C" {
        fn mach_host_self() -> u32;
        fn mach_task_self() -> u32;
        fn host_processor_info(host: u32, flavor: i32, out_processor_count: *mut u32,
            out_processor_info: *mut *mut i32, out_processor_info_count: *mut u32) -> i32;
        fn vm_deallocate(target_task: u32, address: usize, size: usize) -> i32;
    }
}

pub mod gpu {
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
    #[derive(Debug, Default)]
    pub struct GpuCollector;
    impl GpuCollector { pub fn sample(&mut self) -> Option<GpuSnapshot> { None } }
}

pub mod network {
    use std::time::Instant;
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
    #[derive(Debug, Default)]
    pub struct NetworkCollector { _sampled_at: Option<Instant> }
    impl NetworkCollector { pub fn sample(&mut self) -> Option<NetworkSnapshot> { None } }
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
        let mut values = [0.0_f64; 3];
        let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
        let boot_time = super::sysctl_timeval("kern.boottime");
        let uptime_seconds = boot_time.and_then(|boot| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?;
            let boot = std::time::Duration::new(boot.tv_sec.max(0) as u64, boot.tv_usec.max(0) as u32 * 1000);
            now.checked_sub(boot).map(|duration| duration.as_secs())
        });
        SystemSnapshot {
            uptime_seconds,
            load_average: (count == 3 && values.iter().all(|v| v.is_finite() && *v >= 0.0)).then_some(values),
            ..Default::default()
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
        num_running: u64,
        priority: u64,
        policy: u64,
        faults: u64,
        pageins: u64,
        cow_faults: u64,
        messages_sent: u64,
        messages_received: u64,
        syscalls_mach: u64,
        syscalls_unix: u64,
        context_switches: u64,
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

    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, buffersize: i32) -> i32;
        fn proc_name(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }
}

pub mod storage {
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
    pub struct StorageCollector;
    impl StorageCollector {
        pub fn sample(&mut self) -> StorageSnapshot {
            StorageSnapshot { filesystems: Vec::new(), devices: Vec::new() }
        }
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
}pub mod memory {
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

pub mod gpu {
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
    #[derive(Debug, Default)]
    pub struct GpuCollector;
    impl GpuCollector { pub fn sample(&mut self) -> Option<GpuSnapshot> { None } }
}

pub mod network {
    use std::time::Instant;
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
    #[derive(Debug, Default)]
    pub struct NetworkCollector { _sampled_at: Option<Instant> }
    impl NetworkCollector { pub fn sample(&mut self) -> Option<NetworkSnapshot> { None } }
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
        let mut values = [0.0_f64; 3];
        let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
        SystemSnapshot {
            load_average: (count == 3 && values.iter().all(|v| v.is_finite() && *v >= 0.0)).then_some(values),
            ..Default::default()
        }
    }
}

pub mod process {
    #[derive(Debug, Clone)]
    pub struct ProcessSnapshot {
        pub pid: u32, pub name: String, pub cpu_percent: Option<f64>,
        pub memory_bytes: Option<u64>, pub state: Option<char>,
    }
    #[derive(Debug, Clone, Default)]
    pub struct ProcessListSnapshot { pub processes: Vec<ProcessSnapshot> }
    #[derive(Debug, Default)]
    pub struct ProcessCollector;
    impl ProcessCollector { pub fn sample(&mut self) -> ProcessListSnapshot { ProcessListSnapshot::default() } }
}

pub mod storage {
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
    pub struct StorageCollector;
    impl StorageCollector {
        pub fn sample(&mut self) -> StorageSnapshot {
            StorageSnapshot { filesystems: Vec::new(), devices: Vec::new() }
        }
    }
}

unsafe extern "C" {
    fn sysctlbyname(name: *const c_char, oldp: *mut c_void, oldlenp: *mut usize,
                    newp: *mut c_void, newlen: usize) -> i32;
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
