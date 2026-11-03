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

struct MetricCard {
    primary: gtk::Label,
    secondary: gtk::Label,
    progress: gtk::ProgressBar,
    details: gtk::Label,
}

impl MetricCard {
    fn new(title: &str, subtitle: &str) -> (gtk::Frame, Self) {
        let container = gtk::Box::new(Orientation::Vertical, 10);
        container.set_margin_top(18);
        container.set_margin_bottom(18);
        container.set_margin_start(18);
        container.set_margin_end(18);

        let heading = gtk::Label::new(Some(title));
        heading.set_halign(Align::Start);
        heading.add_css_class("heading");
        heading.add_css_class("dim-label");

        let primary = gtk::Label::new(Some("Meten…"));
        primary.set_halign(Align::Start);
        primary.add_css_class("title-1");
        primary.add_css_class("numeric");

        let secondary = gtk::Label::new(Some(subtitle));
        secondary.set_halign(Align::Start);
        secondary.add_css_class("dim-label");
        secondary.set_wrap(true);

        let progress = gtk::ProgressBar::new();
        progress.set_show_text(false);
        progress.set_fraction(0.0);
        progress.add_css_class("osd");

        let details = gtk::Label::new(Some("Gegevens worden verzameld…"));
        details.set_halign(Align::Start);
        details.set_valign(Align::Start);
        details.set_wrap(true);
        details.set_selectable(true);
        details.add_css_class("dim-label");

        container.append(&heading);
        container.append(&primary);
        container.append(&secondary);
        container.append(&progress);
        container.append(&details);

        let frame = gtk::Frame::new(None);
        frame.add_css_class("card");
        frame.set_child(Some(&container));
        frame.set_hexpand(true);
        frame.set_vexpand(true);

        (
            frame,
            Self {
                primary,
                secondary,
                progress,
                details,
            },
        )
    }

    fn set_usage(&self, value: Option<f64>) {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            self.progress.set_fraction((value / 100.0).clamp(0.0, 1.0));
        } else {
            self.progress.set_fraction(0.0);
        }
    }
}

pub fn activate(application: &Application) {
    let (cpu_frame, cpu_card) = MetricCard::new("PROCESSOR", "Totale processorbelasting");
    let (gpu_frame, gpu_card) = MetricCard::new("GRAPHICS", "Grafische processor");
    let (memory_frame, memory_card) = MetricCard::new("GEHEUGEN", "RAM-gebruik");
    let (network_frame, network_card) = MetricCard::new("NETWERK", "Actuele netwerkdoorvoer");

    let content = gtk::Box::new(Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let intro = gtk::Box::new(Orientation::Vertical, 4);
    let title = gtk::Label::new(Some("GAMEMON"));
    title.set_halign(Align::Start);
    title.add_css_class("title-1");
    let subtitle = gtk::Label::new(Some("Hardwareoverzicht · live systeemtelemetrie"));
    subtitle.set_halign(Align::Start);
    subtitle.add_css_class("dim-label");
    intro.append(&title);
    intro.append(&subtitle);
    content.append(&intro);

    let grid = gtk::Grid::new();
    grid.set_column_spacing(16);
    grid.set_row_spacing(16);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);
    grid.attach(&cpu_frame, 0, 0, 1, 1);
    grid.attach(&gpu_frame, 1, 0, 1, 1);
    grid.attach(&memory_frame, 0, 1, 1, 1);
    grid.attach(&network_frame, 1, 1, 1, 1);
    content.append(&grid);

    let footer = gtk::Label::new(Some("Gegevens worden lokaal gelezen via Linux procfs, sysfs en ondersteunde driverinterfaces."));
    footer.set_halign(Align::Start);
    footer.set_wrap(true);
    footer.add_css_class("dim-label");
    footer.add_css_class("caption");
    content.append(&footer);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&content));

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&HeaderBar::new());
    toolbar.set_content(Some(&scrolled));
    let window = ApplicationWindow::builder()
        .application(application)
        .title("GAMEMON")
        .default_width(1080)
        .default_height(760)
        .content(&toolbar)
        .build();

    let config = AppConfig::load();
    let receiver = sampler::start(SamplerConfig {
        interval: config.refresh_interval(),
    });

    let cpu_card = downgrade_card(cpu_card);
    let gpu_card = downgrade_card(gpu_card);
    let memory_card = downgrade_card(memory_card);
    let network_card = downgrade_card(network_card);

    gtk::glib::timeout_add_local(UI_POLL_INTERVAL, move || {
        let (Some(cpu_card), Some(gpu_card), Some(memory_card), Some(network_card)) = (
            upgrade_card(&cpu_card),
            upgrade_card(&gpu_card),
            upgrade_card(&memory_card),
            upgrade_card(&network_card),
        ) else {
            return gtk::glib::ControlFlow::Break;
        };

        let mut latest = None;
        loop {
            match receiver.try_recv() {
                Ok(snapshot) => latest = Some(snapshot),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    cpu_card.primary.set_text("Niet beschikbaar");
                    gpu_card.primary.set_text("Niet beschikbaar");
                    memory_card.primary.set_text("Niet beschikbaar");
                    network_card.primary.set_text("Niet beschikbaar");
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        if let Some(snapshot) = latest {
            render_snapshot(&snapshot, &cpu_card, &gpu_card, &memory_card, &network_card);
        }
        gtk::glib::ControlFlow::Continue
    });

    window.present();
}

// Weak references prevent the sampling timer from keeping closed windows alive.
fn downgrade_card(card: MetricCard) -> WeakMetricCard {
    WeakMetricCard {
        primary: card.primary.downgrade(),
        secondary: card.secondary.downgrade(),
        progress: card.progress.downgrade(),
        details: card.details.downgrade(),
    }
}

struct WeakMetricCard {
    primary: gtk::glib::WeakRef<gtk::Label>,
    secondary: gtk::glib::WeakRef<gtk::Label>,
    progress: gtk::glib::WeakRef<gtk::ProgressBar>,
    details: gtk::glib::WeakRef<gtk::Label>,
}

fn upgrade_card(card: &WeakMetricCard) -> Option<MetricCard> {
    Some(MetricCard {
        primary: card.primary.upgrade()?,
        secondary: card.secondary.upgrade()?,
        progress: card.progress.upgrade()?,
        details: card.details.upgrade()?,
    })
}

fn render_snapshot(
    snapshot: &MonitorSnapshot,
    cpu_card: &MetricCard,
    gpu_card: &MetricCard,
    memory_card: &MetricCard,
    network_card: &MetricCard,
) {
    let cpu = &snapshot.cpu;
    let cpu_usage = cpu_utilization_value(cpu.utilization);
    cpu_card.primary.set_text(&format_cpu_utilization(cpu.utilization));
    cpu_card.set_usage(cpu_usage);
    cpu_card.secondary.set_text(cpu.model.as_deref().unwrap_or("CPU-model niet beschikbaar"));
    let average_frequency = average_cpu_frequency(cpu);
    let package_power = cpu.package_power_domains.iter()
        .filter_map(|domain| domain.power_watts.map(|watts| format!("{}: {watts:.1} W", domain.name)))
        .collect::<Vec<_>>();
    cpu_card.details.set_text(&format!(
        "{} logische CPU's · Gemiddelde gerapporteerde klok: {}\nPakketvermogen: {}",
        cpu.logical_cpus.len(),
        average_frequency,
        if package_power.is_empty() { "Niet beschikbaar".to_owned() } else { package_power.join(" · ") }
    ));

    match &snapshot.gpu {
        Some(gpu) if !gpu.devices.is_empty() => {
            let device = &gpu.devices[0];
            let name = device.model.as_deref().unwrap_or("GPU-model niet beschikbaar");
            let usage = device.gpu_busy_percent.map(f64::from);
            gpu_card.primary.set_text(&usage.map(|value| format!("{value:.0}%")).unwrap_or_else(|| "N/B".to_owned()));
            gpu_card.set_usage(usage);
            gpu_card.secondary.set_text(name);
            let vram = match (device.vram_used_bytes, device.vram_total_bytes) {
                (Some(used), Some(total)) => format!("VRAM {} / {}", format_bytes(used), format_bytes(total)),
                _ => "VRAM niet beschikbaar".to_owned(),
            };
            let temperature = device.temperature_celsius.map(|value| format!("{value} °C")).unwrap_or_else(|| "N/B".to_owned());
            let power = device.power_milliwatts.map(|value| format!("{:.1} W", value as f64 / 1000.0)).unwrap_or_else(|| "N/B".to_owned());
            let clocks = device.graphics_clock_mhz.map(|value| format!("{value} MHz")).unwrap_or_else(|| "N/B".to_owned());
            gpu_card.details.set_text(&format!("Temperatuur: {temperature} · Vermogen: {power}\n{vram} · Core clock: {clocks}\n{} GPU(s) gedetecteerd", gpu.devices.len()));
        }
        Some(_) => {
            gpu_card.primary.set_text("Geen GPU");
            gpu_card.secondary.set_text("Geen grafische apparaten gevonden");
            gpu_card.details.set_text("Controleer of de kernel DRM-apparaten beschikbaar stelt.");
            gpu_card.set_usage(None);
        }
        None => {
            gpu_card.primary.set_text("N/B");
            gpu_card.secondary.set_text("GPU-telemetrie niet beschikbaar");
            gpu_card.details.set_text("Er kon geen GPU-informatie worden verzameld.");
            gpu_card.set_usage(None);
        }
    }

    let memory = snapshot.memory;
    let used = memory.used_bytes();
    let total = memory.total_bytes;
    let memory_percent = match (used, total.filter(|value| *value > 0)) {
        (Some(used), Some(total)) => Some((used as f64 / total as f64) * 100.0),
        _ => None,
    };
    memory_card.primary.set_text(&memory_percent.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "N/B".to_owned()));
    memory_card.set_usage(memory_percent);
    memory_card.secondary.set_text(&format!(
        "{} gebruikt van {}",
        used.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        total.map(format_bytes).unwrap_or_else(|| "N/B".to_owned())
    ));
    let swap = match (memory.swap_used_bytes(), memory.swap_total_bytes) {
        (Some(used), Some(total)) => format!("Swap: {} / {}", format_bytes(used), format_bytes(total)),
        _ => "Swap: N/B".to_owned(),
    };
    memory_card.details.set_text(&format!(
        "Beschikbaar: {} · Vrij: {}\n{}",
        memory.available_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        memory.free_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        swap
    ));

    match &snapshot.network {
        Some(network) if !network.interfaces.is_empty() => {
            let active = network.interfaces.iter().filter(|interface| interface.carrier == Some(true) || interface.operational_state.as_deref() == Some("up")).collect::<Vec<_>>();
            let rx = active.iter().filter_map(|interface| interface.rx_bytes_per_second).sum::<f64>();
            let tx = active.iter().filter_map(|interface| interface.tx_bytes_per_second).sum::<f64>();
            network_card.primary.set_text(&format!("↓ {}  ↑ {}", format_rate(rx), format_rate(tx)));
            network_card.secondary.set_text(&format!("{} interfaces · {} actief", network.interfaces.len(), active.len()));
            network_card.details.set_text(&active.iter().map(|interface| {
                format!("{} · ↓ {} · ↑ {}", interface.name, format_rate(interface.rx_bytes_per_second.unwrap_or(0.0)), format_rate(interface.tx_bytes_per_second.unwrap_or(0.0)))
            }).collect::<Vec<_>>().join("\n"));
            network_card.set_usage(None);
        }
        Some(_) => {
            network_card.primary.set_text("Geen interfaces");
            network_card.secondary.set_text("Geen netwerkinterfaces gevonden");
            network_card.details.set_text("Netwerkdetails zijn niet beschikbaar.");
            network_card.set_usage(None);
        }
        None => {
            network_card.primary.set_text("N/B");
            network_card.secondary.set_text("Netwerktelemetrie niet beschikbaar");
            network_card.details.set_text("Er kon geen netwerk-informatie worden verzameld.");
            network_card.set_usage(None);
        }
    }
}

fn cpu_utilization_value(value: cpu::CpuUtilization) -> Option<f64> {
    match value {
        cpu::CpuUtilization::Available(value) if value.is_finite() => Some(value.clamp(0.0, 100.0)),
        _ => None,
    }
}

fn average_cpu_frequency(cpu: &cpu::CpuSnapshot) -> String {
    let frequencies = cpu.logical_cpus.iter().filter_map(|core| core.current_frequency_hz).collect::<Vec<_>>();
    if frequencies.is_empty() {
        "Niet beschikbaar".to_owned()
    } else {
        let average = frequencies.iter().map(|value| *value as f64).sum::<f64>() / frequencies.len() as f64;
        format!("{:.0} MHz", average / 1_000_000.0)
    }
}

fn format_cpu_utilization(value: cpu::CpuUtilization) -> String {
    match value {
        cpu::CpuUtilization::Available(value) => format!("{value:.1}%"),
        cpu::CpuUtilization::WarmingUp => "Meten…".to_owned(),
        cpu::CpuUtilization::Unavailable => "N/B".to_owned(),
    }
}

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1_073_741_824.0;
    if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MiB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    }
}

fn format_rate(bytes: f64) -> String {
    if bytes >= 1_048_576.0 {
        format!("{:.1} MiB/s", bytes / 1_048_576.0)
    } else if bytes >= 1024.0 {
        format!("{:.1} KiB/s", bytes / 1024.0)
    } else {
        format!("{bytes:.0} B/s")
    }
}
