use std::{ffi::CString, fs};

#[derive(Debug, Clone)]
pub struct StorageSnapshot {
    pub filesystems: Vec<FilesystemSnapshot>,
    pub devices: Vec<BlockDeviceSnapshot>,
}

#[derive(Debug, Clone)]
pub struct FilesystemSnapshot {
    pub device: String,
    pub mountpoint: String,
    pub filesystem: String,
    pub total_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct BlockDeviceSnapshot {
    pub name: String,
    pub read_bytes_per_second: Option<f64>,
    pub write_bytes_per_second: Option<f64>,
    pub reads_completed: Option<u64>,
    pub writes_completed: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct DiskCounters {
    sectors_read: u64,
    sectors_written: u64,
    sampled_at: std::time::Instant,
}

#[derive(Debug, Default)]
pub struct StorageCollector {
    previous: std::collections::HashMap<String, DiskCounters>,
}

impl StorageCollector {
    pub fn sample(&mut self) -> StorageSnapshot {
        let now = std::time::Instant::now();
        let devices = fs::read_to_string("/proc/diskstats")
            .ok()
            .map(|contents| parse_diskstats(&contents, &mut self.previous, now))
            .unwrap_or_default();
        StorageSnapshot {
            filesystems: read_filesystems(),
            devices,
        }
    }
}

fn parse_diskstats(
    contents: &str,
    previous: &mut std::collections::HashMap<String, DiskCounters>,
    now: std::time::Instant,
) -> Vec<BlockDeviceSnapshot> {
    let mut current = std::collections::HashMap::new();
    let mut devices = Vec::new();
    for line in contents.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 14 { continue; }
        let Some(name) = fields.get(2).copied().filter(|name| valid_device_name(name)) else { continue };
        let parsed = (
            fields[3].parse::<u64>().ok(),
            fields[5].parse::<u64>().ok(),
            fields[7].parse::<u64>().ok(),
            fields[9].parse::<u64>().ok(),
        );
        let (Some(reads), Some(sectors_read), Some(writes), Some(sectors_written)) = parsed else { continue };
        let baseline = previous.get(name).copied();
        let (read_rate, write_rate) = baseline.map(|old| {
            let elapsed = now.duration_since(old.sampled_at).as_secs_f64();
            if elapsed <= 0.0 { return (None, None); }
            let read_delta = sectors_read.checked_sub(old.sectors_read).map(|value| value as f64 * 512.0 / elapsed);
            let write_delta = sectors_written.checked_sub(old.sectors_written).map(|value| value as f64 * 512.0 / elapsed);
            (read_delta, write_delta)
        }).unwrap_or((None, None));
        current.insert(name.to_owned(), DiskCounters { sectors_read, sectors_written, sampled_at: now });
        devices.push(BlockDeviceSnapshot {
            name: name.to_owned(),
            read_bytes_per_second: read_rate,
            write_bytes_per_second: write_rate,
            reads_completed: Some(reads),
            writes_completed: Some(writes),
        });
    }
    *previous = current;
    devices.sort_by(|a,b| a.name.cmp(&b.name));
    devices
}

fn valid_device_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 &&
        name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._!-".contains(&byte))
}

fn read_filesystems() -> Vec<FilesystemSnapshot> {
    let Ok(contents) = fs::read_to_string("/proc/self/mounts") else { return Vec::new() };
    let mut filesystems = Vec::new();
    for line in contents.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 3 { continue; }
        let device = unescape_mount_field(fields[0]);
        let mountpoint = unescape_mount_field(fields[1]);
        let filesystem = fields[2].to_owned();
        if !mountpoint.starts_with('/') || !matches!(filesystem.as_str(),
            "ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "vfat" | "f2fs" | "zfs" | "ntfs" | "ntfs3" | "exfat")
        { continue; }
        let (total_bytes, available_bytes) = filesystem_space(&mountpoint);
        let used_bytes = total_bytes.zip(available_bytes).map(|(total, available)| total.saturating_sub(available));
        filesystems.push(FilesystemSnapshot { device, mountpoint, filesystem, total_bytes, used_bytes, available_bytes });
    }
    filesystems.sort_by(|a,b| a.mountpoint.cmp(&b.mountpoint));
    filesystems.dedup_by(|a,b| a.mountpoint == b.mountpoint);
    filesystems
}

fn filesystem_space(mountpoint: &str) -> (Option<u64>, Option<u64>) {
    let Ok(path) = CString::new(mountpoint) else { return (None, None) };
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 { return (None, None); }
    let stats = unsafe { stats.assume_init() };
    let block_size = stats.f_frsize as u64;
    let total = (stats.f_blocks as u64).checked_mul(block_size);
    let available = (stats.f_bavail as u64).checked_mul(block_size);
    (total, available)
}

fn unescape_mount_field(value: &str) -> String {
    value.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_disk_counters_and_rates() {
        let mut previous = std::collections::HashMap::new();
        let now = std::time::Instant::now();
        let first = "8 0 sda 10 0 100 1 20 0 200 2 0 0 0";
        assert_eq!(parse_diskstats(first, &mut previous, now).len(), 1);
        let second = "8 0 sda 12 0 120 1 24 0 240 2 0 0 0";
        let rows = parse_diskstats(second, &mut previous, now + std::time::Duration::from_secs(2));
        assert_eq!(rows[0].read_bytes_per_second, Some(5120.0));
        assert_eq!(rows[0].write_bytes_per_second, Some(10240.0));
    }

    #[test]
    fn rejects_unsafe_device_names() {
        assert!(valid_device_name("nvme0n1"));
        assert!(!valid_device_name("../sda"));
    }
}
