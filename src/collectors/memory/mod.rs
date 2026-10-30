use std::fs;

#[derive(Debug, Clone, Copy)]
pub struct MemorySnapshot {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

impl MemorySnapshot {
    pub fn used_bytes(self) -> Option<u64> {
        Some(self.total_bytes?.saturating_sub(self.available_bytes?))
    }
}

pub fn sample() -> MemorySnapshot {
    let contents = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    parse_meminfo(&contents)
}

fn parse_meminfo(contents: &str) -> MemorySnapshot {
    let kib = |key: &str| -> Option<u64> {
        contents
            .lines()
            .find(|line| line.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
    };
    let to_bytes = |value: Option<u64>| value.and_then(|value| value.checked_mul(1024));

    MemorySnapshot {
        total_bytes: to_bytes(kib("MemTotal:")),
        available_bytes: to_bytes(kib("MemAvailable:")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_total_and_available_memory() {
        let snapshot = parse_meminfo("MemTotal:       4096 kB\nMemAvailable:   1024 kB\n");
        assert_eq!(snapshot.total_bytes, Some(4 * 1024 * 1024));
        assert_eq!(snapshot.available_bytes, Some(1024 * 1024));
        assert_eq!(snapshot.used_bytes(), Some(3 * 1024 * 1024));
    }

    #[test]
    fn keeps_missing_or_invalid_values_unavailable() {
        let snapshot = parse_meminfo("MemTotal: nope kB\n");
        assert_eq!(snapshot.total_bytes, None);
        assert_eq!(snapshot.available_bytes, None);
        assert_eq!(snapshot.used_bytes(), None);
    }
}
