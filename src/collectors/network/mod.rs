use std::{
    collections::HashMap,
    fs,
    time::{Duration, Instant},
};

const INTERFACE_RESCAN_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct NetworkSnapshot {
    pub interfaces: Vec<NetworkInterfaceSnapshot>,
}

#[derive(Debug, Clone)]
pub struct NetworkInterfaceSnapshot {
    pub name: String,
    pub operational_state: Option<String>,
    pub carrier: Option<bool>,
    pub speed_mbps: Option<u64>,
    pub duplex: Option<String>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    pub rx_bytes_per_second: Option<f64>,
    pub tx_bytes_per_second: Option<f64>,
    pub rx_errors: Option<u64>,
    pub tx_errors: Option<u64>,
    pub rx_dropped: Option<u64>,
    pub tx_dropped: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct CounterBaseline {
    rx_bytes: Option<u64>,
    tx_bytes: Option<u64>,
    sampled_at: Instant,
}

#[derive(Debug, Default)]
pub struct NetworkCollector {
    interface_names: Option<Vec<String>>,
    last_discovery: Option<Instant>,
    previous: HashMap<String, CounterBaseline>,
}

impl NetworkCollector {
    pub fn sample(&mut self) -> Option<NetworkSnapshot> {
        let now = Instant::now();
        if self
            .last_discovery
            .is_none_or(|last| now.duration_since(last) >= INTERFACE_RESCAN_INTERVAL)
        {
            if let Some(names) = discover_interfaces() {
                self.interface_names = Some(names);
                self.last_discovery = Some(now);
                let names = self.interface_names.as_ref()?;
                self.previous
                    .retain(|name, _| names.binary_search(name).is_ok());
            } else if self.interface_names.is_none() {
                return None;
            } else {
                self.last_discovery = Some(now);
            }
        }

        let names = self.interface_names.as_ref()?;
        let mut interfaces = Vec::with_capacity(names.len());
        for name in names {
            let root = format!("/sys/class/net/{name}");
            let read_counter = |counter: &str| {
                fs::read_to_string(format!("{root}/statistics/{counter}"))
                    .ok()
                    .and_then(|value| value.trim().parse::<u64>().ok())
            };
            let rx_bytes = read_counter("rx_bytes");
            let tx_bytes = read_counter("tx_bytes");
            let baseline = self.previous.get(name).copied();
            let (rx_bytes_per_second, tx_bytes_per_second) = match baseline {
                Some(previous) => {
                    let elapsed = now.duration_since(previous.sampled_at);
                    (
                        counter_rate(previous.rx_bytes, rx_bytes, elapsed),
                        counter_rate(previous.tx_bytes, tx_bytes, elapsed),
                    )
                }
                None => (None, None),
            };
            self.previous.insert(
                name.clone(),
                CounterBaseline {
                    rx_bytes,
                    tx_bytes,
                    sampled_at: now,
                },
            );

            interfaces.push(NetworkInterfaceSnapshot {
                name: name.clone(),
                operational_state: read_attribute(&root, "operstate").and_then(parse_operstate),
                carrier: read_attribute(&root, "carrier").and_then(parse_carrier),
                speed_mbps: read_attribute(&root, "speed").and_then(parse_speed_mbps),
                duplex: read_attribute(&root, "duplex").and_then(parse_duplex),
                rx_bytes,
                tx_bytes,
                rx_bytes_per_second,
                tx_bytes_per_second,
                rx_errors: read_counter("rx_errors"),
                tx_errors: read_counter("tx_errors"),
                rx_dropped: read_counter("rx_dropped"),
                tx_dropped: read_counter("tx_dropped"),
            });
        }
        Some(NetworkSnapshot { interfaces })
    }
}

fn discover_interfaces() -> Option<Vec<String>> {
    let entries = fs::read_dir("/sys/class/net").ok()?;
    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if valid_interface_name(&name) {
            names.push(name);
        }
    }
    names.sort_unstable();
    Some(names)
}

fn valid_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 15
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
}

fn read_attribute(root: &str, name: &str) -> Option<String> {
    fs::read_to_string(format!("{root}/{name}"))
        .ok()
        .map(|value| value.trim().to_owned())
}

fn parse_operstate(value: String) -> Option<String> {
    matches!(
        value.as_str(),
        "up" | "down" | "unknown" | "dormant" | "notpresent" | "lowerlayerdown" | "testing"
    )
    .then_some(value)
}

fn parse_carrier(value: String) -> Option<bool> {
    match value.as_str() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn parse_speed_mbps(value: String) -> Option<u64> {
    value.parse::<u64>().ok().filter(|speed| *speed > 0)
}

fn parse_duplex(value: String) -> Option<String> {
    matches!(value.as_str(), "full" | "half" | "unknown").then_some(value)
}

fn counter_rate(previous: Option<u64>, current: Option<u64>, elapsed: Duration) -> Option<f64> {
    let delta = current?.checked_sub(previous?)?;
    let seconds = elapsed.as_secs_f64();
    (seconds > 0.0).then_some(delta as f64 / seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_rate_from_counter_delta_and_elapsed_time() {
        assert_eq!(
            counter_rate(Some(100), Some(250), Duration::from_secs(3)),
            Some(50.0)
        );
        assert_eq!(
            counter_rate(Some(250), Some(250), Duration::from_secs(1)),
            Some(0.0)
        );
    }

    #[test]
    fn rejects_missing_reset_and_zero_interval_counters() {
        assert_eq!(counter_rate(None, Some(50), Duration::from_secs(1)), None);
        assert_eq!(counter_rate(Some(60), Some(50), Duration::from_secs(1)), None);
        assert_eq!(counter_rate(Some(1), Some(2), Duration::ZERO), None);
    }

    #[test]
    fn validates_link_attributes_without_fabricating_unknown_values() {
        assert_eq!(parse_operstate("up".to_owned()).as_deref(), Some("up"));
        assert_eq!(parse_operstate("up\ninjected".to_owned()), None);
        assert_eq!(parse_carrier("1".to_owned()), Some(true));
        assert_eq!(parse_carrier("2".to_owned()), None);
        assert_eq!(parse_speed_mbps("10000".to_owned()), Some(10_000));
        assert_eq!(parse_speed_mbps("-1".to_owned()), None);
        assert_eq!(parse_duplex("full".to_owned()).as_deref(), Some("full"));
        assert_eq!(parse_duplex("mystery".to_owned()), None);
    }

    #[test]
    fn accepts_kernel_interface_names_and_rejects_path_input() {
        assert!(valid_interface_name("enp5s0"));
        assert!(valid_interface_name("wlan0"));
        assert!(!valid_interface_name("../proc"));
        assert!(!valid_interface_name(""));
    }
}
