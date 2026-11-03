use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::mpsc::TryRecvError, time::Duration};

use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
use gtk::{Align, Orientation};

use crate::{
    collectors::cpu,
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
    history: Rc<RefCell<VecDeque<f64>>>,
    chart: gtk::DrawingArea,
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

        let history = Rc::new(RefCell::new(VecDeque::<f64>::with_capacity(60)));
        let chart = gtk::DrawingArea::new();
        chart.set_content_height(88);
        chart.set_hexpand(true);
        chart.add_css_class("gamemon-sparkline");
        let chart_history = Rc::clone(&history);
        chart.set_draw_func(move |_, context, width, height| {
            let values = chart_history.borrow();
            if values.len() < 2 || width <= 1 || height <= 1 {
                return;
            }

            let width = f64::from(width);
            let height = f64::from(height);
            let left = 38.0;
            let right = width - 8.0;
            let top = 8.0;
            let bottom = height - 20.0;
            let plot_width = right - left;
            let plot_height = bottom - top;
            if plot_width <= 1.0 || plot_height <= 1.0 {
                return;
            }

            let points = values.iter().copied().collect::<Vec<_>>();
            let minimum = points.iter().copied().fold(f64::INFINITY, f64::min);
            let maximum = points.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let span = (maximum - minimum).max(1.0);
            let padding = (span * 0.18).max(0.5);
            let scale_min = (minimum - padding).max(0.0);
            let scale_max = (maximum + padding).min(100.0).max(scale_min + 1.0);
            let scale_span = scale_max - scale_min;

            context.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
            context.set_font_size(10.0);
            context.set_line_width(1.0);
            context.set_source_rgba(0.65, 0.68, 0.72, 0.22);

            for tick in 0..=2 {
                let fraction = tick as f64 / 2.0;
                let y = bottom - fraction * plot_height;
                context.move_to(left, y);
                context.line_to(right, y);
                let _ = context.stroke();

                let value = scale_min + fraction * scale_span;
                context.set_source_rgba(0.72, 0.74, 0.77, 0.9);
                context.move_to(0.0, y + 3.5);
                let _ = context.show_text(&format!("{value:.1}%"));
                context.set_source_rgba(0.65, 0.68, 0.72, 0.22);
            }

            let coordinates = points.iter().enumerate().map(|(index, value)| {
                let x = left + index as f64 / (points.len() - 1) as f64 * plot_width;
                let normalized = ((*value).clamp(scale_min, scale_max) - scale_min) / scale_span;
                let y = bottom - normalized * plot_height;
                (x, y)
            }).collect::<Vec<_>>();

            context.move_to(coordinates[0].0, bottom);
            for (x, y) in &coordinates {
                context.line_to(*x, *y);
            }
            context.line_to(coordinates.last().unwrap().0, bottom);
            context.close_path();
            context.set_source_rgba(0.25, 0.68, 0.91, 0.12);
            let _ = context.fill();

            context.set_line_width(2.0);
            context.set_line_cap(gtk::cairo::LineCap::Round);
            context.set_line_join(gtk::cairo::LineJoin::Round);
            context.set_source_rgba(0.35, 0.72, 0.95, 1.0);
            for (index, (x, y)) in coordinates.iter().enumerate() {
                if index == 0 {
                    context.move_to(*x, *y);
                } else {
                    context.line_to(*x, *y);
                }
            }
            let _ = context.stroke();

            let latest = points.last().copied().unwrap_or_default();
            context.set_source_rgba(0.35, 0.72, 0.95, 1.0);
            context.arc(coordinates.last().unwrap().0, coordinates.last().unwrap().1, 3.0, 0.0, std::f64::consts::TAU);
            let _ = context.fill();

            context.set_source_rgba(0.72, 0.74, 0.77, 0.9);
            context.move_to(left, height - 4.0);
            let _ = context.show_text("60 metingen geleden");
            let label = "nu";
            let label_width = context.text_extents(label).map(|extents| extents.width()).unwrap_or(12.0);
            context.move_to(right - label_width, height - 4.0);
            let _ = context.show_text(label);

            context.set_source_rgba(0.35, 0.72, 0.95, 1.0);
            context.move_to(right - 66.0, top - 1.0);
            let _ = context.show_text(&format!("{latest:.1}%"));
        });

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
        container.append(&chart);
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
                history,
                chart,
                details,
            },
        )
    }

    fn set_usage(&self, value: Option<f64>) {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            self.chart.set_visible(true);
            let value = value.clamp(0.0, 100.0);
            self.progress.set_fraction(value / 100.0);
            let mut history = self.history.borrow_mut();
            if history.len() == 60 {
                history.pop_front();
            }
            history.push_back(value);
            drop(history);
            self.chart.queue_draw();
        } else {
            self.progress.set_fraction(0.0);
            self.chart.set_visible(false);
        }
    }
}

pub fn activate(application: &Application) {
    let (cpu_frame, cpu_card) = MetricCard::new("PROCESSOR", "Totale processorbelasting");
    let (gpu_frame, gpu_card) = MetricCard::new("GRAPHICS", "Grafische processor");
    let (memory_frame, memory_card) = MetricCard::new("GEHEUGEN", "RAM-gebruik");
    let (network_frame, network_card) = MetricCard::new("NETWERK", "Actuele netwerkdoorvoer");
    let (system_frame, system_card) = MetricCard::new("SYSTEEM", "Uptime en load average");
    let (sensors_frame, sensors_card) = MetricCard::new("SENSOREN", "Temperaturen en ventilatoren");

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
    grid.attach(&system_frame, 0, 2, 1, 1);
    grid.attach(&sensors_frame, 1, 2, 1, 1);
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
    let system_card = downgrade_card(system_card);
    let sensors_card = downgrade_card(sensors_card);

    gtk::glib::timeout_add_local(UI_POLL_INTERVAL, move || {
        let (Some(cpu_card), Some(gpu_card), Some(memory_card), Some(network_card), Some(system_card), Some(sensors_card)) = (
            upgrade_card(&cpu_card),
            upgrade_card(&gpu_card),
            upgrade_card(&memory_card),
            upgrade_card(&network_card),
            upgrade_card(&system_card),
            upgrade_card(&sensors_card),
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
                    system_card.primary.set_text("Niet beschikbaar");
                    sensors_card.primary.set_text("Niet beschikbaar");
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        if let Some(snapshot) = latest {
            render_snapshot(&snapshot, &cpu_card, &gpu_card, &memory_card, &network_card, &system_card, &sensors_card);
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
        chart: card.chart.downgrade(),
        history: Rc::clone(&card.history),
        details: card.details.downgrade(),
    }
}

struct WeakMetricCard {
    primary: gtk::glib::WeakRef<gtk::Label>,
    secondary: gtk::glib::WeakRef<gtk::Label>,
    progress: gtk::glib::WeakRef<gtk::ProgressBar>,
    chart: gtk::glib::WeakRef<gtk::DrawingArea>,
    history: Rc<RefCell<VecDeque<f64>>>,
    details: gtk::glib::WeakRef<gtk::Label>,
}

fn upgrade_card(card: &WeakMetricCard) -> Option<MetricCard> {
    Some(MetricCard {
        primary: card.primary.upgrade()?,
        secondary: card.secondary.upgrade()?,
        progress: card.progress.upgrade()?,
        chart: card.chart.upgrade()?,
        history: Rc::clone(&card.history),
        details: card.details.upgrade()?,
    })
}

fn render_snapshot(
    snapshot: &MonitorSnapshot,
    cpu_card: &MetricCard,
    gpu_card: &MetricCard,
    memory_card: &MetricCard,
    network_card: &MetricCard,
    system_card: &MetricCard,
    sensors_card: &MetricCard,
) {
    let cpu = &snapshot.cpu;
    let cpu_usage = cpu_utilization_value(cpu.utilization);
    cpu_card.primary.set_text(&format_cpu_utilization(cpu.utilization));
    cpu_card.set_usage(cpu_usage);
    cpu_card.secondary.set_text(cpu.model.as_deref().unwrap_or("CPU-model niet beschikbaar"));
    let average_frequency = average_cpu_frequency(cpu);
    let mut power_by_domain = std::collections::BTreeMap::<String, f64>::new();
    for domain in &cpu.package_power_domains {
        if let Some(watts) = domain.power_watts.filter(|value| value.is_finite()) {
            power_by_domain.entry(domain.name.clone())
                .and_modify(|current| *current = current.max(watts))
                .or_insert(watts);
        }
    }
    let package_power = power_by_domain.iter()
        .map(|(name, watts)| format!("{name}: {watts:.1} W"))
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

    let system = &snapshot.system;
    let uptime = system.uptime_seconds.map(format_uptime).unwrap_or_else(|| "Niet beschikbaar".to_owned());
    let load = system.load_average.map(|values| format!("{:.2} · {:.2} · {:.2}", values[0], values[1], values[2])).unwrap_or_else(|| "Niet beschikbaar".to_owned());
    system_card.primary.set_text(&uptime);
    system_card.secondary.set_text("Systeem actief");
    system_card.details.set_text(&format!("Load average (1 / 5 / 15 min): {load}"));
    system_card.set_usage(None);

    if system.temperatures.is_empty() && system.fans.is_empty() {
        sensors_card.primary.set_text("Geen sensoren");
        sensors_card.secondary.set_text("Geen hwmon-metingen beschikbaar");
        sensors_card.details.set_text("Controleer of de kernel temperatuur- en fanwaarden via /sys/class/hwmon exporteert.");
    } else {
        let hottest = system.temperatures.iter().max_by(|left, right| left.celsius.total_cmp(&right.celsius));
        if let Some(reading) = hottest {
            sensors_card.primary.set_text(&format!("{:.1} °C", reading.celsius));
            sensors_card.secondary.set_text(&format!("Hoogste temperatuur · {}", reading.label));
        } else {
            sensors_card.primary.set_text(&format!("{} fans", system.fans.len()));
            sensors_card.secondary.set_text("Ventilatorsnelheden");
        }
        let temperatures = system.temperatures.iter().take(8).map(|reading| format!("{}: {:.1} °C", reading.label, reading.celsius)).collect::<Vec<_>>();
        let fans = system.fans.iter().take(6).map(|reading| format!("{}: {} RPM", reading.label, reading.rpm)).collect::<Vec<_>>();
        sensors_card.details.set_text(&temperatures.into_iter().chain(fans).collect::<Vec<_>>().join("\n"));
    }
    sensors_card.set_usage(None);

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

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours:02}u {minutes:02}m")
    } else {
        format!("{hours}u {minutes:02}m")
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
