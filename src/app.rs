use std::{sync::mpsc::TryRecvError, time::Duration};

use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
use gtk::{Align, Orientation};

use crate::{
    collectors::{cpu, gpu, memory, network},
    core::{
        config::AppConfig,
        sampler::{self, MonitorSnapshot, SamplerConfig},
    },
};

const UI_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn activate(application: &Application) {
    let title = gtk::Label::builder()
        .label("GAMEMON")
        .halign(Align::Start)
        .css_classes(["title-1"])
        .build();
    let cpu_label = gtk::Label::new(Some("CPU\nMeten…"));
    cpu_label.set_halign(Align::Start);
    cpu_label.set_selectable(true);
    let gpu_label = gtk::Label::new(Some("GPU\nDetecteren…"));
    gpu_label.set_halign(Align::Start);
    gpu_label.set_selectable(true);
    let memory_label = gtk::Label::new(Some("Geheugen\nMeten…"));
    memory_label.set_halign(Align::Start);
    memory_label.set_selectable(true);
    let network_label = gtk::Label::new(Some("Netwerk\nMeten…"));
    network_label.set_halign(Align::Start);
    network_label.set_selectable(true);

    let content = gtk::Box::new(Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&title);
    content.append(&cpu_label);
    content.append(&gpu_label);
    content.append(&memory_label);
    content.append(&network_label);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&content));

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&HeaderBar::new());
    toolbar.set_content(Some(&scrolled));
    let window = ApplicationWindow::builder()
        .application(application)
        .title("GAMEMON")
        .default_width(900)
        .default_height(650)
        .content(&toolbar)
        .build();

    let config = AppConfig::load();
    let receiver = sampler::start(SamplerConfig {
        interval: config.refresh_interval(),
    });
    let cpu_label = cpu_label.downgrade();
    let gpu_label = gpu_label.downgrade();
    let memory_label = memory_label.downgrade();
    let network_label = network_label.downgrade();
    gtk::glib::timeout_add_local(UI_POLL_INTERVAL, move || {
        let (Some(cpu_label), Some(gpu_label), Some(memory_label), Some(network_label)) = (
            cpu_label.upgrade(),
            gpu_label.upgrade(),
            memory_label.upgrade(),
            network_label.upgrade(),
        ) else {
            return gtk::glib::ControlFlow::Break;
        };

        let mut latest = None;
        loop {
            match receiver.try_recv() {
                Ok(snapshot) => latest = Some(snapshot),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    cpu_label.set_text("CPU\nSampling niet beschikbaar");
                    gpu_label.set_text("GPU\nSampling niet beschikbaar");
                    memory_label.set_text("Geheugen\nSampling niet beschikbaar");
                    network_label.set_text("Netwerk\nSampling niet beschikbaar");
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        if let Some(snapshot) = latest {
            render_snapshot(
                &snapshot,
                &cpu_label,
                &gpu_label,
                &memory_label,
                &network_label,
            );
        }
        gtk::glib::ControlFlow::Continue
    });

    window.present();
}

fn render_snapshot(
    snapshot: &MonitorSnapshot,
    cpu_label: &gtk::Label,
    gpu_label: &gtk::Label,
    memory_label: &gtk::Label,
    network_label: &gtk::Label,
) {
    let cpu = &snapshot.cpu;
    let model = cpu.model.as_deref().unwrap_or("Niet beschikbaar");
    let online = cpu.online.as_deref().unwrap_or("Niet beschikbaar");
    let utilization = format_cpu_utilization(cpu.utilization);
    let logical_cpu_lines = cpu
        .logical_cpus
        .iter()
        .map(|logical_cpu| {
            let package = logical_cpu
                .package_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_owned());
            let core = logical_cpu
                .core_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_owned());
            let frequency = logical_cpu
                .current_frequency_hz
                .map(|value| format!("reported {:.0} MHz", value as f64 / 1_000_000.0))
                .unwrap_or_else(|| "reported frequency n.b.".to_owned());
            format!(
                "CPU{}: {} · {} · package {} · core {}",
                logical_cpu.id,
                format_cpu_utilization(logical_cpu.utilization),
                frequency,
                package,
                core
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let logical_cpu_lines = if logical_cpu_lines.is_empty() {
        "Per-logical-CPU details: Niet beschikbaar".to_owned()
    } else {
        logical_cpu_lines
    };
    cpu_label.set_text(&format!(
        "CPU\n{model}\nOnline logical CPU's: {online}\nTotaal gebruik: {utilization}\n{logical_cpu_lines}"
    ));

    let gpu_text = match &snapshot.gpu {
        None => "GPU\nNiet beschikbaar".to_owned(),
        Some(snapshot) if snapshot.devices.is_empty() => {
            "GPU\nGeen DRM-apparaten gevonden".to_owned()
        }
        Some(snapshot) => format!(
            "GPU\n{}",
            snapshot
                .devices
                .iter()
                .map(format_gpu_device)
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    gpu_label.set_text(&gpu_text);

    let memory = snapshot.memory;
    let memory_text = format!(
        "Geheugen\nRAM gebruikt: {} / {}\nBeschikbaar: {} · Vrij: {}\nBuffers: {} · Cache: {}\nSwap gebruikt: {} / {} (vrij {})\nHugePages: {}\nMemory PSI: {}",
        format_optional_bytes(memory.used_bytes()),
        format_optional_bytes(memory.total_bytes),
        format_optional_bytes(memory.available_bytes),
        format_optional_bytes(memory.free_bytes),
        format_optional_bytes(memory.buffers_bytes),
        format_optional_bytes(memory.cached_bytes),
        format_optional_bytes(memory.swap_used_bytes()),
        format_optional_bytes(memory.swap_total_bytes),
        format_optional_bytes(memory.swap_free_bytes),
        format_hugepages(memory),
        format_memory_pressure(memory.pressure),
    );
    memory_label.set_text(&memory_text);

    let network_text = match &snapshot.network {
        None => "Netwerk\nNiet beschikbaar".to_owned(),
        Some(snapshot) if snapshot.interfaces.is_empty() => {
            "Netwerk\nGeen interfaces gevonden".to_owned()
        }
        Some(snapshot) => format!(
            "Netwerk\n{}",
            snapshot
                .interfaces
                .iter()
                .map(format_network_interface)
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    network_label.set_text(&network_text);
}

fn format_gpu_device(device: &gpu::GpuDeviceSnapshot) -> String {
    let name = device
        .model
        .as_deref()
        .map(str::to_owned)
        .or_else(|| match (device.vendor_id, device.device_id) {
            (Some(vendor), Some(product)) => Some(format!("PCI {vendor:#06x}:{product:#06x}")),
            _ => None,
        })
        .unwrap_or_else(|| "GPU-model niet beschikbaar".to_owned());
    let pci_address = device.pci_address.as_deref().unwrap_or("PCI-adres n.b.");
    let driver = device.driver.as_deref().unwrap_or("driver n.b.");
    let utilization = device
        .gpu_busy_percent
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "Niet beschikbaar".to_owned());
    let vram = match (device.vram_used_bytes, device.vram_total_bytes) {
        (Some(used), Some(total)) => format!(
            "{} / {}",
            format_optional_bytes(Some(used)),
            format_optional_bytes(Some(total))
        ),
        _ => "Niet beschikbaar".to_owned(),
    };
    format!(
        "{} · {pci_address} · {driver}\n  Belasting: {utilization} · VRAM: {vram} · temperatuur: Niet beschikbaar",
        name
    )
}

fn format_cpu_utilization(utilization: cpu::CpuUtilization) -> String {
    match utilization {
        cpu::CpuUtilization::Available(value) => format!("{value:.1}%"),
        cpu::CpuUtilization::WarmingUp => "Meten…".to_owned(),
        cpu::CpuUtilization::Unavailable => "Niet beschikbaar".to_owned(),
    }
}

fn format_optional_bytes(value: Option<u64>) -> String {
    value
        .map(|bytes| format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0))
        .unwrap_or_else(|| "Niet beschikbaar".to_owned())
}

fn format_hugepages(memory: memory::MemorySnapshot) -> String {
    match (
        memory.huge_pages_free,
        memory.huge_pages_total,
        memory.huge_page_size_bytes,
    ) {
        (Some(free), Some(total), Some(size)) => format!(
            "{free} vrij / {total} totaal ({:.0} MiB per pagina)",
            size as f64 / 1_048_576.0
        ),
        _ => "Niet beschikbaar".to_owned(),
    }
}

fn format_memory_pressure(pressure: Option<memory::MemoryPressure>) -> String {
    pressure
        .map(|pressure| {
            let full = pressure
                .full
                .map(|metrics| format!("{:.2}%", metrics.avg10))
                .unwrap_or_else(|| "Niet beschikbaar".to_owned());
            format!("some {:.2}% · full {full}", pressure.some.avg10)
        })
        .unwrap_or_else(|| "Niet beschikbaar".to_owned())
}

fn format_network_interface(interface: &network::NetworkInterfaceSnapshot) -> String {
    let state = interface
        .operational_state
        .as_deref()
        .unwrap_or("state n.b.");
    let carrier = match interface.carrier {
        Some(true) => "carrier on",
        Some(false) => "carrier off",
        None => "carrier n.b.",
    };
    let speed = interface
        .speed_mbps
        .map(|value| format!("{value} Mbps"))
        .unwrap_or_else(|| "speed n.b.".to_owned());
    let duplex = interface.duplex.as_deref().unwrap_or("duplex n.b.");
    format!(
        "{} · {state}, {carrier} · {speed} {duplex}\n  ↓ {} ({} totaal, {} errors, {} drops) · ↑ {} ({} totaal, {} errors, {} drops)",
        interface.name,
        format_data_rate(interface.rx_bytes_per_second),
        format_optional_data_size(interface.rx_bytes),
        format_optional_count(interface.rx_errors),
        format_optional_count(interface.rx_dropped),
        format_data_rate(interface.tx_bytes_per_second),
        format_optional_data_size(interface.tx_bytes),
        format_optional_count(interface.tx_errors),
        format_optional_count(interface.tx_dropped),
    )
}

fn format_data_rate(value: Option<f64>) -> String {
    match value {
        Some(bytes) if bytes >= 1_048_576.0 => format!("{:.1} MiB/s", bytes / 1_048_576.0),
        Some(bytes) if bytes >= 1024.0 => format!("{:.1} KiB/s", bytes / 1024.0),
        Some(bytes) => format!("{bytes:.0} B/s"),
        None => "Meten…".to_owned(),
    }
}

fn format_optional_data_size(value: Option<u64>) -> String {
    match value {
        Some(bytes) if bytes >= 1_073_741_824 => {
            format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
        }
        Some(bytes) if bytes >= 1_048_576 => {
            format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
        }
        Some(bytes) if bytes >= 1024 => format!("{:.1} KiB", bytes as f64 / 1024.0),
        Some(bytes) => format!("{bytes} B"),
        None => "n.b.".to_owned(),
    }
}

fn format_optional_count(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n.b.".to_owned())
}
