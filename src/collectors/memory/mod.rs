use std::fs;

#[derive(Debug, Clone, Copy, Default)]
pub struct MemorySnapshot {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub buffers_bytes: Option<u64>,
    pub cached_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub swap_free_bytes: Option<u64>,
    pub huge_pages_total: Option<u64>,
    pub huge_pages_free: Option<u64>,
    pub huge_page_size_bytes: Option<u64>,
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
pub struct MemoryPressure {
    pub some: PressureMetrics,
    pub full: Option<PressureMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureMetrics {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total_us: Option<u64>,
}

pub fn sample() -> MemorySnapshot {
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let pressure = fs::read_to_string("/proc/pressure/memory")
        .ok()
        .and_then(|contents| parse_pressure(&contents));
    parse_meminfo(&meminfo, pressure)
}

fn parse_meminfo(contents: &str, pressure: Option<MemoryPressure>) -> MemorySnapshot {
    let kib = |key: &str| -> Option<u64> {
        let mut fields = contents
            .lines()
            .find(|line| line.starts_with(key))?
            .split_whitespace();
        fields.next()?;
        let amount = fields.next()?.parse::<u64>().ok()?;
        if fields.next()? != "kB" {
            return None;
        }
        amount.checked_mul(1024)
    };
    let count = |key: &str| -> Option<u64> {
        contents
            .lines()
            .find(|line| line.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
    };

    MemorySnapshot {
        total_bytes: kib("MemTotal:"),
        available_bytes: kib("MemAvailable:"),
        free_bytes: kib("MemFree:"),
        buffers_bytes: kib("Buffers:"),
        cached_bytes: kib("Cached:"),
        swap_total_bytes: kib("SwapTotal:"),
        swap_free_bytes: kib("SwapFree:"),
        huge_pages_total: count("HugePages_Total:"),
        huge_pages_free: count("HugePages_Free:"),
        huge_page_size_bytes: kib("Hugepagesize:"),
        pressure,
    }
}

fn parse_pressure(contents: &str) -> Option<MemoryPressure> {
    let mut some = None;
    let mut full = None;
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        match fields.next()? {
            "some" => some = parse_pressure_metrics(fields),
            "full" => full = parse_pressure_metrics(fields),
            _ => continue,
        }
    }
    Some(MemoryPressure {
        some: some?,
        full,
    })
}

fn parse_pressure_metrics<'a>(fields: impl Iterator<Item = &'a str>) -> Option<PressureMetrics> {
    let mut avg10 = None;
    let mut avg60 = None;
    let mut avg300 = None;
    let mut total_us = None;
    for field in fields {
        let (key, value) = field.split_once('=')?;
        match key {
            "avg10" => avg10 = parse_percent(value),
            "avg60" => avg60 = parse_percent(value),
            "avg300" => avg300 = parse_percent(value),
            "total" => total_us = value.parse().ok(),
            _ => {}
        }
    }
    Some(PressureMetrics {
        avg10: avg10?,
        avg60: avg60?,
        avg300: avg300?,
        total_us,
    })
}

fn parse_percent(value: &str) -> Option<f64> {
    let percent = value.parse::<f64>().ok()?;
    (percent.is_finite() && (0.0..=100.0).contains(&percent)).then_some(percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_memory_swap_and_hugepage_values() {
        let snapshot = parse_meminfo(
            "MemTotal: 4096 kB\nMemAvailable: 1024 kB\nMemFree: 512 kB\nBuffers: 128 kB\nCached: 256 kB\nSwapTotal: 2048 kB\nSwapFree: 1536 kB\nHugePages_Total: 4\nHugePages_Free: 3\nHugepagesize: 2048 kB\n",
            None,
        );
        assert_eq!(snapshot.total_bytes, Some(4 * 1024 * 1024));
        assert_eq!(snapshot.available_bytes, Some(1024 * 1024));
        assert_eq!(snapshot.used_bytes(), Some(3 * 1024 * 1024));
        assert_eq!(snapshot.free_bytes, Some(512 * 1024));
        assert_eq!(snapshot.buffers_bytes, Some(128 * 1024));
        assert_eq!(snapshot.cached_bytes, Some(256 * 1024));
        assert_eq!(snapshot.swap_used_bytes(), Some(512 * 1024));
        assert_eq!(snapshot.huge_pages_total, Some(4));
        assert_eq!(snapshot.huge_pages_free, Some(3));
        assert_eq!(snapshot.huge_page_size_bytes, Some(2 * 1024 * 1024));
    }

    #[test]
    fn keeps_missing_invalid_and_overflowed_values_unavailable() {
        let snapshot = parse_meminfo(
            "MemTotal: nope kB\nMemAvailable: 1 kB\nHugePages_Total: invalid\nHugepagesize: 18446744073709551615 kB\n",
            None,
        );
        assert_eq!(snapshot.total_bytes, None);
        assert_eq!(snapshot.available_bytes, Some(1024));
        assert_eq!(snapshot.used_bytes(), None);
        assert_eq!(snapshot.huge_pages_total, None);
        assert_eq!(snapshot.huge_page_size_bytes, None);
        assert_eq!(MemorySnapshot::default().swap_used_bytes(), None);
    }

    #[test]
    fn parses_some_and_full_memory_pressure() {
        let pressure = parse_pressure(
            "some avg10=0.25 avg60=0.10 avg300=0.05 total=1234\nfull avg10=0.01 avg60=0.02 avg300=0.03 total=22\n",
        )
        .expect("valid PSI data should parse");
        assert_eq!(pressure.some.avg10, 0.25);
        assert_eq!(pressure.some.total_us, Some(1234));
        assert_eq!(pressure.full.expect("full PSI exists").avg300, 0.03);
    }

    #[test]
    fn rejects_invalid_or_missing_pressure_without_zero_fallback() {
        assert!(parse_pressure("full avg10=1 avg60=1 avg300=1 total=1\n").is_none());
        assert!(parse_pressure("some avg10=NaN avg60=1 avg300=1 total=1\n").is_none());
    }
}
