use std::{cell::{Cell, RefCell}, collections::{BTreeMap, VecDeque}, rc::Rc, sync::mpsc::TryRecvError, time::Duration};

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
    displayed_usage: Rc<Cell<f64>>,
    target_usage: Rc<Cell<f64>>,
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
        let displayed_usage = Rc::new(Cell::new(0.0));
        let target_usage = Rc::new(Cell::new(0.0));
        let chart_history = Rc::clone(&history);
        let chart_displayed = Rc::clone(&displayed_usage);
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

            let mut points = values.iter().copied().collect::<Vec<_>>();
            if let Some(last) = points.last_mut() { *last = chart_displayed.get(); }
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
        details.set_lines(6);
        details.set_ellipsize(gtk::pango::EllipsizeMode::End);
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
        frame.set_vexpand(false);
        frame.set_valign(Align::Start);

        // Samples arrive at the configured interval; ease visual values at ~60 FPS.
        let weak_progress = progress.downgrade();
        let weak_chart = chart.downgrade();
        let animated = Rc::clone(&displayed_usage);
        let target = Rc::clone(&target_usage);
        gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
            let (Some(progress), Some(chart)) = (weak_progress.upgrade(), weak_chart.upgrade()) else {
                return gtk::glib::ControlFlow::Break;
            };
            let current = animated.get();
            let destination = target.get();
            let difference = destination - current;
            let next = if difference.abs() < 0.05 { destination } else { current + difference * 0.22 };
            animated.set(next);
            progress.set_fraction(next / 100.0);
            chart.queue_draw();
            gtk::glib::ControlFlow::Continue
        });

        (
            frame,
            Self {
                primary,
                secondary,
                progress,
                history,
                displayed_usage,
                target_usage,
                chart,
                details,
            },
        )
    }

    fn set_usage(&self, value: Option<f64>) {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            self.chart.set_visible(true);
            let value = value.clamp(0.0, 100.0);
            self.target_usage.set(value);
            let mut history = self.history.borrow_mut();
            if history.len() == 60 {
                history.pop_front();
            }
            history.push_back(value);
            drop(history);
            self.chart.queue_draw();
        } else {
            self.target_usage.set(0.0);
            self.displayed_usage.set(0.0);
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
    let (storage_frame, storage_card) = MetricCard::new("OPSLAG", "Bestandssystemen en schijfactiviteit");
    let (process_frame, process_card) = MetricCard::new("PROCESSEN", "Actieve processen · CPU en geheugen");

    let content = gtk::Box::new(Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);

    let intro = gtk::Box::new(Orientation::Vertical, 4);
    let title = gtk::Label::new(Some("Overzicht"));
    title.set_halign(Align::Start);
    title.add_css_class("title-1");
    let subtitle = gtk::Label::new(Some("Hardwareoverzicht · live systeemtelemetrie"));
    subtitle.set_halign(Align::Start);
    subtitle.add_css_class("dim-label");
    intro.append(&title);
    intro.append(&subtitle);
    content.append(&intro);

    // Two independent vertical stacks avoid FlowBox's equal-height rows,
    // which created large blank areas below shorter cards.
    let dashboard = gtk::Box::new(Orientation::Horizontal, 16);
    dashboard.set_hexpand(true);
    let left_column = gtk::Box::new(Orientation::Vertical, 16);
    left_column.set_hexpand(true);
    left_column.set_valign(Align::Start);
    let right_column = gtk::Box::new(Orientation::Vertical, 16);
    right_column.set_hexpand(true);
    right_column.set_valign(Align::Start);

    for card in [&cpu_frame, &memory_frame, &system_frame, &storage_frame] {
        left_column.append(card);
    }
    for card in [&gpu_frame, &network_frame, &sensors_frame, &process_frame] {
        right_column.append(card);
    }
    dashboard.append(&left_column);
    dashboard.append(&right_column);
    content.append(&dashboard);

    let footer = gtk::Label::new(Some("Gegevens worden lokaal gelezen via Linux procfs, sysfs en ondersteunde driverinterfaces."));
    footer.set_halign(Align::Start);
    footer.set_wrap(true);
    footer.add_css_class("dim-label");
    footer.add_css_class("caption");
    content.append(&footer);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(180);
    stack.set_vexpand(true);
    let overview_scroll = gtk::ScrolledWindow::new();
    overview_scroll.set_vexpand(true);
    overview_scroll.set_child(Some(&content));
    stack.add_named(&overview_scroll, Some("overview"));

    let mut detail_labels = BTreeMap::new();
    for (key, heading, description) in [
        ("cpu", "Processor", "Per-logische CPU-belasting, frequentie en topologie."),
        ("gpu", "Graphics", "GPU-belasting, VRAM, temperatuur, vermogen en kloksnelheden."),
        ("memory", "Geheugen", "RAM, swap, cache, buffers, HugePages en pressure."),
        ("motherboard", "Moederbord & sensoren", "Alle hardwaretemperaturen en ventilatorsnelheden."),
        ("storage", "Opslag", "Bestandssystemen, schijfactiviteit en I/O-tellers."),
        ("network", "Netwerk", "Interfaces, linkstatus, snelheid, verkeer en fouttellers."),
        ("processes", "Processen", "Actieve processen met CPU-, geheugen- en statusinformatie."),
        ("system", "Systeem", "Uptime en load average."),
    ] {
        let page = gtk::Box::new(Orientation::Vertical, 12);
        page.set_margin_top(24);
        page.set_margin_bottom(24);
        page.set_margin_start(24);
        page.set_margin_end(24);
        let title = gtk::Label::new(Some(heading));
        title.set_halign(Align::Start);
        title.add_css_class("title-1");
        let subtitle = gtk::Label::new(Some(description));
        subtitle.set_halign(Align::Start);
        subtitle.set_wrap(true);
        subtitle.add_css_class("dim-label");
        let data = gtk::Label::new(Some("Gegevens worden verzameld…"));
        data.set_halign(Align::Start);
        data.set_valign(Align::Start);
        data.set_xalign(0.0);
        data.set_yalign(0.0);
        data.set_selectable(true);
        data.set_wrap(false);
        data.add_css_class("monospace");
        page.append(&title);
        page.append(&subtitle);
        page.append(&gtk::Separator::new(Orientation::Horizontal));
        page.append(&data);
        let page_scroll = gtk::ScrolledWindow::new();
        page_scroll.set_vexpand(true);
        page_scroll.set_child(Some(&page));
        stack.add_named(&page_scroll, Some(key));
        detail_labels.insert(key.to_owned(), data);
    }
    let menu_button = gtk::MenuButton::new();
    menu_button.set_label("Hardware");
    let popover = gtk::Popover::new();
    let menu = gtk::Box::new(Orientation::Vertical, 4);
    menu.set_margin_top(8);
    menu.set_margin_bottom(8);
    menu.set_margin_start(8);
    menu.set_margin_end(8);
    for (key, label) in [
        ("overview", "Overzicht"), ("cpu", "Processor"), ("gpu", "Graphics"),
        ("memory", "Geheugen"), ("motherboard", "Moederbord & sensoren"),
        ("storage", "Opslag"), ("network", "Netwerk"), ("processes", "Processen"),
        ("system", "Systeem"),
    ] {
        let button = gtk::Button::with_label(label);
        button.set_halign(Align::Fill);
        let target_stack = stack.clone();
        let target_popover = popover.clone();
        button.connect_clicked(move |_| {
            target_stack.set_visible_child_name(key);
            target_popover.popdown();
        });
        menu.append(&button);
    }
    popover.set_child(Some(&menu));
    menu_button.set_popover(Some(&popover));
    let header = HeaderBar::new();
    header.pack_start(&menu_button);
    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));
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
    let storage_card = downgrade_card(storage_card);
    let process_card = downgrade_card(process_card);

    gtk::glib::timeout_add_local(UI_POLL_INTERVAL, move || {
        let (Some(cpu_card), Some(gpu_card), Some(memory_card), Some(network_card), Some(system_card), Some(sensors_card), Some(storage_card), Some(process_card)) = (
            upgrade_card(&cpu_card),
            upgrade_card(&gpu_card),
            upgrade_card(&memory_card),
            upgrade_card(&network_card),
            upgrade_card(&system_card),
            upgrade_card(&sensors_card),
            upgrade_card(&storage_card),
            upgrade_card(&process_card),
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
                    storage_card.primary.set_text("Niet beschikbaar");
                    process_card.primary.set_text("Niet beschikbaar");
                    return gtk::glib::ControlFlow::Break;
                }
            }
        }
        if let Some(snapshot) = latest {
            render_snapshot(&snapshot, &cpu_card, &gpu_card, &memory_card, &network_card, &system_card, &sensors_card, &storage_card, &process_card, &detail_labels);
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
        displayed_usage: Rc::clone(&card.displayed_usage),
        target_usage: Rc::clone(&card.target_usage),
        details: card.details.downgrade(),
    }
}

struct WeakMetricCard {
    primary: gtk::glib::WeakRef<gtk::Label>,
    secondary: gtk::glib::WeakRef<gtk::Label>,
    progress: gtk::glib::WeakRef<gtk::ProgressBar>,
    chart: gtk::glib::WeakRef<gtk::DrawingArea>,
    history: Rc<RefCell<VecDeque<f64>>>,
    displayed_usage: Rc<Cell<f64>>,
    target_usage: Rc<Cell<f64>>,
    details: gtk::glib::WeakRef<gtk::Label>,
}

fn upgrade_card(card: &WeakMetricCard) -> Option<MetricCard> {
    Some(MetricCard {
        primary: card.primary.upgrade()?,
        secondary: card.secondary.upgrade()?,
        progress: card.progress.upgrade()?,
        chart: card.chart.upgrade()?,
        history: Rc::clone(&card.history),
        displayed_usage: Rc::clone(&card.displayed_usage),
        target_usage: Rc::clone(&card.target_usage),
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
    storage_card: &MetricCard,
    process_card: &MetricCard,
    detail_labels: &BTreeMap<String, gtk::Label>,
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
        "{} logische CPU's · Gemiddelde klok: {}\nPakketvermogen: {}",
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
        let temperatures = system.temperatures.iter().take(3).map(|reading| format!("{}: {:.1} °C", reading.label, reading.celsius)).collect::<Vec<_>>();
        let fans = system.fans.iter().take(3).map(|reading| format!("{}: {} RPM", reading.label, reading.rpm)).collect::<Vec<_>>();
        sensors_card.details.set_text(&temperatures.into_iter().chain(fans).collect::<Vec<_>>().join("\n"));
    }
    sensors_card.set_usage(None);

    let filesystems = &snapshot.storage.filesystems;
    let total_capacity = filesystems.iter().filter_map(|fs| fs.total_bytes).sum::<u64>();
    let total_used = filesystems.iter().filter_map(|fs| fs.used_bytes).sum::<u64>();
    if filesystems.is_empty() {
        storage_card.primary.set_text("Geen bestandssystemen");
        storage_card.secondary.set_text("Opslaginformatie niet beschikbaar");
    } else {
        storage_card.primary.set_text(&format!("{} bestandssystemen", filesystems.len()));
        storage_card.secondary.set_text(&format!("{} gebruikt van {}", format_bytes(total_used), format_bytes(total_capacity)));
    }
    let mounts = filesystems.iter().take(4).map(|fs| {
        let used = fs.used_bytes.zip(fs.total_bytes.filter(|total| *total > 0))
            .map(|(used,total)| format!("{:.0}%", used as f64 / total as f64 * 100.0))
            .unwrap_or_else(|| "N/B".to_owned());
        format!("{} · {} · {} / {} ({})", fs.mountpoint, fs.filesystem,
            fs.used_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
            fs.total_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()), used)
    }).collect::<Vec<_>>();
    // Keep the overview list stable: do not hide a disk just because its
    // current I/O rate is zero. Device-mapper and loop nodes duplicate the
    // underlying physical storage and make the preview noisy.
    let io = snapshot.storage.devices.iter().filter(|device|
        !device.name.starts_with("dm-")
            && !device.name.starts_with("loop")
            && !device.name.starts_with("ram")
            && !device.name.starts_with("zram")
    ).take(3).map(|device| format!("{} · lezen {} · schrijven {}", device.name,
        device.read_bytes_per_second.map(format_rate).unwrap_or_else(|| "0 B/s".to_owned()),
        device.write_bytes_per_second.map(format_rate).unwrap_or_else(|| "0 B/s".to_owned())
    )).collect::<Vec<_>>();
    storage_card.details.set_text(&mounts.into_iter().chain(io).collect::<Vec<_>>().join("\n"));
    storage_card.set_usage(None);

    let processes = &snapshot.processes.processes;
    process_card.primary.set_text(&format!("{} processen", processes.len()));
    process_card.secondary.set_text("Gerangschikt op CPU-gebruik");
    let process_lines = processes.iter().take(6).map(|process| format!(
        "{:>6}  {:>5}  {:>7}  {}",
        process.pid,
        process.cpu_percent.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "N/B".to_owned()),
        process.memory_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        process.name
    )).collect::<Vec<_>>();
    process_card.details.set_text(&if process_lines.is_empty() {
        "Geen processen beschikbaar".to_owned()
    } else {
        format!(" PID    CPU    RAM    Proces\n{}", process_lines.join("\n"))
    });
    process_card.set_usage(None);

    match &snapshot.network {
        Some(network) if !network.interfaces.is_empty() => {
            let active = network.interfaces.iter().filter(|interface| interface.carrier == Some(true) || interface.operational_state.as_deref() == Some("up")).collect::<Vec<_>>();
            let rx = active.iter().filter_map(|interface| interface.rx_bytes_per_second).sum::<f64>();
            let tx = active.iter().filter_map(|interface| interface.tx_bytes_per_second).sum::<f64>();
            network_card.primary.set_text(&format!("↓ {}  ↑ {}", format_rate(rx), format_rate(tx)));
            network_card.secondary.set_text(&format!("{} interfaces · {} actief", network.interfaces.len(), active.len()));
            network_card.details.set_text(&active.iter().take(3).map(|interface| {
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

    let cpu_rows = snapshot.cpu.logical_cpus.iter().map(|core| {
        let usage = match core.utilization {
            cpu::CpuUtilization::Available(v) => format!("{v:5.1}%"),
            cpu::CpuUtilization::WarmingUp => " meten".to_owned(),
            cpu::CpuUtilization::Unavailable => "   N/B".to_owned(),
        };
        format!("CPU{:02}  {}  {:>9}  package={} core={}",
            core.id, usage,
            core.current_frequency_hz.map(|hz| format!("{:.0} MHz", hz as f64 / 1_000_000.0)).unwrap_or_else(|| "N/B".to_owned()),
            core.package_id.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
            core.core_id.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()))
    }).collect::<Vec<_>>().join("\n");
    set_detail(detail_labels, "cpu", &format!(
        "Model: {}\nOnline CPU's: {}\nTotale belasting: {}\nGemiddelde klok: {}\nPakketvermogen:\n{}\n\nPer-logische processor\nID     Gebruik  Frequentie  Topologie\n{}",
        snapshot.cpu.model.as_deref().unwrap_or("N/B"),
        snapshot.cpu.online.as_deref().unwrap_or("N/B"),
        format_cpu_utilization(snapshot.cpu.utilization),
        average_cpu_frequency(&snapshot.cpu),
        if package_power.is_empty() { "N/B".to_owned() } else { package_power.join("\n") },
        cpu_rows
    ));

    let gpu_text = snapshot.gpu.as_ref().map(|gpu| gpu.devices.iter().enumerate().map(|(i, d)| format!(
        "GPU {i}: {}\nDRM: {} · PCI: {}\nDriver: {} · Telemetrie: {}\nGPU-belasting: {} · Geheugenbelasting: {}\nVRAM: {} / {}\nTemperatuur: {} · Vermogen: {} / limiet {}\nCore clock: {} MHz · Memory clock: {} MHz\nVentilator: {} · Encoder: {} · Decoder: {}\nUUID: {}\n",
        d.model.as_deref().unwrap_or("N/B"),
        d.drm_card.as_deref().unwrap_or("N/B"), d.pci_address.as_deref().unwrap_or("N/B"),
        d.driver.as_deref().unwrap_or("N/B"), d.telemetry_source.unwrap_or("N/B"),
        d.gpu_busy_percent.map(|v| format!("{v}%")).unwrap_or_else(|| "N/B".to_owned()),
        d.memory_busy_percent.map(|v| format!("{v}%")).unwrap_or_else(|| "N/B".to_owned()),
        d.vram_used_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        d.vram_total_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        d.temperature_celsius.map(|v| format!("{v} °C")).unwrap_or_else(|| "N/B".to_owned()),
        d.power_milliwatts.map(|v| format!("{:.1} W", v as f64 / 1000.0)).unwrap_or_else(|| "N/B".to_owned()),
        d.power_limit_milliwatts.map(|v| format!("{:.1} W", v as f64 / 1000.0)).unwrap_or_else(|| "N/B".to_owned()),
        d.graphics_clock_mhz.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
        d.memory_clock_mhz.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
        d.fan_percent.map(|v| format!("{v}%")).unwrap_or_else(|| "N/B".to_owned()),
        d.encoder_busy_percent.map(|v| format!("{v}%")).unwrap_or_else(|| "N/B".to_owned()),
        d.decoder_busy_percent.map(|v| format!("{v}%")).unwrap_or_else(|| "N/B".to_owned()),
        d.uuid.as_deref().unwrap_or("N/B")
    )).collect::<Vec<_>>().join("\n")).unwrap_or_else(|| "GPU-telemetrie niet beschikbaar".to_owned());
    set_detail(detail_labels, "gpu", &gpu_text);

    let m = snapshot.memory;
    set_detail(detail_labels, "memory", &format!(
        "RAM totaal: {}\nRAM gebruikt: {}\nRAM beschikbaar: {}\nRAM vrij: {}\nBuffers: {}\nCache: {}\nSwap totaal: {}\nSwap gebruikt: {}\nSwap vrij: {}\nHugePages totaal/vrij: {} / {}\nHugePage-grootte: {}\nMemory PSI some: {}\nMemory PSI full: {}",
        m.total_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.used_bytes().map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.available_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.free_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.buffers_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.cached_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.swap_total_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.swap_used_bytes().map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.swap_free_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.huge_pages_total.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
        m.huge_pages_free.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
        m.huge_page_size_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        m.pressure.map(|p| format!("avg10 {:.2}% · avg60 {:.2}% · avg300 {:.2}%", p.some.avg10,p.some.avg60,p.some.avg300)).unwrap_or_else(|| "N/B".to_owned()),
        m.pressure.and_then(|p| p.full).map(|p| format!("avg10 {:.2}% · avg60 {:.2}% · avg300 {:.2}%", p.avg10,p.avg60,p.avg300)).unwrap_or_else(|| "N/B".to_owned())
    ));

    let sensor_text = snapshot.system.temperatures.iter().map(|v| format!("TEMP  {:<40} {:>7.1} °C", v.label, v.celsius))
        .chain(snapshot.system.fans.iter().map(|v| format!("FAN   {:<40} {:>7} RPM", v.label, v.rpm)))
        .collect::<Vec<_>>().join("\n");
    set_detail(detail_labels, "motherboard", &if sensor_text.is_empty() { "Geen hwmon-sensoren beschikbaar".to_owned() } else { sensor_text });

    let storage_text = snapshot.storage.filesystems.iter().map(|fs| format!(
        "MOUNT {:<24} {:<8} {} / {} · beschikbaar {} · {}",
        fs.mountpoint, fs.filesystem,
        fs.used_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        fs.total_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        fs.available_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()), fs.device
    )).chain(snapshot.storage.devices.iter().map(|d| format!(
        "DISK  {:<16} lezen {:>12} · schrijven {:>12} · reads {} · writes {}",
        d.name,
        d.read_bytes_per_second.map(format_rate).unwrap_or_else(|| "N/B".to_owned()),
        d.write_bytes_per_second.map(format_rate).unwrap_or_else(|| "N/B".to_owned()),
        d.reads_completed.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
        d.writes_completed.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned())
    ))).collect::<Vec<_>>().join("\n");
    set_detail(detail_labels, "storage", &storage_text);

    let process_text = snapshot.processes.processes.iter().map(|p| format!(
        "{:>7}  {:<28} {:>8}  {:>12}  {}",
        p.pid, p.name,
        p.cpu_percent.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "N/B".to_owned()),
        p.memory_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        p.state.map(|v| v.to_string()).unwrap_or_else(|| "?".to_owned())
    )).collect::<Vec<_>>().join("\n");
    set_detail(detail_labels, "processes", &format!("PID     Proces                         CPU       Geheugen      Status\n{}", process_text));

    let network_text = snapshot.network.as_ref().map(|n| n.interfaces.iter().map(|i| format!(
        "{}\n  Status: {} · carrier: {} · link: {} · duplex: {}\n  RX: {} · TX: {}\n  Totaal RX: {} · TX: {}\n  Errors RX/TX: {} / {} · Drops RX/TX: {} / {}",
        i.name,
        i.operational_state.as_deref().unwrap_or("N/B"),
        i.carrier.map(|v| if v { "ja" } else { "nee" }).unwrap_or("N/B"),
        i.speed_mbps.map(|v| format!("{v} Mbps")).unwrap_or_else(|| "N/B".to_owned()),
        i.duplex.as_deref().unwrap_or("N/B"),
        i.rx_bytes_per_second.map(format_rate).unwrap_or_else(|| "N/B".to_owned()),
        i.tx_bytes_per_second.map(format_rate).unwrap_or_else(|| "N/B".to_owned()),
        i.rx_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        i.tx_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        i.rx_errors.unwrap_or(0), i.tx_errors.unwrap_or(0), i.rx_dropped.unwrap_or(0), i.tx_dropped.unwrap_or(0)
    )).collect::<Vec<_>>().join("\n")).unwrap_or_else(|| "Netwerkgegevens niet beschikbaar".to_owned());
    set_detail(detail_labels, "network", &network_text);

    set_detail(detail_labels, "system", &format!(
        "Uptime: {}\nLoad average (1 / 5 / 15 min): {}",
        snapshot.system.uptime_seconds.map(|v| format!("{v} seconden")).unwrap_or_else(|| "N/B".to_owned()),
        snapshot.system.load_average.map(|v| format!("{:.2} · {:.2} · {:.2}", v[0],v[1],v[2])).unwrap_or_else(|| "N/B".to_owned())
    ));
    }
}

fn set_detail(labels: &BTreeMap<String, gtk::Label>, key: &str, value: &str) {
    if let Some(label) = labels.get(key) { label.set_text(value); }
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
