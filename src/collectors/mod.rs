#[cfg(target_os = "linux")]
pub mod cpu;
#[cfg(target_os = "linux")]
pub mod gpu;
#[cfg(target_os = "linux")]
pub mod memory;
#[cfg(target_os = "linux")]
pub mod network;
#[cfg(target_os = "linux")]
pub mod system;
#[cfg(target_os = "linux")]
pub mod process;
#[cfg(target_os = "linux")]
pub mod storage;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{cpu, gpu, memory, network, process, storage, system};
pub(crate) use macos::AppleSiliconCollector;
