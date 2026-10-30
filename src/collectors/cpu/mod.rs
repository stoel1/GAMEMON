use std::{collections::HashMap, fs};

const MAX_LOGICAL_CPUS: u32 = 4096;

#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub model: Option<String>,
    pub online: Option<String>,
    pub utilization: CpuUtilization,
    pub logical_cpus: Vec<LogicalCpuSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CpuUtilization {
    Available(f64),
    WarmingUp,
    Unavailable,
}

#[derive(Debug, Clone, Copy)]
pub struct LogicalCpuSnapshot {
    pub id: u32,
    pub utilization: CpuUtilization,
    pub package_id: Option<u32>,
    pub core_id: Option<u32>,
    pub current_frequency_hz: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuCounters {
    total: u64,
    idle: u64,
}

#[derive(Debug, Default)]
struct CpuStatSnapshot {
    aggregate: Option<CpuCounters>,
    logical_cpus: HashMap<u32, CpuCounters>,
}

#[derive(Debug, Clone, Copy, Default)]
struct CpuTopology {
    package_id: Option<u32>,
    core_id: Option<u32>,
}

#[derive(Debug, Default)]
pub struct CpuCollector {
    previous_aggregate: Option<CpuCounters>,
    previous_logical: HashMap<u32, CpuCounters>,
    topology: HashMap<u32, CpuTopology>,
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
        let online_ids = online.as_deref().and_then(parse_cpu_list);
        let stats = fs::read_to_string("/proc/stat")
            .map(|contents| parse_cpu_stat(&contents))
            .unwrap_or_default();

        let utilization = stats
            .aggregate
            .map(|current| {
                let utilization = self
                    .previous_aggregate
                    .and_then(|previous| calculate_utilization(previous, current));
                self.previous_aggregate = Some(current);
                utilization
                    .map(CpuUtilization::Available)
                    .unwrap_or(CpuUtilization::WarmingUp)
            })
            .unwrap_or(CpuUtilization::Unavailable);

        let mut logical_cpus = Vec::new();
        if let Some(ids) = online_ids {
            self.previous_logical
                .retain(|cpu_id, _| ids.binary_search(cpu_id).is_ok());
            for id in ids {
                let topology = self.topology.entry(id).or_default();
                if topology.package_id.is_none() || topology.core_id.is_none() {
                    let discovered = read_topology(id);
                    topology.package_id = topology.package_id.or(discovered.package_id);
                    topology.core_id = topology.core_id.or(discovered.core_id);
                }
                let topology = *topology;
                let cpu_utilization = match stats.logical_cpus.get(&id).copied() {
                    Some(current) => {
                        let value = self
                            .previous_logical
                            .get(&id)
                            .and_then(|previous| calculate_utilization(*previous, current));
                        self.previous_logical.insert(id, current);
                        value
                            .map(CpuUtilization::Available)
                            .unwrap_or(CpuUtilization::WarmingUp)
                    }
                    None => CpuUtilization::Unavailable,
                };
                logical_cpus.push(LogicalCpuSnapshot {
                    id,
                    utilization: cpu_utilization,
                    package_id: topology.package_id,
                    core_id: topology.core_id,
                    current_frequency_hz: read_current_frequency(id),
                });
            }
        } else {
            self.previous_logical.clear();
        }

        CpuSnapshot {
            model,
            online,
            utilization,
            logical_cpus,
        }
    }
}

fn read_topology(cpu_id: u32) -> CpuTopology {
    let path = format!("/sys/devices/system/cpu/cpu{cpu_id}/topology");
    CpuTopology {
        package_id: fs::read_to_string(format!("{path}/physical_package_id"))
            .ok()
            .and_then(|value| value.trim().parse().ok()),
        core_id: fs::read_to_string(format!("{path}/core_id"))
            .ok()
            .and_then(|value| value.trim().parse().ok()),
    }
}

fn read_current_frequency(cpu_id: u32) -> Option<u64> {
    fs::read_to_string(format!(
        "/sys/devices/system/cpu/cpu{cpu_id}/cpufreq/scaling_cur_freq"
    ))
    .ok()
    .and_then(|value| parse_frequency_hz(value.trim()))
}

fn parse_frequency_hz(value: &str) -> Option<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|frequency| *frequency > 0)?
        .checked_mul(1000)
}

fn calculate_utilization(previous: CpuCounters, current: CpuCounters) -> Option<f64> {
    let total_delta = current.total.checked_sub(previous.total)?;
    let idle_delta = current.idle.checked_sub(previous.idle)?;
    let busy_delta = total_delta.checked_sub(idle_delta)?;
    (total_delta > 0).then(|| (busy_delta as f64 / total_delta as f64) * 100.0)
}

fn parse_model(contents: &str) -> Option<String> {
    contents
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name").then_some(value.trim())
        })
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
}

fn parse_cpu_stat(contents: &str) -> CpuStatSnapshot {
    let mut snapshot = CpuStatSnapshot::default();
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(label) = fields.next() else {
            continue;
        };
        let counters = match parse_counter_fields(fields) {
            Some(counters) => counters,
            None => continue,
        };
        if label == "cpu" {
            snapshot.aggregate = Some(counters);
        } else if let Some(id) = label.strip_prefix("cpu").and_then(|id| id.parse().ok()) {
            snapshot.logical_cpus.insert(id, counters);
        }
    }
    snapshot
}

fn parse_counter_fields<'a>(fields: impl Iterator<Item = &'a str>) -> Option<CpuCounters> {
    let values = fields
        .take(8)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 4 {
        return None;
    }
    let total = values
        .iter()
        .try_fold(0_u64, |sum, value| sum.checked_add(*value))?;
    let idle = values[3].checked_add(*values.get(4).unwrap_or(&0))?;
    Some(CpuCounters { total, idle })
}

fn parse_cpu_list(contents: &str) -> Option<Vec<u32>> {
    let mut ids = Vec::new();
    for item in contents.trim().split(',') {
        let (start, end) = match item.split_once('-') {
            Some((start, end)) => (start.parse::<u32>().ok()?, end.parse::<u32>().ok()?),
            None => {
                let id = item.parse::<u32>().ok()?;
                (id, id)
            }
        };
        if start > end || end >= MAX_LOGICAL_CPUS {
            return None;
        }
        let count = end.checked_sub(start)?.checked_add(1)?;
        if ids.len().checked_add(count as usize)? > MAX_LOGICAL_CPUS as usize {
            return None;
        }
        ids.extend(start..=end);
    }
    let parsed_count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != parsed_count {
        return None;
    }
    (!ids.is_empty()).then_some(ids)
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
    fn parses_aggregate_and_per_cpu_counters() {
        let snapshot = parse_cpu_stat(
            "cpu  100 20 30 400 5 6 7 8\ncpu0 50 10 15 200 2 3 3 4\ncpu2 50 10 15 200 3 3 4 4\n",
        );
        assert_eq!(snapshot.aggregate.map(|value| value.total), Some(576));
        assert_eq!(snapshot.logical_cpus.len(), 2);
        assert_eq!(snapshot.logical_cpus.get(&0).map(|value| value.idle), Some(202));
        assert_eq!(snapshot.logical_cpus.get(&2).map(|value| value.total), Some(289));
    }

    #[test]
    fn ignores_malformed_cpu_rows_without_fabricating_values() {
        let snapshot = parse_cpu_stat("cpu bad 2 3 4\ncpu0 1 2 3\n");
        assert!(snapshot.aggregate.is_none());
        assert!(snapshot.logical_cpus.is_empty());
    }

    #[test]
    fn rejects_counter_sum_overflow() {
        let snapshot = parse_cpu_stat("cpu 18446744073709551615 1 0 0\n");
        assert!(snapshot.aggregate.is_none());
    }

    #[test]
    fn parses_sysfs_frequency_in_khz_without_guessing_invalid_values() {
        assert_eq!(parse_frequency_hz("3600000"), Some(3_600_000_000));
        assert_eq!(parse_frequency_hz("0"), None);
        assert_eq!(parse_frequency_hz("not-a-frequency"), None);
        assert_eq!(parse_frequency_hz("18446744073709551615"), None);
    }

    #[test]
    fn calculates_utilization_from_counter_deltas() {
        let previous = CpuCounters { total: 100, idle: 40 };
        let current = CpuCounters { total: 200, idle: 90 };
        assert_eq!(calculate_utilization(previous, current), Some(50.0));
        assert_eq!(calculate_utilization(current, previous), None);
        assert_eq!(calculate_utilization(previous, previous), None);
        assert_eq!(
            calculate_utilization(
                CpuCounters { total: 100, idle: 40 },
                CpuCounters { total: 200, idle: 150 }
            ),
            None
        );
    }

    #[test]
    fn parses_online_cpu_ranges_and_rejects_invalid_ranges() {
        assert_eq!(parse_cpu_list("0-3,5,7-8"), Some(vec![0, 1, 2, 3, 5, 7, 8]));
        assert!(parse_cpu_list("0-2,2").is_none());
        assert!(parse_cpu_list("3-1").is_none());
        assert!(parse_cpu_list("0-4096").is_none());
        assert!(parse_cpu_list("").is_none());
    }
}
