use std::fs;

pub fn snapshot() -> String {
    let data = match fs::read_to_string("/proc/meminfo") { Ok(data) => data, Err(error) => return format!("Geheugen\nNiet beschikbaar: {error}") };
    let kib = |key: &str| -> Option<u64> { data.lines().find(|line| line.starts_with(key)).and_then(|line| line.split_whitespace().nth(1)).and_then(|v| v.parse().ok()) };
    let total = kib("MemTotal:").unwrap_or(0);
    let available = kib("MemAvailable:").unwrap_or(0);
    let used = total.saturating_sub(available);
    format!("Geheugen\nRAM gebruikt: {:.1} GiB / {:.1} GiB", used as f64 / 1_048_576.0, total as f64 / 1_048_576.0)
}
