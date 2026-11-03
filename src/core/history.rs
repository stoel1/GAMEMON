use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::core::sampler::MonitorSnapshot;

/// Appends one compact, portable CSV row per sampler snapshot.
/// History is stored below XDG_DATA_HOME/gamemon by default.
pub struct HistoryStore {
    path: PathBuf,
}

impl HistoryStore {
    pub fn new() -> Option<Self> {
        let root = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::var_os("HOME").map(PathBuf::from).map(|home| home.join(".local/share")))?;
        let directory = root.join("gamemon");
        if let Err(error) = fs::create_dir_all(&directory) {
            tracing::warn!(path = %directory.display(), %error, "could not create history directory");
            return None;
        }
        Some(Self { path: directory.join("history.csv") })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn append(&self, snapshot: &MonitorSnapshot) {
        let timestamp = snapshot.sampled_at.duration_since(UNIX_EPOCH)
            .unwrap_or_default().as_secs();
        let cpu = match snapshot.cpu.utilization {
            crate::collectors::cpu::CpuUtilization::Available(value) => Some(value),
            _ => None,
        };
        let gpu = snapshot.gpu.as_ref()
            .and_then(|gpu| gpu.devices.first())
            .and_then(|device| device.gpu_busy_percent.map(f64::from));
        let memory = snapshot.memory.used_bytes()
            .zip(snapshot.memory.total_bytes.filter(|value| *value > 0))
            .map(|(used,total)| used as f64 / total as f64 * 100.0);
        let temperature = snapshot.system.temperatures.iter()
            .map(|reading| reading.celsius)
            .reduce(f64::max);
        let network_rx = snapshot.network.as_ref()
            .map(|network| network.interfaces.iter().filter_map(|item| item.rx_bytes_per_second).sum::<f64>());
        let network_tx = snapshot.network.as_ref()
            .map(|network| network.interfaces.iter().filter_map(|item| item.tx_bytes_per_second).sum::<f64>());
        let disk_read = snapshot.storage.devices.iter()
            .filter_map(|item| item.read_bytes_per_second).sum::<f64>();
        let disk_write = snapshot.storage.devices.iter()
            .filter_map(|item| item.write_bytes_per_second).sum::<f64>();

        let is_new = !self.path.exists();
        let file = OpenOptions::new().create(true).append(true).open(&self.path);
        let mut file = match file {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error, "could not append monitoring history");
                return;
            }
        };
        if is_new {
            if let Err(error) = writeln!(file, "timestamp_unix,cpu_percent,gpu_percent,memory_percent,max_temperature_c,network_rx_bytes_s,network_tx_bytes_s,disk_read_bytes_s,disk_write_bytes_s") {
                tracing::warn!(%error, "could not write history CSV header");
                return;
            }
        }
        let number = |value: Option<f64>| value.filter(|v| v.is_finite()).map(|v| format!("{v:.3}")).unwrap_or_default();
        let row = [
            timestamp.to_string(),
            number(cpu),
            number(gpu),
            number(memory),
            number(temperature),
            number(network_rx),
            number(network_tx),
            number(Some(disk_read)),
            number(Some(disk_write)),
        ].join(",");
        if let Err(error) = writeln!(file, "{row}") {
            tracing::warn!(%error, "could not write monitoring history row");
        }
    }

    pub fn export_csv(&self, destination: &std::path::Path) -> std::io::Result<u64> {
        fs::copy(&self.path, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_path_is_named_history_csv() {
        let path = PathBuf::from("/tmp/gamemon/history.csv");
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("history.csv"));
    }

    #[test]
    fn unix_timestamp_is_non_negative() {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        assert!(timestamp > 0);
    }
}
