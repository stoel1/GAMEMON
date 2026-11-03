use std::{fs, path::Path};

#[derive(Debug, Clone, Default)]
pub struct SystemSnapshot {
    pub uptime_seconds: Option<u64>,
    pub load_average: Option<[f64; 3]>,
    pub temperatures: Vec<TemperatureReading>,
    pub fans: Vec<FanReading>,
}

#[derive(Debug, Clone)]
pub struct TemperatureReading {
    pub label: String,
    pub celsius: f64,
}

#[derive(Debug, Clone)]
pub struct FanReading {
    pub label: String,
    pub rpm: u64,
}

pub fn sample() -> SystemSnapshot {
    SystemSnapshot {
        uptime_seconds: fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|value| parse_uptime(&value)),
        load_average: fs::read_to_string("/proc/loadavg")
            .ok()
            .and_then(|value| parse_load_average(&value)),
        temperatures: read_temperatures(Path::new("/sys/class/hwmon")),
        fans: read_fans(Path::new("/sys/class/hwmon")),
    }
}

fn parse_uptime(contents: &str) -> Option<u64> {
    let seconds = contents.split_whitespace().next()?.parse::<f64>().ok()?;
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64)
}

fn parse_load_average(contents: &str) -> Option<[f64; 3]> {
    let mut fields = contents.split_whitespace();
    let values = [
        fields.next()?.parse::<f64>().ok()?,
        fields.next()?.parse::<f64>().ok()?,
        fields.next()?.parse::<f64>().ok()?,
    ];
    values.iter().all(|value| value.is_finite() && *value >= 0.0).then_some(values)
}

fn read_temperatures(root: &Path) -> Vec<TemperatureReading> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut readings = Vec::new();
    for entry in entries.flatten() {
        let hwmon = entry.path();
        let chip = read_trimmed(&hwmon.join("name")).unwrap_or_else(|| "hwmon".to_owned());
        let Ok(files) = fs::read_dir(&hwmon) else { continue };
        for file in files.flatten() {
            let filename = file.file_name().to_string_lossy().into_owned();
            let Some(index) = filename.strip_prefix("temp").and_then(|name| name.strip_suffix("_input")) else { continue };
            if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) { continue; }
            let Some(raw) = read_i64(&file.path()) else { continue };
            let celsius = raw as f64 / 1000.0;
            if !celsius.is_finite() || !(-100.0..=250.0).contains(&celsius) { continue; }
            let label = read_trimmed(&hwmon.join(format!("temp{index}_label")))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("{chip} temp{index}"));
            readings.push(TemperatureReading { label, celsius });
        }
    }
    readings.sort_by(|left, right| left.label.cmp(&right.label));
    readings
}

fn read_fans(root: &Path) -> Vec<FanReading> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut readings = Vec::new();
    for entry in entries.flatten() {
        let hwmon = entry.path();
        let chip = read_trimmed(&hwmon.join("name")).unwrap_or_else(|| "hwmon".to_owned());
        let Ok(files) = fs::read_dir(&hwmon) else { continue };
        for file in files.flatten() {
            let filename = file.file_name().to_string_lossy().into_owned();
            let Some(index) = filename.strip_prefix("fan").and_then(|name| name.strip_suffix("_input")) else { continue };
            if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) { continue; }
            let Some(rpm) = read_u64(&file.path()) else { continue };
            let label = read_trimmed(&hwmon.join(format!("fan{index}_label")))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("{chip} fan{index}"));
            readings.push(FanReading { label, rpm });
        }
    }
    readings.sort_by(|left, right| left.label.cmp(&right.label));
    readings
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|value| value.trim().to_owned())
}

fn read_i64(path: &Path) -> Option<i64> {
    read_trimmed(path)?.parse().ok()
}

fn read_u64(path: &Path) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_load_average, parse_uptime};

    #[test]
    fn parses_procfs_uptime_and_load_average() {
        assert_eq!(parse_uptime("12345.67 900.0"), Some(12345));
        assert_eq!(parse_load_average("1.25 0.75 0.50 2/100 1234"), Some([1.25, 0.75, 0.5]));
    }

    #[test]
    fn rejects_invalid_procfs_values() {
        assert_eq!(parse_uptime("-1.0 0"), None);
        assert_eq!(parse_uptime("NaN 0"), None);
        assert_eq!(parse_load_average("NaN 0.5 0.2"), None);
        assert_eq!(parse_load_average("1.0 0.5"), None);
    }
}
