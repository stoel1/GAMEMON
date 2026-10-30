use std::fs;

#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub model: Option<String>,
    pub online: Option<String>,
    pub utilization: CpuUtilization,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CpuUtilization {
    Available(f64),
    WarmingUp,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuCounters {
    total: u64,
    idle: u64,
}

#[derive(Debug, Default)]
pub struct CpuCollector {
    previous: Option<CpuCounters>,
}

impl CpuCollector {
    pub fn sample(&mut self) -> CpuSnapshot {
        let model = fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|contents| parse_model(&contents));
        let online = fs::read_to_string("/sys/devices/system/cpu/online")
            .ok()
            .map(|contents| contents.trim().to_owned())
            .filter(|contents| !contents.is_empty());
        let utilization = match fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|contents| parse_cpu_counters(&contents))
        {
            Some(current) => {
                let utilization = self
                    .previous
                    .and_then(|previous| calculate_utilization(previous, current));
                self.previous = Some(current);
                utilization
                    .map(CpuUtilization::Available)
                    .unwrap_or(CpuUtilization::WarmingUp)
            }
            None => CpuUtilization::Unavailable,
        };

        CpuSnapshot {
            model,
            online,
            utilization,
        }
    }
}

fn calculate_utilization(previous: CpuCounters, current: CpuCounters) -> Option<f64> {
    let total_delta = current.total.checked_sub(previous.total)?;
    let idle_delta = current.idle.checked_sub(previous.idle)?;
    (total_delta > 0).then(|| {
        (total_delta.saturating_sub(idle_delta) as f64 / total_delta as f64) * 100.0
    })
}

fn parse_model(contents: &str) -> Option<String> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix("model name\t:").or_else(|| line.strip_prefix("model name :")))
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
}

fn parse_cpu_counters(contents: &str) -> Option<CpuCounters> {
    let mut fields = contents.lines().find(|line| line.starts_with("cpu "))?.split_whitespace();
    fields.next()?;
    let values = fields
        .take(8)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 4 {
        return None;
    }

    let total = values.iter().try_fold(0_u64, |sum, value| sum.checked_add(*value))?;
    let idle = values[3].checked_add(*values.get(4).unwrap_or(&0))?;
    Some(CpuCounters { total, idle })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_name() {
        assert_eq!(
            parse_model("processor : 0\nmodel name\t: Example CPU 9000\n"),
            Some("Example CPU 9000".to_owned())
        );
    }

    #[test]
    fn parses_aggregate_cpu_counters() {
        let counters = parse_cpu_counters("cpu  100 20 30 400 5 6 7 8\ncpu0 10 2 3 40 0 0 0 0\n")
            .expect("valid aggregate CPU line should parse");
        assert_eq!(counters.total, 576);
        assert_eq!(counters.idle, 405);
    }

    #[test]
    fn rejects_incomplete_or_invalid_cpu_counters() {
        assert!(parse_cpu_counters("cpu 1 2 3\n").is_none());
        assert!(parse_cpu_counters("cpu 1 2 nope 4\n").is_none());
    }

    #[test]
    fn calculates_utilization_from_counter_deltas() {
        let previous = CpuCounters { total: 100, idle: 40 };
        let current = CpuCounters { total: 200, idle: 90 };
        assert_eq!(calculate_utilization(previous, current), Some(50.0));
        assert_eq!(calculate_utilization(current, previous), None);
        assert_eq!(calculate_utilization(previous, previous), None);
    }
}
