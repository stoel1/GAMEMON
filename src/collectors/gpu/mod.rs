use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DEVICE_RESCAN_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub struct GpuSnapshot {
    pub devices: Vec<GpuDeviceSnapshot>,
}

#[derive(Debug, Clone)]
pub struct GpuDeviceSnapshot {
    pub drm_card: String,
    pub pci_address: Option<String>,
    pub vendor_id: Option<u16>,
    pub device_id: Option<u16>,
    pub driver: Option<String>,
    pub model: Option<String>,
    pub gpu_busy_percent: Option<u8>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
struct DiscoveredDevice {
    drm_card: String,
    device_path: PathBuf,
}

#[derive(Debug, Default)]
pub struct GpuCollector {
    devices: Option<Vec<DiscoveredDevice>>,
    last_discovery: Option<Instant>,
}

impl GpuCollector {
    pub fn sample(&mut self) -> Option<GpuSnapshot> {
        let now = Instant::now();
        if self
            .last_discovery
            .is_none_or(|last| now.duration_since(last) >= DEVICE_RESCAN_INTERVAL)
        {
            if let Some(devices) = discover_devices() {
                self.devices = Some(devices);
                self.last_discovery = Some(now);
            } else if self.devices.is_none() {
                return None;
            } else {
                self.last_discovery = Some(now);
            }
        }

        let devices = self.devices.as_ref()?;
        Some(GpuSnapshot {
            devices: devices.iter().map(read_device_snapshot).collect(),
        })
    }
}

fn discover_devices() -> Option<Vec<DiscoveredDevice>> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    let mut devices = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !is_drm_card_name(&name) {
            continue;
        }
        let device_path = entry.path().join("device");
        if device_path.exists() {
            devices.push(DiscoveredDevice {
                drm_card: name,
                device_path,
            });
        }
    }
    devices.sort_by(|left, right| left.drm_card.cmp(&right.drm_card));
    Some(devices)
}

fn is_drm_card_name(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()))
}

fn read_device_snapshot(device: &DiscoveredDevice) -> GpuDeviceSnapshot {
    let vendor_id = read_text(&device.device_path.join("vendor")).and_then(|value| parse_pci_id(&value));
    let device_id = read_text(&device.device_path.join("device")).and_then(|value| parse_pci_id(&value));
    let uevent = read_text(&device.device_path.join("uevent"));
    let pci_address = uevent.as_deref().and_then(parse_pci_address);
    let driver = fs::read_link(device.device_path.join("driver"))
        .ok()
        .and_then(|path| path.file_name()?.to_str().map(str::to_owned));
    let model = read_text(&device.device_path.join("product_name"))
        .and_then(|value| valid_display_text(&value, 100));
    let gpu_busy_percent = read_text(&device.device_path.join("gpu_busy_percent"))
        .and_then(|value| parse_percent(&value));
    let vram_total_bytes = read_text(&device.device_path.join("mem_info_vram_total"))
        .and_then(|value| value.parse::<u64>().ok());
    let vram_used_bytes = read_text(&device.device_path.join("mem_info_vram_used"))
        .and_then(|value| value.parse::<u64>().ok());
    let (vram_used_bytes, vram_total_bytes) = validate_vram(vram_used_bytes, vram_total_bytes);

    GpuDeviceSnapshot {
        drm_card: device.drm_card.clone(),
        pci_address,
        vendor_id,
        device_id,
        driver,
        model,
        gpu_busy_percent,
        vram_used_bytes,
        vram_total_bytes,
    }
}

fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn parse_pci_id(value: &str) -> Option<u16> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u16::from_str_radix(value, 16).ok()
}

fn parse_pci_address(uevent: &str) -> Option<String> {
    let address = uevent
        .lines()
        .find_map(|line| line.strip_prefix("PCI_SLOT_NAME="))?;
    (address.len() <= 16
        && address
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || b":.".contains(&byte)))
    .then(|| address.to_owned())
}

fn parse_percent(value: &str) -> Option<u8> {
    let percent = value.parse::<u8>().ok()?;
    (percent <= 100).then_some(percent)
}

fn validate_vram(used: Option<u64>, total: Option<u64>) -> (Option<u64>, Option<u64>) {
    if used.zip(total).is_some_and(|(used, total)| used > total) {
        (None, total)
    } else {
        (used, total)
    }
}

fn valid_display_text(value: &str, max_length: usize) -> Option<String> {
    (!value.is_empty()
        && value.len() <= max_length
        && !value.chars().any(char::is_control))
    .then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_drm_cards_without_matching_connector_names() {
        assert!(is_drm_card_name("card0"));
        assert!(is_drm_card_name("card12"));
        assert!(!is_drm_card_name("card0-DP-1"));
        assert!(!is_drm_card_name("renderD128"));
        assert!(!is_drm_card_name("card"));
    }

    #[test]
    fn parses_pci_ids_and_address_without_guessing_models() {
        assert_eq!(parse_pci_id("0x10de"), Some(0x10de));
        assert_eq!(parse_pci_id("8086"), Some(0x8086));
        assert_eq!(parse_pci_id("0xnot-an-id"), None);
        assert_eq!(
            parse_pci_address("DRIVER=amdgpu\nPCI_SLOT_NAME=0000:01:00.0\n").as_deref(),
            Some("0000:01:00.0")
        );
        assert_eq!(parse_pci_address("PCI_SLOT_NAME=../../etc/passwd\n"), None);
    }

    #[test]
    fn validates_gpu_utilization_vram_and_device_text() {
        assert_eq!(parse_percent("0"), Some(0));
        assert_eq!(parse_percent("100"), Some(100));
        assert_eq!(parse_percent("101"), None);
        assert_eq!(parse_percent("unknown"), None);
        assert_eq!(validate_vram(Some(120), Some(100)), (None, Some(100)));
        assert_eq!(validate_vram(Some(80), Some(100)), (Some(80), Some(100)));
        assert_eq!(valid_display_text("Example GPU", 100).as_deref(), Some("Example GPU"));
        assert_eq!(valid_display_text("bad\nlabel", 100), None);
    }
}
