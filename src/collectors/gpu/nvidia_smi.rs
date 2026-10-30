use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::{normalize_pci_address, nvml::DeviceMetrics};

const QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const QUERY_FIELDS: &str = "uuid,pci.bus_id,name,utilization.gpu,utilization.memory,memory.used,memory.total,temperature.gpu,power.draw,power.limit,clocks.gr,clocks.mem,fan.speed,utilization.encoder,utilization.decoder";
const MIB_BYTES: u64 = 1_048_576;

pub fn sample() -> Option<Vec<DeviceMetrics>> {
    let mut child = Command::new("nvidia-smi")
        .args([
            format!("--query-gpu={QUERY_FIELDS}"),
            "--format=csv,noheader,nounits".to_owned(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let output = child.wait_with_output().ok()?;
                return parse_output(&String::from_utf8(output.stdout).ok()?);
            }
            Ok(None) if started.elapsed() < QUERY_TIMEOUT => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn parse_output(output: &str) -> Option<Vec<DeviceMetrics>> {
    let rows = parse_csv(output)?;
    if rows.is_empty() {
        return None;
    }
    let devices = rows
        .into_iter()
        .filter_map(|row| {
            if row.len() != 15 {
                return None;
            }
            let pci_address = normalize_pci_address(&row[1])?;
            Some(DeviceMetrics {
                source: Some("nvidia-smi"),
                uuid: valid_text(&row[0], 100),
                pci_address: Some(pci_address),
                name: valid_text(&row[2], 100),
                gpu_busy_percent: parse_percent(&row[3]),
                memory_busy_percent: parse_percent(&row[4]),
                vram_used_bytes: parse_mib(&row[5]),
                vram_total_bytes: parse_mib(&row[6]).filter(|value| *value > 0),
                temperature_celsius: parse_u32(&row[7]),
                power_milliwatts: parse_watts(&row[8]),
                power_limit_milliwatts: parse_watts(&row[9]),
                graphics_clock_mhz: parse_u32(&row[10]),
                memory_clock_mhz: parse_u32(&row[11]),
                fan_percent: parse_percent(&row[12]),
                encoder_busy_percent: parse_percent(&row[13]),
                decoder_busy_percent: parse_percent(&row[14]),
            })
        })
        .collect::<Vec<_>>();
    (!devices.is_empty()).then_some(devices)
}

fn parse_csv(input: &str) -> Option<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if !quoted && field.trim().is_empty() => {
                field.clear();
                quoted = true;
            }
            '"' if quoted => quoted = false,
            ',' if !quoted => {
                row.push(field.trim().to_owned());
                field.clear();
            }
            '\n' | '\r' if !quoted => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(field.trim().to_owned());
                field.clear();
                if row.iter().any(|value| !value.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(ch),
        }
    }
    if quoted {
        return None;
    }
    row.push(field.trim().to_owned());
    if row.iter().any(|value| !value.is_empty()) {
        rows.push(row);
    }
    Some(rows)
}

fn valid_text(value: &str, max_length: usize) -> Option<String> {
    (!value.is_empty() && value.len() <= max_length && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn parse_percent(value: &str) -> Option<u8> {
    let value = parse_u32(value)?;
    u8::try_from(value).ok().filter(|value| *value <= 100)
}

fn parse_u32(value: &str) -> Option<u32> {
    value.trim().parse().ok()
}

fn parse_mib(value: &str) -> Option<u64> {
    parse_u64(value)?.checked_mul(MIB_BYTES)
}

fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

fn parse_watts(value: &str) -> Option<u32> {
    let milliwatts = value.trim().parse::<f64>().ok()? * 1000.0;
    (milliwatts.is_finite() && (0.0..=u32::MAX as f64).contains(&milliwatts))
        .then(|| milliwatts.round() as u32)
}

#[cfg(test)]
mod tests {
    use super::{MIB_BYTES, parse_output};

    #[test]
    fn parses_nvidia_smi_csv_and_keeps_unavailable_metrics_empty() {
        let output = concat!(
            "GPU-abc, 00000000:01:00.0, \"NVIDIA Example, 12GB\", 23, 0, 1200, 12288, 42, 95.5, 250, 2100, 10501, N/A, 0, [Not Supported]\n",
            "GPU-def, 00000000:02:00.0, Other GPU, N/A, N/A, N/A, N/A, N/A, N/A, N/A, N/A, N/A, N/A, N/A, N/A\n",
        );

        let devices = parse_output(output).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].source, Some("nvidia-smi"));
        assert_eq!(devices[0].name.as_deref(), Some("NVIDIA Example, 12GB"));
        assert_eq!(devices[0].pci_address.as_deref(), Some("0000:01:00.0"));
        assert_eq!(devices[0].gpu_busy_percent, Some(23));
        assert_eq!(devices[0].vram_used_bytes, Some(1200 * MIB_BYTES));
        assert_eq!(devices[0].temperature_celsius, Some(42));
        assert_eq!(devices[0].power_milliwatts, Some(95_500));
        assert_eq!(devices[0].fan_percent, None);
        assert_eq!(devices[0].decoder_busy_percent, None);
        assert_eq!(devices[1].gpu_busy_percent, None);
        assert_eq!(devices[1].vram_total_bytes, None);
    }

    #[test]
    fn rejects_malformed_rows_and_numeric_overflow() {
        assert!(parse_output("one, two, three\n").is_none());
        let output = "GPU, 0000:01:00.0, Name, 101, 0, 18446744073709551615, 12288, 45, 10, 20, 1, 1, 0, 0, 0";
        let device = &parse_output(output).unwrap()[0];
        assert_eq!(device.gpu_busy_percent, None);
        assert_eq!(device.vram_used_bytes, None);
    }
}
