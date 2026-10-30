use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DOMAIN_RESCAN_INTERVAL: Duration = Duration::from_secs(5);
const MAX_DOMAIN_DEPTH: usize = 4;

#[derive(Debug, Clone)]
pub struct PowerDomainSnapshot {
    pub name: String,
    pub power_watts: Option<f64>,
}

#[derive(Debug, Clone)]
struct PowerDomain {
    path: PathBuf,
    name: String,
    max_energy_range_uj: u64,
}

#[derive(Debug, Clone, Copy)]
struct PreviousEnergy {
    energy_uj: u64,
    sampled_at: Instant,
}

#[derive(Debug, Default)]
pub struct RaplCollector {
    domains: Option<Vec<PowerDomain>>,
    last_discovery: Option<Instant>,
    previous: HashMap<PathBuf, PreviousEnergy>,
}

impl RaplCollector {
    pub fn sample(&mut self) -> Vec<PowerDomainSnapshot> {
        let now = Instant::now();
        if self
            .last_discovery
            .is_none_or(|last| now.duration_since(last) >= DOMAIN_RESCAN_INTERVAL)
        {
            if let Some(domains) = discover_domains(Path::new("/sys/class/powercap")) {
                self.previous
                    .retain(|path, _| domains.iter().any(|domain| &domain.path == path));
                self.domains = Some(domains);
            }
            self.last_discovery = Some(now);
        }

        self.domains
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|domain| {
                let energy = read_u64(&domain.path.join("energy_uj"));
                let power_watts = energy.and_then(|current| {
                    let previous = self.previous.insert(
                        domain.path.clone(),
                        PreviousEnergy {
                            energy_uj: current,
                            sampled_at: now,
                        },
                    );
                    previous.and_then(|previous| {
                        calculate_power_watts(
                            previous.energy_uj,
                            current,
                            domain.max_energy_range_uj,
                            now.saturating_duration_since(previous.sampled_at),
                        )
                    })
                });
                PowerDomainSnapshot {
                    name: domain.name.clone(),
                    power_watts,
                }
            })
            .collect()
    }
}

fn discover_domains(root: &Path) -> Option<Vec<PowerDomain>> {
    let mut paths = Vec::new();
    collect_directories(root, 0, &mut paths);
    let mut domains = paths
        .into_iter()
        .filter_map(|path| {
            let name = fs::read_to_string(path.join("name")).ok()?;
            let name = name.trim();
            if !name.starts_with("package-")
                || name.len() > 64
                || name.chars().any(char::is_control)
            {
                return None;
            }
            let max_energy_range_uj = read_u64(&path.join("max_energy_range_uj"))?;
            (max_energy_range_uj > 0).then(|| PowerDomain {
                path,
                name: name.to_owned(),
                max_energy_range_uj,
            })
        })
        .collect::<Vec<_>>();
    domains.sort_by(|left, right| left.name.cmp(&right.name));
    Some(domains)
}

fn collect_directories(path: &Path, depth: usize, output: &mut Vec<PathBuf>) {
    if depth >= MAX_DOMAIN_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            output.push(child.clone());
            collect_directories(&child, depth + 1, output);
        }
    }
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn calculate_power_watts(
    previous_uj: u64,
    current_uj: u64,
    max_range_uj: u64,
    elapsed: Duration,
) -> Option<f64> {
    if max_range_uj == 0 || previous_uj >= max_range_uj || current_uj >= max_range_uj {
        return None;
    }
    let energy_delta_uj = if current_uj >= previous_uj {
        current_uj - previous_uj
    } else {
        max_range_uj
            .checked_sub(previous_uj)?
            .checked_add(current_uj)?
    };
    let seconds = elapsed.as_secs_f64();
    (seconds > 0.0).then(|| energy_delta_uj as f64 / 1_000_000.0 / seconds)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::Path,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use super::{calculate_power_watts, discover_domains};

    #[test]
    fn calculates_power_from_energy_delta_and_elapsed_time() {
        assert_eq!(
            calculate_power_watts(10_000_000, 20_000_000, 100_000_000, Duration::from_secs(2)),
            Some(5.0)
        );
    }

    #[test]
    fn handles_counter_wrap_and_rejects_bad_samples() {
        assert_eq!(
            calculate_power_watts(90, 10, 100, Duration::from_secs(2)),
            Some(0.00001)
        );
        assert_eq!(
            calculate_power_watts(10, 5, 0, Duration::from_secs(1)),
            None
        );
        assert_eq!(
            calculate_power_watts(5, 5, 100, Duration::ZERO),
            None
        );
        assert_eq!(
            calculate_power_watts(100, 1, 100, Duration::from_secs(1)),
            None
        );
    }

    #[test]
    fn discovers_named_package_domains_without_fixed_device_numbers() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("gamemon-rapl-{}-{unique}", std::process::id()));
        let package = root.join("vendor-domain:7");
        let core = package.join("vendor-domain:7:0");
        fs::create_dir_all(&core).unwrap();
        fs::write(package.join("name"), "package-7\n").unwrap();
        fs::write(package.join("max_energy_range_uj"), "1000000\n").unwrap();
        fs::write(core.join("name"), "core\n").unwrap();
        fs::write(core.join("max_energy_range_uj"), "500000\n").unwrap();

        let domains = discover_domains(Path::new(&root)).unwrap();
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0].name, "package-7");
        assert_eq!(domains[0].path, package);

        fs::remove_dir_all(root).unwrap();
    }
}
