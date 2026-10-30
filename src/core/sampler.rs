use std::{
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::collectors::{cpu, memory, network};

const MINIMUM_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy)]
pub struct SamplerConfig {
    pub interval: Duration,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MonitorSnapshot {
    pub sampled_at: SystemTime,
    pub cpu: cpu::CpuSnapshot,
    pub memory: memory::MemorySnapshot,
    pub network: Option<network::NetworkSnapshot>,
}

#[derive(Debug, Default)]
struct Collectors {
    cpu: cpu::CpuCollector,
    network: network::NetworkCollector,
}

impl Collectors {
    fn sample(&mut self) -> MonitorSnapshot {
        let cpu = self.cpu.sample();
        let memory = memory::sample();
        let network = self.network.sample();
        MonitorSnapshot {
            sampled_at: SystemTime::now(),
            cpu,
            memory,
            network,
        }
    }
}

pub fn start(config: SamplerConfig) -> Receiver<MonitorSnapshot> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let interval = config.interval.max(MINIMUM_INTERVAL);
    if let Err(error) = thread::Builder::new()
        .name("gamemon-sampler".to_owned())
        .spawn(move || run_sampler(sender, interval))
    {
        tracing::error!(%error, "could not start hardware sampling thread");
    }
    receiver
}

fn run_sampler(sender: SyncSender<MonitorSnapshot>, interval: Duration) {
    let mut collectors = Collectors::default();
    loop {
        let cycle_started = Instant::now();
        let snapshot = collectors.sample();
        match sender.try_send(snapshot) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => break,
        }
        thread::sleep(interval.saturating_sub(cycle_started.elapsed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_interval_is_one_second() {
        assert_eq!(SamplerConfig::default().interval, Duration::from_secs(1));
    }

    #[test]
    fn clamps_too_small_sampling_intervals() {
        assert_eq!(
            SamplerConfig {
                interval: Duration::ZERO
            }
            .interval
            .max(MINIMUM_INTERVAL),
            MINIMUM_INTERVAL
        );
    }

    #[test]
    fn background_sampler_delivers_a_snapshot() {
        let receiver = start(SamplerConfig::default());
        let snapshot = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("sampler should deliver a snapshot");
        assert!(snapshot.sampled_at <= SystemTime::now());
    }
}
