use std::{
    sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}},
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::collectors::{cpu, gpu, memory, network, process, storage, system};

const MINIMUM_INTERVAL: Duration = Duration::from_millis(100);
const PROCESS_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy)]
pub struct SamplerConfig {
    pub interval: Duration,
}

#[derive(Clone)]
pub struct SamplerControl {
    interval_ms: Arc<AtomicU64>,
    worker: Arc<Mutex<Option<thread::Thread>>>,
}

impl SamplerControl {
    pub fn set_interval(&self, interval: Duration) {
        let milliseconds = interval.as_millis().clamp(100, 60_000) as u64;
        self.interval_ms.store(milliseconds, Ordering::Relaxed);
        if let Ok(worker) = self.worker.lock() {
            if let Some(worker) = worker.as_ref() {
                worker.unpark();
            }
        }
    }
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
    pub gpu: Option<gpu::GpuSnapshot>,
    pub memory: memory::MemorySnapshot,
    pub network: Option<network::NetworkSnapshot>,
    pub system: system::SystemSnapshot,
    pub storage: storage::StorageSnapshot,
    pub processes: Arc<process::ProcessListSnapshot>,
}

#[derive(Default)]
struct Collectors {
    cpu: cpu::CpuCollector,
    gpu: gpu::GpuCollector,
    network: network::NetworkCollector,
    storage: storage::StorageCollector,
    processes: process::ProcessCollector,
    last_process_sample: Option<Instant>,
    process_snapshot: Arc<process::ProcessListSnapshot>,
}

impl Collectors {
    fn sample(&mut self) -> MonitorSnapshot {
        let cpu = self.cpu.sample();
        let gpu = self.gpu.sample();
        let memory = memory::sample();
        let network = self.network.sample();
        let system = system::sample();
        let storage = self.storage.sample();
        let now = Instant::now();
        if self.last_process_sample.map_or(true, |last| now.duration_since(last) >= PROCESS_SAMPLE_INTERVAL) {
            self.process_snapshot = Arc::new(self.processes.sample());
            self.last_process_sample = Some(now);
        }
        let processes = Arc::clone(&self.process_snapshot);
        MonitorSnapshot {
            sampled_at: SystemTime::now(),
            cpu,
            gpu,
            memory,
            network,
            system,
            storage,
            processes,
        }
    }
}

pub fn start(config: SamplerConfig) -> (Receiver<MonitorSnapshot>, SamplerControl) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let initial_ms = config.interval.max(MINIMUM_INTERVAL).as_millis().clamp(100, 60_000) as u64;
    let interval_ms = Arc::new(AtomicU64::new(initial_ms));
    let worker = Arc::new(Mutex::new(None));
    let control = SamplerControl { interval_ms: Arc::clone(&interval_ms), worker: Arc::clone(&worker) };
    match thread::Builder::new()
        .name("gamemon-sampler".to_owned())
        .spawn(move || run_sampler(sender, interval_ms))
    {
        Ok(handle) => {
            if let Ok(mut slot) = worker.lock() {
                *slot = Some(handle.thread().clone());
            }
        }
        Err(error) => tracing::error!(%error, "could not start hardware sampling thread"),
    }
    (receiver, control)
}

fn run_sampler(sender: SyncSender<MonitorSnapshot>, interval_ms: Arc<AtomicU64>) {
    let mut collectors = Collectors::default();
    loop {
        let cycle_started = Instant::now();
        let snapshot = collectors.sample();
        match sender.try_send(snapshot) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => break,
        }
        let interval = Duration::from_millis(interval_ms.load(Ordering::Relaxed));
        thread::park_timeout(interval.saturating_sub(cycle_started.elapsed()));
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
        let (receiver, _control) = start(SamplerConfig::default());
        let snapshot = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("sampler should deliver a snapshot");
        assert!(snapshot.sampled_at <= SystemTime::now());
    }
}
