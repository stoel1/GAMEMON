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
    animation_active: Rc<Cell<bool>>,
    network_history: Rc<RefCell<VecDeque<(f64, f64)>>>,
    overview_data: Rc<RefCell<Vec<(String, f64)>>>,
    chart: gtk::DrawingArea,
    details: gtk::Box,
}

impl MetricCard {
    fn new(title: &str, subtitle: &str) -> (gtk::Frame, Self) {
        let container = gtk::Box::new(Orientation::Vertical, 10);
        container.set_margin_top(18);
        container.set_margin_bottom(18);
        container.set_margin_start(18);
        container.set_margin_end(18);
        container.set_vexpand(false);
        container.set_hexpand(true);

        let heading = gtk::Label::new(Some(title));
        heading.set_halign(Align::Start);
        heading.set_single_line_mode(true);
        heading.set_ellipsize(gtk::pango::EllipsizeMode::End);
        heading.set_height_request(22);
        heading.add_css_class("heading");
        heading.add_css_class("dim-label");

        let primary = gtk::Label::new(Some("Meten…"));
        primary.set_halign(Align::Start);
        primary.set_single_line_mode(true);
        primary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        primary.set_height_request(44);
        primary.set_max_width_chars(34);
        primary.add_css_class("title-1");
        primary.add_css_class("numeric");

        let secondary = gtk::Label::new(Some(subtitle));
        secondary.set_halign(Align::Start);
        secondary.set_single_line_mode(true);
        secondary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        secondary.set_height_request(22);
        secondary.set_max_width_chars(72);
        secondary.set_tooltip_text(Some(subtitle));
        secondary.add_css_class("dim-label");

        let progress = gtk::ProgressBar::new();
        progress.set_show_text(false);
        progress.set_fraction(0.0);
        progress.add_css_class("gamemon-progress");

        let history = Rc::new(RefCell::new(VecDeque::<f64>::with_capacity(60)));
        let chart = gtk::DrawingArea::new();
        chart.set_content_height(120);
        chart.set_size_request(-1, 120);
        chart.set_hexpand(true);
        chart.set_vexpand(false);
        chart.add_css_class("gamemon-sparkline");
        let displayed_usage = Rc::new(Cell::new(0.0));
        let target_usage = Rc::new(Cell::new(0.0));
        let animation_active = Rc::new(Cell::new(false));
        let network_history = Rc::new(RefCell::new(VecDeque::<(f64, f64)>::with_capacity(600)));
        let overview_data = Rc::new(RefCell::new(Vec::<(String, f64)>::new()));
        let chart_history = Rc::clone(&history);
        let chart_displayed = Rc::clone(&displayed_usage);
        chart.set_draw_func(move |_, context, width, height| {
            let values = chart_history.borrow();
            if values.is_empty() || width <= 1 || height <= 1 {
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
            // Overview cards show utilization percentages: keep the Y-axis
            // anchored at 0%, matching the detailed processor charts.
            let scale_min = 0.0;
            let scale_max = (maximum + padding).min(100.0).max(1.0);
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
                // Use the full 60-sample time window even while it is filling.
                // This makes the trace grow from left to right instead of stretching
                // the first two samples across the entire chart.
                let x = left + index as f64 / 59.0 * plot_width;
                let normalized = ((*value).clamp(scale_min, scale_max) - scale_min) / scale_span;
                let y = bottom - normalized * plot_height;
                (x, y)
            }).collect::<Vec<_>>();

            context.move_to(coordinates[0].0, bottom);
            context.line_to(coordinates[0].0, coordinates[0].1);
            append_smooth_line(context, &coordinates);
            context.line_to(coordinates.last().unwrap().0, bottom);
            context.close_path();
            context.set_source_rgba(0.25, 0.68, 0.91, 0.12);
            let _ = context.fill();

            context.set_line_width(2.2);
            context.set_line_cap(gtk::cairo::LineCap::Round);
            context.set_line_join(gtk::cairo::LineJoin::Round);
            context.set_source_rgba(0.35, 0.72, 0.95, 1.0);
            context.move_to(coordinates[0].0, coordinates[0].1);
            append_smooth_line(context, &coordinates);
            let _ = context.stroke();

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

        });

        let details = gtk::Box::new(Orientation::Vertical, 5);
        details.set_hexpand(true);
        details.set_vexpand(false);
        details.set_size_request(-1, 66);
        // Keep a fixed three-row widget structure. Updating label text instead
        // of recreating rows prevents GTK from renegotiating card sizes each poll.
        for _ in 0..3 {
            let row = gtk::Box::new(Orientation::Horizontal, 8);
            row.set_hexpand(true);
            row.set_halign(Align::Fill);
            row.set_size_request(-1, 20);
            let label = gtk::Label::new(None);
            label.set_halign(Align::Start);
            label.set_xalign(0.0);
            label.set_single_line_mode(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_hexpand(true);
            label.add_css_class("dim-label");
            let value = gtk::Label::new(None);
            value.set_halign(Align::End);
            value.set_xalign(1.0);
            value.set_single_line_mode(true);
            value.set_ellipsize(gtk::pango::EllipsizeMode::End);
            value.set_max_width_chars(32);
            value.add_css_class("numeric");
            row.append(&label);
            row.append(&value);
            details.append(&row);
        }
        set_metric_details(&details, "Gegevens worden verzameld…");

        container.append(&heading);
        container.append(&primary);
        container.append(&secondary);
        container.append(&progress);
        container.append(&chart);
        container.append(&details);

        let frame = gtk::Frame::new(None);
        frame.add_css_class("card");
        frame.add_css_class("gamemon-metric-card");
        frame.set_child(Some(&container));
        frame.set_hexpand(true);
        frame.set_vexpand(false);
        frame.set_valign(Align::Start);
        let initial_height = match title {
            "SYSTEEM" => 330,
            "SENSOREN" => 350,
            "GEHEUGEN" => 360,
            "PROCESSEN" => 380,
            "PROCESSOR" => 390,
            "GRAPHICS" => 410,
            "NETWERK" => 430,
            "OPSLAG" => 450,
            _ => 330,
        };
        frame.set_size_request(-1, initial_height);
        frame.set_height_request(initial_height);
        container.set_size_request(-1, initial_height - 36);
        details.set_valign(Align::Start);

        // Sleepbare onderrand om de kaart individueel hoger of lager te maken.
        let resize_grip = gtk::Button::with_label("⠿  Sleep om kaart te vergroten/verkleinen");
        resize_grip.set_halign(Align::End);
        resize_grip.set_valign(Align::End);
        resize_grip.set_has_frame(false);
        resize_grip.add_css_class("flat");
        resize_grip.add_css_class("dim-label");
        resize_grip.add_css_class("caption");
        resize_grip.set_margin_top(6);
        resize_grip.set_cursor_from_name(Some("ns-resize"));
        resize_grip.set_tooltip_text(Some("Sleep omhoog of omlaag om de kaart hoger of lager te maken"));
        container.append(&resize_grip);

        // Attach the drag controller to a real button hit target. A passive
        // label can lose pointer events to surrounding widgets in GTK layouts.
        let drag = gtk::GestureDrag::new();
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let start_height = Rc::new(Cell::new(initial_height));
        let start_for_begin = Rc::clone(&start_height);
        let weak_frame = frame.downgrade();
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(frame) = weak_frame.upgrade() {
                let height = frame.height();
                start_for_begin.set(if height > 0 { height } else { initial_height });
            }
        });
        let start_for_update = Rc::clone(&start_height);
        let weak_frame = frame.downgrade();
        let weak_container = container.downgrade();
        drag.connect_drag_update(move |_, _, offset_y| {
            let height = (start_for_update.get() + offset_y.round() as i32).clamp(260, 900);
            if let Some(frame) = weak_frame.upgrade() {
                frame.set_size_request(-1, height);
                frame.queue_resize();
            }
            if let Some(container) = weak_container.upgrade() {
                container.set_size_request(-1, height - 36);
                container.queue_resize();
            }
        });
        resize_grip.add_controller(drag);

        (
            frame,
            Self {
                primary,
                secondary,
                progress,
                history,
                displayed_usage,
                target_usage,
                animation_active,
                network_history,
                overview_data,
                chart,
                details,
            },
        )
    }

    fn configure_summary_chart(&self, kind: &'static str) {
        self.progress.set_visible(false);
        let height = match kind {
            "storage" => 170,
            "sensors" | "processes" => 145,
            _ => 112,
        };
        self.chart.set_content_height(height);
        self.chart.set_size_request(-1, height);
        let data = Rc::clone(&self.overview_data);
        self.chart.set_draw_func(move |_, context, width, height| {
            let values = data.borrow();
            if values.is_empty() || width < 160 || height < 70 { return; }

            let palette = [
                (0.26, 0.72, 0.96),
                (0.25, 0.82, 0.58),
                (0.95, 0.68, 0.30),
                (0.70, 0.55, 0.95),
                (0.90, 0.48, 0.48),
            ];
            context.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
            context.set_font_size(11.0);

            if kind == "storage" {
                let total = values.iter().map(|(_, value)| (*value).max(0.0)).sum::<f64>();
                if total <= 0.0 { return; }
                let center_x = 76.0;
                let center_y = f64::from(height) / 2.0;
                let radius = 57.0;
                let mut angle = -std::f64::consts::FRAC_PI_2;
                for (index, (_, value)) in values.iter().enumerate() {
                    let fraction = (*value).max(0.0) / total;
                    if fraction <= 0.0 { continue; }
                    let next = angle + fraction * std::f64::consts::TAU;
                    let (red, green, blue) = palette[index % palette.len()];
                    context.move_to(center_x, center_y);
                    context.arc(center_x, center_y, radius, angle, next);
                    context.close_path();
                    context.set_source_rgb(red, green, blue);
                    let _ = context.fill();
                    angle = next;
                }
                context.set_source_rgb(0.20, 0.20, 0.20);
                context.arc(center_x, center_y, 31.0, 0.0, std::f64::consts::TAU);
                let _ = context.fill();
                context.set_source_rgb(0.94, 0.94, 0.94);
                context.set_font_size(11.0);
                context.move_to(center_x - 18.0, center_y + 4.0);
                let _ = context.show_text("SSD");

                for (index, (label, value)) in values.iter().enumerate() {
                    let y = 37.0 + index as f64 * 28.0;
                    let (red, green, blue) = palette[index % palette.len()];
                    context.set_source_rgb(red, green, blue);
                    context.rectangle(155.0, y - 9.0, 9.0, 9.0);
                    let _ = context.fill();
                    context.set_source_rgba(0.82, 0.83, 0.85, 1.0);
                    context.move_to(174.0, y);
                    let _ = context.show_text(label);
                    context.move_to((f64::from(width) - 100.0).max(260.0), y);
                    let _ = context.show_text(&format!("{:.1}%", value / total * 100.0));
                }
                return;
            }

            let max_value = values.iter().map(|(_, value)| (*value).max(0.0)).fold(0.0_f64, f64::max).max(1.0);
            let row_height = (f64::from(height) - 12.0) / values.len() as f64;
            let label_width = if kind == "sensors" { 150.0 } else { 125.0 };
            let bar_left = label_width;
            let bar_right = (f64::from(width) - 58.0).max(bar_left + 30.0);
            for (index, (label, value)) in values.iter().enumerate() {
                let y = 8.0 + index as f64 * row_height;
                let (red, green, blue) = palette[index % palette.len()];
                context.set_source_rgba(0.72, 0.74, 0.77, 0.9);
                context.move_to(0.0, y + row_height * 0.68);
                let _ = context.show_text(label);
                context.set_source_rgba(0.38, 0.40, 0.43, 0.65);
                context.rectangle(bar_left, y + 3.0, bar_right - bar_left, 9.0);
                let _ = context.fill();
                context.set_source_rgb(red, green, blue);
                context.rectangle(bar_left, y + 3.0, (bar_right - bar_left) * ((*value).max(0.0) / max_value).clamp(0.0, 1.0), 9.0);
                let _ = context.fill();
                context.set_source_rgba(0.86, 0.87, 0.89, 1.0);
                context.move_to(bar_right + 8.0, y + row_height * 0.68);
                let formatted = if kind == "sensors" { format!("{value:.1}°") } else { format!("{value:.2}") };
                let _ = context.show_text(&formatted);
            }
        });
    }

    fn configure_network_chart(&self) {
        self.progress.set_visible(false);
        self.chart.set_vexpand(true);
        self.chart.set_content_height(190);
        self.chart.set_size_request(-1, 170);
        if let Some(parent) = self.chart.parent() {
            parent.set_vexpand(true);
        }

        let history = Rc::clone(&self.network_history);
        self.chart.set_draw_func(move |_, context, width, height| {
            if width < 120 || height < 90 {
                return;
            }
            let samples = history.borrow();
            if samples.is_empty() {
                return;
            }

            let width = f64::from(width);
            let height = f64::from(height);
            let left = 58.0;
            let right = width - 12.0;
            let top = 30.0;
            let bottom = height - 22.0;
            let plot_width = right - left;
            let plot_height = bottom - top;
            if plot_width <= 1.0 || plot_height <= 1.0 {
                return;
            }

            let peak = samples.iter()
                .map(|(rx, tx)| (*rx).max(*tx))
                .fold(0.0_f64, f64::max);
            let scale_max = if peak < 1.0 { 1.0 } else {
                let raw = peak * 1.15;
                let magnitude = 10_f64.powf(raw.log10().floor());
                (raw / magnitude).ceil() * magnitude
            };

            context.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
            context.set_font_size(10.0);
            for tick in 0..=3 {
                let fraction = tick as f64 / 3.0;
                let y = bottom - fraction * plot_height;
                context.set_line_width(1.0);
                context.set_source_rgba(0.65, 0.68, 0.72, 0.18);
                context.move_to(left, y);
                context.line_to(right, y);
                let _ = context.stroke();

                context.set_source_rgba(0.72, 0.74, 0.77, 0.78);
                context.move_to(2.0, y + 3.5);
                let _ = context.show_text(&format!("{:.1}", scale_max * fraction));
            }

            // The history is sampled at 10 Hz; retain 600 points for a rolling
            // 60-second window and spread the samples across the whole plot.
            let count = samples.len();
            let make_points = |series: usize| -> Vec<(f64, f64)> {
                samples.iter().enumerate().map(|(index, (rx, tx))| {
                    let value = if series == 0 { *rx } else { *tx };
                    let x = left + index as f64 / 599.0 * plot_width;
                    let y = bottom - (value / scale_max).clamp(0.0, 1.0) * plot_height;
                    (x, y)
                }).collect()
            };

            let draw_series = |points: &[(f64, f64)], red: f64, green: f64, blue: f64| {
                if points.is_empty() { return; }
                context.move_to(points[0].0, bottom);
                context.line_to(points[0].0, points[0].1);
                append_smooth_line(context, points);
                context.line_to(points.last().unwrap().0, bottom);
                context.close_path();
                context.set_source_rgba(red, green, blue, 0.10);
                let _ = context.fill();

                context.move_to(points[0].0, points[0].1);
                append_smooth_line(context, points);
                context.set_line_width(2.2);
                context.set_line_cap(gtk::cairo::LineCap::Round);
                context.set_line_join(gtk::cairo::LineJoin::Round);
                context.set_source_rgba(red, green, blue, 0.95);
                let _ = context.stroke();
            };

            let download = make_points(0);
            let upload = make_points(1);
            draw_series(&download, 0.26, 0.72, 0.96);
            draw_series(&upload, 0.25, 0.82, 0.58);

            // Compact legend and rolling-window labels.
            context.set_font_size(11.0);
            context.set_source_rgba(0.26, 0.72, 0.96, 1.0);
            context.move_to(left, 14.0);
            let _ = context.show_text("↓ DOWNLOAD");
            context.set_source_rgba(0.25, 0.82, 0.58, 1.0);
            context.move_to(left + 105.0, 14.0);
            let _ = context.show_text("↑ UPLOAD");
            context.set_font_size(10.0);
            context.set_source_rgba(0.72, 0.74, 0.77, 0.78);
            context.move_to(left, height - 4.0);
            let _ = context.show_text("60 seconden geleden");
            context.move_to(right - 18.0, height - 4.0);
            let _ = context.show_text("nu");
            let _ = count;
        });
    }

    fn start_animation_if_needed(&self) {
        if !self.chart.is_mapped() || self.animation_active.replace(true) {
            return;
        }

        let weak_progress = self.progress.downgrade();
        let weak_chart = self.chart.downgrade();
        let displayed = Rc::clone(&self.displayed_usage);
        let target = Rc::clone(&self.target_usage);
        let active = Rc::clone(&self.animation_active);
        let previous_frame_time = Cell::new(None::<i64>);

        // GTK's frame clock follows the refresh rate of the monitor currently
        // displaying this widget. The callback exists only while a value animates.
        self.chart.add_tick_callback(move |_, frame_clock| {
            let (Some(progress), Some(chart)) = (weak_progress.upgrade(), weak_chart.upgrade()) else {
                active.set(false);
                return gtk::glib::ControlFlow::Break;
            };

            let frame_time = frame_clock.frame_time();
            let delta_seconds = previous_frame_time
                .replace(Some(frame_time))
                .map(|previous| ((frame_time - previous).max(0) as f64 / 1_000_000.0).min(0.05))
                .unwrap_or(1.0 / 60.0);

            let current = displayed.get();
            let destination = target.get();
            let difference = destination - current;
            let alpha = 1.0 - (-delta_seconds / 0.135).exp();
            let next = if difference.abs() < 0.05 {
                destination
            } else {
                current + difference * alpha
            };
            displayed.set(next);

            if (next - current).abs() > 0.001 {
                progress.set_fraction(next / 100.0);
                chart.queue_draw();
                gtk::glib::ControlFlow::Continue
            } else {
                displayed.set(destination);
                progress.set_fraction(destination / 100.0);
                chart.queue_draw();
                active.set(false);
                gtk::glib::ControlFlow::Break
            }
        });
    }

    fn set_usage(&self, value: Option<f64>) {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            self.chart.set_visible(true);
            let value = value.clamp(0.0, 100.0);
            self.target_usage.set(value);
            self.start_animation_if_needed();
            let mut history = self.history.borrow_mut();
            if history.len() == 60 {
                history.pop_front();
            }
            history.push_back(value);
            drop(history);
            if self.chart.is_mapped() {
                self.chart.queue_draw();
            }
        } else {
            self.target_usage.set(0.0);
            self.start_animation_if_needed();
            // Keep the chart allocated even when telemetry is temporarily unavailable.
            self.chart.set_visible(true);
        }
    }
}

pub fn activate(application: &Application) {
    install_styles();
    let (cpu_frame, cpu_card) = MetricCard::new("PROCESSOR", "Totale processorbelasting");
    let (gpu_frame, gpu_card) = MetricCard::new("GRAPHICS", "Grafische processor");
    let (memory_frame, memory_card) = MetricCard::new("GEHEUGEN", "RAM-gebruik");
    let (network_frame, network_card) = MetricCard::new("NETWERK", "Actuele netwerkdoorvoer");
    network_card.configure_network_chart();
    let (system_frame, system_card) = MetricCard::new("SYSTEEM", "Uptime en load average");
    let (sensors_frame, sensors_card) = MetricCard::new("SENSOREN", "Temperaturen en ventilatoren");
    let (storage_frame, storage_card) = MetricCard::new("OPSLAG", "Bestandssystemen en schijfactiviteit");
    let (process_frame, process_card) = MetricCard::new("PROCESSEN", "Actieve processen · CPU en geheugen");
    system_card.configure_summary_chart("system");
    sensors_card.configure_summary_chart("sensors");
    storage_card.configure_summary_chart("storage");
    process_card.configure_summary_chart("processes");

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
    // Keep both dashboard columns exactly the same width. Without a homogeneous
    // allocation, changing label/chart natural widths can make GTK redistribute
    // horizontal space between the columns during telemetry updates.
    dashboard.set_homogeneous(true);
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

    let mut detail_labels: BTreeMap<String, gtk::Box> = BTreeMap::new();
    let mut detail_charts: BTreeMap<String, (gtk::DrawingArea, Rc<RefCell<VecDeque<f64>>>)> = BTreeMap::new();
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
        let data = gtk::Box::new(Orientation::Vertical, 8);
        data.set_hexpand(true);
        let chart_unit = match key {
            "cpu" | "memory" | "processes" | "gpu" => "%",
            "network" | "storage" => "MiB/s",
            "system" => "load",
            "motherboard" => "°C",
            _ => "",
        };
        let (chart, history) = make_detail_chart(chart_unit);
        let chart_card = gtk::Frame::new(None);
        chart_card.add_css_class("card");
        let chart_content = gtk::Box::new(Orientation::Vertical, 6);
        chart_content.set_margin_top(12);
        chart_content.set_margin_bottom(12);
        chart_content.set_margin_start(12);
        chart_content.set_margin_end(12);
        let chart_title = gtk::Label::new(Some("Live trend · laatste 60 metingen"));
        chart_title.set_halign(Align::Start);
        chart_title.add_css_class("heading");
        chart_title.add_css_class("dim-label");
        chart_content.append(&chart_title);
        chart_content.append(&chart);
        chart_card.set_child(Some(&chart_content));
        page.append(&chart_card);
        detail_charts.insert(key.to_owned(), (chart, history));

        // CPU hybrid-core and GPU telemetry get independent trend panels.
        let extra_charts: &[(&str, &str)] = match key {
            "cpu" => &[("cpu_p", "Prestatiekernen (P-cores) · gebruik"), ("cpu_e", "Efficiëntiekernen (E-cores) · gebruik")],
            "gpu" => &[("gpu_power", "GPU-vermogen · watt"), ("gpu_vram", "VRAM-gebruik · GiB")],
            _ => &[],
        };
        for (series_key, series_title) in extra_charts {
            let series_unit = match *series_key {
                "cpu_p" | "cpu_e" => "%",
                "gpu_power" => "W",
                "gpu_vram" => "GiB",
                _ => "",
            };
            let (series_chart, series_history) = make_detail_chart(series_unit);
            let series_card = gtk::Frame::new(None);
            series_card.add_css_class("card");
            let series_content = gtk::Box::new(Orientation::Vertical, 6);
            series_content.set_margin_top(12);
            series_content.set_margin_bottom(12);
            series_content.set_margin_start(12);
            series_content.set_margin_end(12);
            let series_heading = gtk::Label::new(Some(series_title));
            series_heading.set_halign(Align::Start);
            series_heading.add_css_class("heading");
            series_heading.add_css_class("dim-label");
            series_content.append(&series_heading);
            series_content.append(&series_chart);
            series_card.set_child(Some(&series_content));
            page.append(&series_card);
            detail_charts.insert((*series_key).to_owned(), (series_chart, series_history));
        }
        let loading = gtk::Label::new(Some("Gegevens worden verzameld…"));
        loading.set_halign(Align::Start);
        loading.add_css_class("dim-label");
        data.append(&loading);
        page.append(&title);
        page.append(&subtitle);
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
    let history_store = crate::core::history::HistoryStore::new().map(Rc::new);
    if let Some(history) = history_store.clone() {
        let export_button = gtk::Button::with_label("Exporteer CSV");
        export_button.set_tooltip_text(Some("Kopieer de meetgeschiedenis naar Downloads"));
        export_button.connect_clicked(move |_| {
            let destination = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|home| home.join("Downloads/gamemon-history.csv"));
            if let Some(destination) = destination {
                match history.export_csv(&destination) {
                    Ok(_) => tracing::info!(path = %destination.display(), "monitoring history exported"),
                    Err(error) => tracing::warn!(%error, "could not export monitoring history"),
                }
            }
        });
        header.pack_end(&export_button);
    }


    let cpu_card = downgrade_card(cpu_card);
    let gpu_card = downgrade_card(gpu_card);
    let memory_card = downgrade_card(memory_card);
    let network_card = downgrade_card(network_card);
    let system_card = downgrade_card(system_card);
    let sensors_card = downgrade_card(sensors_card);
    let storage_card = downgrade_card(storage_card);
    let process_card = downgrade_card(process_card);

    let mut detail_expanded: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();

    let history_for_timer = history_store.clone();
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
            if let Some(history) = &history_for_timer { history.append(&snapshot); }
            let visible_page = stack.visible_child_name()
                .map(|name| name.to_string())
                .unwrap_or_else(|| "overview".to_owned());
            render_snapshot(&snapshot, &cpu_card, &gpu_card, &memory_card, &network_card, &system_card, &sensors_card, &storage_card, &process_card, &detail_labels, &mut detail_expanded, &detail_charts, &visible_page);
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
        animation_active: Rc::clone(&card.animation_active),
        network_history: Rc::clone(&card.network_history),
        overview_data: Rc::clone(&card.overview_data),
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
    animation_active: Rc<Cell<bool>>,
    network_history: Rc<RefCell<VecDeque<(f64, f64)>>>,
    overview_data: Rc<RefCell<Vec<(String, f64)>>>,
    details: gtk::glib::WeakRef<gtk::Box>,
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
        animation_active: Rc::clone(&card.animation_active),
        network_history: Rc::clone(&card.network_history),
        overview_data: Rc::clone(&card.overview_data),
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
    detail_labels: &BTreeMap<String, gtk::Box>,
    detail_expanded: &mut BTreeMap<String, BTreeMap<String, bool>>,
    detail_charts: &BTreeMap<String, (gtk::DrawingArea, Rc<RefCell<VecDeque<f64>>>)>,
    visible_page: &str,
) {
    let cpu = &snapshot.cpu;
    let cpu_usage = cpu_utilization_value(cpu.utilization);
    push_detail_sample(detail_charts, "cpu", cpu_usage);
    let average_core_usage = |core_type: u32| -> Option<f64> {
        let values = cpu.logical_cpus.iter().filter(|core| core.core_type == Some(core_type))
            .filter_map(|core| match core.utilization { cpu::CpuUtilization::Available(value) => Some(value), _ => None })
            .collect::<Vec<_>>();
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    };
    // Linux topology core_type: 2 = Core/P-core, 1 = Atom/E-core.
    push_detail_sample(detail_charts, "cpu_p", average_core_usage(2));
    push_detail_sample(detail_charts, "cpu_e", average_core_usage(1));
    set_label_text_if_changed(&cpu_card.primary, &format_cpu_utilization(cpu.utilization));
    cpu_card.set_usage(cpu_usage);
    set_label_text_if_changed(&cpu_card.secondary, cpu.model.as_deref().unwrap_or("CPU-model niet beschikbaar"));
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
    set_metric_details(&cpu_card.details, &format!(
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
            push_detail_sample(detail_charts, "gpu", usage);
            push_detail_sample(detail_charts, "gpu_power", device.power_milliwatts.map(|value| value as f64 / 1000.0));
            push_detail_sample(detail_charts, "gpu_vram", device.vram_used_bytes.map(|value| value as f64 / 1_073_741_824.0));
            set_label_text_if_changed(&gpu_card.primary, &usage.map(|value| format!("{value:.0}%")).unwrap_or_else(|| "N/B".to_owned()));
            gpu_card.set_usage(usage);
            set_label_text_if_changed(&gpu_card.secondary, name);
            let vram = match (device.vram_used_bytes, device.vram_total_bytes) {
                (Some(used), Some(total)) => format!("VRAM {} / {}", format_bytes(used), format_bytes(total)),
                _ => "VRAM niet beschikbaar".to_owned(),
            };
            let temperature = device.temperature_celsius.map(|value| format!("{value} °C")).unwrap_or_else(|| "N/B".to_owned());
            let power = device.power_milliwatts.map(|value| format!("{:.1} W", value as f64 / 1000.0)).unwrap_or_else(|| "N/B".to_owned());
            let clocks = device.graphics_clock_mhz.map(|value| format!("{value} MHz")).unwrap_or_else(|| "N/B".to_owned());
            set_metric_details(&gpu_card.details, &format!("Temperatuur: {temperature} · Vermogen: {power}\n{vram} · Core clock: {clocks}\n{} GPU(s) gedetecteerd", gpu.devices.len()));
        }
        Some(_) => {
            set_label_text_if_changed(&gpu_card.primary, "Geen GPU");
            set_label_text_if_changed(&gpu_card.secondary, "Geen grafische apparaten gevonden");
            set_metric_details(&gpu_card.details, "Controleer of de kernel DRM-apparaten beschikbaar stelt.");
            gpu_card.set_usage(None);
        }
        None => {
            set_label_text_if_changed(&gpu_card.primary, "N/B");
            set_label_text_if_changed(&gpu_card.secondary, "GPU-telemetrie niet beschikbaar");
            set_metric_details(&gpu_card.details, "Er kon geen GPU-informatie worden verzameld.");
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
    set_label_text_if_changed(&memory_card.primary, &memory_percent.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "N/B".to_owned()));
    push_detail_sample(detail_charts, "memory", memory_percent);
    memory_card.set_usage(memory_percent);
    set_label_text_if_changed(&memory_card.secondary, &format!(
        "{} gebruikt van {}",
        used.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        total.map(format_bytes).unwrap_or_else(|| "N/B".to_owned())
    ));
    let swap = match (memory.swap_used_bytes(), memory.swap_total_bytes) {
        (Some(used), Some(total)) => format!("Swap: {} / {}", format_bytes(used), format_bytes(total)),
        _ => "Swap: N/B".to_owned(),
    };
    set_metric_details(&memory_card.details, &format!(
        "Beschikbaar: {} · Vrij: {}\n{}",
        memory.available_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        memory.free_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        swap
    ));

    let system = &snapshot.system;
    let uptime = system.uptime_seconds.map(format_uptime).unwrap_or_else(|| "Niet beschikbaar".to_owned());
    push_detail_sample(detail_charts, "system", system.load_average.map(|v| v[0]));
    let load = system.load_average.map(|values| format!("{:.2} · {:.2} · {:.2}", values[0], values[1], values[2])).unwrap_or_else(|| "Niet beschikbaar".to_owned());
    *system_card.overview_data.borrow_mut() = system.load_average.map(|values| vec![
        ("1 min".to_owned(), values[0]),
        ("5 min".to_owned(), values[1]),
        ("15 min".to_owned(), values[2]),
    ]).unwrap_or_default();
    if system_card.chart.is_mapped() { system_card.chart.queue_draw(); }
    set_label_text_if_changed(&system_card.primary, &uptime);
    set_label_text_if_changed(&system_card.secondary, "Systeem actief");
    set_metric_details(&system_card.details, &format!("Load average (1 / 5 / 15 min): {load}"));
    system_card.set_usage(None);

    let hottest_temperature = system.temperatures.iter().map(|v| v.celsius).reduce(f64::max);
    push_detail_sample(detail_charts, "motherboard", hottest_temperature);
    if system.temperatures.is_empty() && system.fans.is_empty() {
        sensors_card.overview_data.borrow_mut().clear();
        if sensors_card.chart.is_mapped() { sensors_card.chart.queue_draw(); }
        set_label_text_if_changed(&sensors_card.primary, "Geen sensoren");
        set_label_text_if_changed(&sensors_card.secondary, "Geen hwmon-metingen beschikbaar");
        set_metric_details(&sensors_card.details, "Controleer of de kernel temperatuur- en fanwaarden via /sys/class/hwmon exporteert.");
    } else {
        let hottest = system.temperatures.iter().max_by(|left, right| left.celsius.total_cmp(&right.celsius));
        if let Some(reading) = hottest {
            set_label_text_if_changed(&sensors_card.primary, &format!("{:.1} °C", reading.celsius));
            set_label_text_if_changed(&sensors_card.secondary, &format!("Hoogste temperatuur · {}", reading.label));
        } else {
            set_label_text_if_changed(&sensors_card.primary, &format!("{} fans", system.fans.len()));
            set_label_text_if_changed(&sensors_card.secondary, "Ventilatorsnelheden");
        }
        *sensors_card.overview_data.borrow_mut() = system.temperatures.iter()
            .take(4)
            .map(|reading| (reading.label.clone(), reading.celsius))
            .collect();
        if sensors_card.chart.is_mapped() { sensors_card.chart.queue_draw(); }
        let temperatures = system.temperatures.iter().take(3).map(|reading| format!("{}: {:.1} °C", reading.label, reading.celsius)).collect::<Vec<_>>();
        let fans = system.fans.iter().take(3).map(|reading| format!("{}: {} RPM", reading.label, reading.rpm)).collect::<Vec<_>>();
        set_metric_details(&sensors_card.details, &temperatures.into_iter().chain(fans).collect::<Vec<_>>().join("\n"));
    }
    sensors_card.set_usage(None);

    let disk_rate = snapshot.storage.devices.iter().filter_map(|d| d.read_bytes_per_second.zip(d.write_bytes_per_second).map(|(r,w)| r+w)).sum::<f64>() / 1_048_576.0;
    push_detail_sample(detail_charts, "storage", Some(disk_rate));
    let filesystems = &snapshot.storage.filesystems;
    let total_capacity = filesystems.iter().filter_map(|fs| fs.total_bytes).sum::<u64>();
    let total_used = filesystems.iter().filter_map(|fs| fs.used_bytes).sum::<u64>();
    let total_free = total_capacity.saturating_sub(total_used);
    *storage_card.overview_data.borrow_mut() = if total_capacity > 0 {
        vec![
            ("Gebruikt".to_owned(), total_used as f64),
            ("Vrij".to_owned(), total_free as f64),
        ]
    } else { Vec::new() };
    if storage_card.chart.is_mapped() { storage_card.chart.queue_draw(); }
    if filesystems.is_empty() {
        set_label_text_if_changed(&storage_card.primary, "Geen bestandssystemen");
        set_label_text_if_changed(&storage_card.secondary, "Opslaginformatie niet beschikbaar");
    } else {
        set_label_text_if_changed(&storage_card.primary, &format!("{} bestandssystemen", filesystems.len()));
        set_label_text_if_changed(&storage_card.secondary, &format!("{} gebruikt van {}", format_bytes(total_used), format_bytes(total_capacity)));
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
    set_metric_details(&storage_card.details, &mounts.into_iter().chain(io).collect::<Vec<_>>().join("\n"));
    storage_card.set_usage(None);

    let processes = &snapshot.processes.processes;
    let process_cpu = processes.iter().filter_map(|p| p.cpu_percent).sum::<f64>();
    *process_card.overview_data.borrow_mut() = processes.iter()
        .filter_map(|process| process.cpu_percent.map(|cpu| (process.name.clone(), cpu)))
        .take(4)
        .collect();
    if process_card.chart.is_mapped() { process_card.chart.queue_draw(); }
    push_detail_sample(detail_charts, "processes", Some(process_cpu));
    set_label_text_if_changed(&process_card.primary, &format!("{} processen", processes.len()));
    set_label_text_if_changed(&process_card.secondary, "Gerangschikt op CPU-gebruik");
    let process_lines = processes.iter().take(6).map(|process| format!(
        "{:>6}  {:>5}  {:>7}  {}",
        process.pid,
        process.cpu_percent.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "N/B".to_owned()),
        process.memory_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        process.name
    )).collect::<Vec<_>>();
    set_metric_details(&process_card.details, &if process_lines.is_empty() {
        "Geen processen beschikbaar".to_owned()
    } else {
        format!(" PID    CPU    RAM    Proces\n{}", process_lines.join("\n"))
    });
    process_card.set_usage(None);

    match &snapshot.network {
        Some(network) if !network.interfaces.is_empty() => {
            let total_rate = network.interfaces.iter().filter_map(|i| i.rx_bytes_per_second).sum::<f64>()
                + network.interfaces.iter().filter_map(|i| i.tx_bytes_per_second).sum::<f64>();
            push_detail_sample(detail_charts, "network", Some(total_rate / 1_048_576.0));
            let active = network.interfaces.iter().filter(|interface| interface.carrier == Some(true) || interface.operational_state.as_deref() == Some("up")).collect::<Vec<_>>();
            let rx = active.iter().filter_map(|interface| interface.rx_bytes_per_second).sum::<f64>();
            let tx = active.iter().filter_map(|interface| interface.tx_bytes_per_second).sum::<f64>();
            {
                let mut history = network_card.network_history.borrow_mut();
                if history.len() == 600 {
                    history.pop_front();
                }
                history.push_back((
                    rx.max(0.0) * 8.0 / 1_000_000.0,
                    tx.max(0.0) * 8.0 / 1_000_000.0,
                ));
            }
            if network_card.chart.is_mapped() {
                network_card.chart.queue_draw();
            }
            set_label_text_if_changed(
                &network_card.primary,
                &format!("↓ {}  ↑ {}", format_rate(rx), format_rate(tx)),
            );
            set_label_text_if_changed(&network_card.secondary, &format!("{} interfaces · {} actief", network.interfaces.len(), active.len()));
            set_metric_details(&network_card.details, &active.iter().take(3).map(|interface| {
                format!("{} · ↓ {} · ↑ {}", interface.name, format_rate(interface.rx_bytes_per_second.unwrap_or(0.0)), format_rate(interface.tx_bytes_per_second.unwrap_or(0.0)))
            }).collect::<Vec<_>>().join("\n"));
            network_card.set_usage(None);
        }
        Some(_) => {
            {
                let mut history = network_card.network_history.borrow_mut();
                if history.len() == 600 { history.pop_front(); }
                history.push_back((0.0, 0.0));
            }
            set_label_text_if_changed(&network_card.primary, "Geen interfaces");
            set_label_text_if_changed(&network_card.secondary, "Geen netwerkinterfaces gevonden");
            set_metric_details(&network_card.details, "Netwerkdetails zijn niet beschikbaar.");
            network_card.set_usage(None);
        }
        None => {
            {
                let mut history = network_card.network_history.borrow_mut();
                if history.len() == 600 { history.pop_front(); }
                history.push_back((0.0, 0.0));
            }
            set_label_text_if_changed(&network_card.primary, "N/B");
            set_label_text_if_changed(&network_card.secondary, "Netwerktelemetrie niet beschikbaar");
            set_metric_details(&network_card.details, "Er kon geen netwerk-informatie worden verzameld.");
            network_card.set_usage(None);
        }
    }

    let cpu_rows = snapshot.cpu.logical_cpus.iter().map(|core| {
        let usage = match core.utilization {
            cpu::CpuUtilization::Available(v) => format!("{v:.1}%"),
            cpu::CpuUtilization::WarmingUp => "Meten…".to_owned(),
            cpu::CpuUtilization::Unavailable => "N/B".to_owned(),
        };
        format!("CPU{:02}: {} · {} · package {} · core {}",
            core.id, usage,
            core.current_frequency_hz.map(|hz| format!("{:.0} MHz", hz as f64 / 1_000_000.0)).unwrap_or_else(|| "N/B".to_owned()),
            core.package_id.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()),
            core.core_id.map(|v| v.to_string()).unwrap_or_else(|| "N/B".to_owned()))
    }).collect::<Vec<_>>().join("\n");
    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "cpu", &format!(
        "# Processoroverzicht\nModel: {}\nOnline CPU's: {}\nTotale belasting: {}\nGemiddelde klok: {}\n\n# Pakketvermogen\n{}\n\n# Logische processors · {} threads\n{}",
        snapshot.cpu.model.as_deref().unwrap_or("N/B"),
        snapshot.cpu.online.as_deref().unwrap_or("N/B"),
        format_cpu_utilization(snapshot.cpu.utilization),
        average_cpu_frequency(&snapshot.cpu),
        if package_power.is_empty() { "Geen RAPL-gegevens beschikbaar".to_owned() } else { package_power.join("\n") },
        snapshot.cpu.logical_cpus.len(),
        cpu_rows
    ));

    let gpu_text = snapshot.gpu.as_ref().map(|gpu| gpu.devices.iter().enumerate().map(|(i, d)| format!(
        "# GPU {i} · {}\nGPU-belasting: {}\nGeheugenbelasting: {}\nVRAM gebruikt: {}\nVRAM totaal: {}\nTemperatuur: {}\nVermogen: {}\nVermogenslimiet: {}\nCore clock: {} MHz\nMemory clock: {} MHz\nVentilator: {}\nEncoder: {}\nDecoder: {}\n\n# Identificatie\nDRM: {}\nPCI-adres: {}\nDriver: {}\nTelemetriebron: {}\nUUID: {}",
        d.model.as_deref().unwrap_or("Onbekende GPU"),
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
        d.drm_card.as_deref().unwrap_or("N/B"), d.pci_address.as_deref().unwrap_or("N/B"),
        d.driver.as_deref().unwrap_or("N/B"), d.telemetry_source.unwrap_or("N/B"), d.uuid.as_deref().unwrap_or("N/B")
    )).collect::<Vec<_>>().join("\n\n")).unwrap_or_else(|| "# GPU\nGPU-telemetrie niet beschikbaar".to_owned());
    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "gpu", &gpu_text);

    let m = snapshot.memory;
    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "memory", &format!(
        "# RAM\nTotaal: {}\nGebruikt: {}\nBeschikbaar: {}\nVrij: {}\nBuffers: {}\nCache: {}\n\n# Swap\nTotaal: {}\nGebruikt: {}\nVrij: {}\n\n# HugePages\nTotaal: {}\nVrij: {}\nPaginaformaat: {}\n\n# Memory pressure (PSI)\nSome: {}\nFull: {}",
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

    let temperature_text = snapshot.system.temperatures.iter().map(|v| format!("{}: {:.1} °C", v.label, v.celsius)).collect::<Vec<_>>().join("\n");
    let fan_text = snapshot.system.fans.iter().map(|v| format!("{}: {} RPM", v.label, v.rpm)).collect::<Vec<_>>().join("\n");
    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "motherboard", &format!(
        "# Temperatuurmetingen\n{}\n\n# Ventilatoren\n{}",
        if temperature_text.is_empty() { "Geen temperatuurmetingen beschikbaar" } else { &temperature_text },
        if fan_text.is_empty() { "Geen ventilatormetingen beschikbaar" } else { &fan_text }
    ));

    if visible_page == "storage" {
        if let Some(container) = detail_labels.get("storage") {
            set_storage_detail(container, &snapshot.storage);
        }
    }

    let process_text = snapshot.processes.processes.iter().map(|p| format!(
        "PID {} · {}: CPU {} · RAM {} · status {}",
        p.pid, p.name,
        p.cpu_percent.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "N/B".to_owned()),
        p.memory_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        p.state.map(|v| v.to_string()).unwrap_or_else(|| "?".to_owned())
    )).collect::<Vec<_>>().join("\n");
    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "processes", &format!("# Actieve processen · {}\n{}", snapshot.processes.processes.len(), if process_text.is_empty() { "Geen processen beschikbaar" } else { &process_text }));

    let network_text = snapshot.network.as_ref().map(|n| n.interfaces.iter().map(|i| format!(
        "# Interface {}\nStatus: {}\nCarrier: {}\nLinksnelheid: {}\nDuplex: {}\nOntvangen: {}\nVerzonden: {}\nTotaal ontvangen: {}\nTotaal verzonden: {}\nRX-fouten: {}\nTX-fouten: {}\nRX-drops: {}\nTX-drops: {}",
        i.name, i.operational_state.as_deref().unwrap_or("N/B"),
        i.carrier.map(|v| if v { "ja" } else { "nee" }).unwrap_or("N/B"),
        i.speed_mbps.map(|v| format!("{v} Mbps")).unwrap_or_else(|| "N/B".to_owned()),
        i.duplex.as_deref().unwrap_or("N/B"),
        i.rx_bytes_per_second.map(format_rate).unwrap_or_else(|| "N/B".to_owned()),
        i.tx_bytes_per_second.map(format_rate).unwrap_or_else(|| "N/B".to_owned()),
        i.rx_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        i.tx_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()),
        i.rx_errors.unwrap_or(0), i.tx_errors.unwrap_or(0), i.rx_dropped.unwrap_or(0), i.tx_dropped.unwrap_or(0)
    )).collect::<Vec<_>>().join("\n\n")).unwrap_or_else(|| "# Netwerk\nNetwerkgegevens niet beschikbaar".to_owned());
    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "network", &network_text);

    set_detail_if_visible(detail_labels, detail_expanded, visible_page, "system", &format!(
        "# Systeemstatus\nUptime: {}\n\n# Load average\n1 minuut: {}\n5 minuten: {}\n15 minuten: {}",
        snapshot.system.uptime_seconds.map(|v| format_uptime(v)).unwrap_or_else(|| "N/B".to_owned()),
        snapshot.system.load_average.map(|v| format!("{:.2}",v[0])).unwrap_or_else(|| "N/B".to_owned()),
        snapshot.system.load_average.map(|v| format!("{:.2}",v[1])).unwrap_or_else(|| "N/B".to_owned()),
        snapshot.system.load_average.map(|v| format!("{:.2}",v[2])).unwrap_or_else(|| "N/B".to_owned())
    ));
}






fn storage_section(title: &str) -> (gtk::Frame, gtk::Box) {
    let frame = gtk::Frame::new(None);
    frame.add_css_class("card");
    frame.set_hexpand(true);
    let content = gtk::Box::new(Orientation::Vertical, 10);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let heading = gtk::Label::new(Some(title));
    heading.set_halign(Align::Start);
    heading.add_css_class("title-3");
    content.append(&heading);
    frame.set_child(Some(&content));
    (frame, content)
}

fn storage_table_label(text: &str, width: i32, align: Align, strong: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_width_chars(width);
    label.set_max_width_chars(width);
    label.set_halign(align);
    label.set_xalign(if align == Align::End { 1.0 } else { 0.0 });
    label.set_single_line_mode(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_selectable(true);
    if strong { label.add_css_class("heading"); }
    label
}

fn set_storage_detail(container: &gtk::Box, storage: &storage::StorageSnapshot) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let (filesystem_frame, filesystem_content) = storage_section("Bestandssystemen");
    if storage.filesystems.is_empty() {
        let empty = gtk::Label::new(Some("Geen bestandssystemen gevonden"));
        empty.set_halign(Align::Start);
        empty.add_css_class("dim-label");
        filesystem_content.append(&empty);
    } else {
        for filesystem in &storage.filesystems {
            let row = gtk::Box::new(Orientation::Vertical, 5);
            row.set_hexpand(true);
            let top = gtk::Box::new(Orientation::Horizontal, 12);
            let mount = gtk::Label::new(Some(&filesystem.mountpoint));
            mount.set_halign(Align::Start);
            mount.set_hexpand(true);
            mount.set_xalign(0.0);
            mount.add_css_class("heading");
            let fs_type = gtk::Label::new(Some(&filesystem.filesystem));
            fs_type.add_css_class("dim-label");
            let device = gtk::Label::new(Some(&filesystem.device));
            device.add_css_class("dim-label");
            device.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            device.set_max_width_chars(34);
            device.set_tooltip_text(Some(&filesystem.device));
            top.append(&mount);
            top.append(&fs_type);
            top.append(&device);
            row.append(&top);

            let used = filesystem.used_bytes.unwrap_or(0);
            let total = filesystem.total_bytes.unwrap_or(0);
            let available = filesystem.available_bytes.unwrap_or(0);
            let fraction = if total > 0 { (used as f64 / total as f64).clamp(0.0, 1.0) } else { 0.0 };
            let usage = gtk::ProgressBar::new();
            usage.set_fraction(fraction);
            usage.set_show_text(false);
            usage.add_css_class("gamemon-progress");
            row.append(&usage);

            let summary = gtk::Box::new(Orientation::Horizontal, 12);
            let used_text = if filesystem.used_bytes.is_some() && filesystem.total_bytes.is_some() {
                format!("{} gebruikt van {}", format_bytes(used), format_bytes(total))
            } else { "Gebruik niet beschikbaar".to_owned() };
            let used_label = gtk::Label::new(Some(&used_text));
            used_label.set_halign(Align::Start);
            used_label.set_hexpand(true);
            used_label.add_css_class("dim-label");
            let percent = if total > 0 { format!("{:.0}%", fraction * 100.0) } else { "N/B".to_owned() };
            let percent_label = gtk::Label::new(Some(&percent));
            percent_label.add_css_class("numeric");
            let free_label = gtk::Label::new(Some(&format!("Vrij: {}", filesystem.available_bytes.map(format_bytes).unwrap_or_else(|| "N/B".to_owned()))));
            free_label.add_css_class("dim-label");
            summary.append(&used_label);
            summary.append(&percent_label);
            summary.append(&free_label);
            row.append(&summary);
            filesystem_content.append(&row);
        }
    }
    container.append(&filesystem_frame);

    let (disk_frame, disk_content) = storage_section("Schijfactiviteit");
    if storage.devices.is_empty() {
        let empty = gtk::Label::new(Some("Geen schijfstatistieken beschikbaar"));
        empty.set_halign(Align::Start);
        empty.add_css_class("dim-label");
        disk_content.append(&empty);
    } else {
        let header = gtk::Box::new(Orientation::Horizontal, 8);
        header.add_css_class("dim-label");
        header.append(&storage_table_label("Apparaat", 18, Align::Start, true));
        header.append(&storage_table_label("Lezen", 13, Align::End, true));
        header.append(&storage_table_label("Schrijven", 13, Align::End, true));
        header.append(&storage_table_label("Leesbewerkingen", 16, Align::End, true));
        header.append(&storage_table_label("Schrijfbewerkingen", 17, Align::End, true));
        disk_content.append(&header);

        for device in &storage.devices {
            let row = gtk::Box::new(Orientation::Horizontal, 8);
            row.set_hexpand(true);
            row.add_css_class("gamemon-storage-row");
            row.append(&storage_table_label(&device.name, 18, Align::Start, true));
            row.append(&storage_table_label(&device.read_bytes_per_second.map(format_rate).unwrap_or_else(|| "—".to_owned()), 13, Align::End, false));
            row.append(&storage_table_label(&device.write_bytes_per_second.map(format_rate).unwrap_or_else(|| "—".to_owned()), 13, Align::End, false));
            row.append(&storage_table_label(&device.reads_completed.map(|v| format!("{v}")).unwrap_or_else(|| "—".to_owned()), 16, Align::End, false));
            row.append(&storage_table_label(&device.writes_completed.map(|v| format!("{v}")).unwrap_or_else(|| "—".to_owned()), 17, Align::End, false));
            disk_content.append(&row);
        }
    }
    container.append(&disk_frame);
}

fn set_label_text_if_changed(label: &gtk::Label, value: &str) {
    if label.text().as_str() != value {
        label.set_text(value);
    }
}

fn set_metric_details(container: &gtk::Box, text: &str) {
    let lines = text.lines()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>();

    let mut child = container.first_child();
    for index in 0..3 {
        let Some(row_widget) = child else { break };
        child = row_widget.next_sibling();
        let Ok(row) = row_widget.downcast::<gtk::Box>() else { continue };
        let Some(label_widget) = row.first_child() else { continue };
        let Some(value_widget) = label_widget.next_sibling() else { continue };
        let (Ok(label), Ok(value)) = (
            label_widget.downcast::<gtk::Label>(),
            value_widget.downcast::<gtk::Label>(),
        ) else { continue };

        if let Some(line) = lines.get(index) {
            if let Some((label_text, value_text)) = line.split_once(':') {
                if label.text().as_str() != label_text.trim() { label.set_text(label_text.trim()); }
                if value.text().as_str() != value_text.trim() { value.set_text(value_text.trim()); }
                label.set_visible(true);
                value.set_visible(true);
            } else {
                if label.text().as_str() != line.trim() { label.set_text(line.trim()); }
                if !value.text().is_empty() { value.set_text(""); }
                label.set_visible(true);
                value.set_visible(false);
            }
        } else {
            if !label.text().is_empty() { label.set_text(""); }
            if !value.text().is_empty() { value.set_text(""); }
            label.set_visible(false);
            value.set_visible(false);
        }
    }
}

fn install_styles() {
    let Some(display) = gtk::gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(r#"
        progressbar.gamemon-progress {
            min-height: 7px;
            border-radius: 999px;
            background: transparent;
        }
        progressbar.gamemon-progress trough {
            min-height: 7px;
            border: 0;
            border-radius: 999px;
            background: alpha(@theme_fg_color, 0.10);
        }
        progressbar.gamemon-progress progress {
            min-height: 7px;
            border: 0;
            border-radius: 999px;
            background: #59b9f4;
        }
        .gamemon-metric-card {
            border-radius: 14px;
            transition: border-color 180ms ease;
        }
        .gamemon-metric-card:hover {
            border-color: alpha(#59b9f4, 0.55);
        }
        .gamemon-section {
            padding: 4px 2px;
        }
        .gamemon-storage-row {
            padding: 6px 0;
            border-bottom: 1px solid alpha(@theme_fg_color, 0.08);
        }
        .gamemon-storage-row:last-child {
            border-bottom: 0;
        }
        .gamemon-sparkline {
            min-height: 88px;
        }
    "#);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

// Convert each segment to a cubic Bezier curve using a Catmull-Rom
// tangent. This preserves the measured sample positions while removing the
// sharp corners of a straight-line sparkline.
fn append_smooth_line(context: &gtk::cairo::Context, points: &[(f64, f64)]) {
    let Some(&(first_x, first_y)) = points.first() else { return };
    // The caller positions the Cairo path at the first point. Keeping this
    // helper append-only lets the overview fill start at the chart baseline.
    let _ = (first_x, first_y);
    if points.len() < 3 {
        for &(x, y) in points.iter().skip(1) {
            context.line_to(x, y);
        }
        return;
    }

    for index in 0..points.len() - 1 {
        let (x0, y0) = points[index];
        let (x1, y1) = points[index + 1];
        let (previous_x, previous_y) = if index == 0 {
            (x0, y0)
        } else {
            points[index - 1]
        };
        let (next_x, next_y) = if index + 2 >= points.len() {
            (x1, y1)
        } else {
            points[index + 2]
        };
        let control1_x = x0 + (x1 - previous_x) / 6.0;
        let control2_x = x1 - (next_x - x0) / 6.0;
        let segment_min_y = y0.min(y1);
        let segment_max_y = y0.max(y1);
        let control1_y = (y0 + (y1 - previous_y) / 6.0)
            .clamp(segment_min_y, segment_max_y);
        let control2_y = (y1 - (next_y - y0) / 6.0)
            .clamp(segment_min_y, segment_max_y);
        context.curve_to(control1_x, control1_y, control2_x, control2_y, x1, y1);
    }
}

fn make_detail_chart(unit: &'static str) -> (gtk::DrawingArea, Rc<RefCell<VecDeque<f64>>>) {
    let history = Rc::new(RefCell::new(VecDeque::<f64>::with_capacity(60)));
    let chart = gtk::DrawingArea::new();
    chart.set_content_height(150);
    chart.set_hexpand(true);
    chart.add_css_class("gamemon-sparkline");
    let values = Rc::clone(&history);
    chart.set_draw_func(move |_, context, width, height| {
        if width < 2 || height < 2 { return; }
        let points = values.borrow();
        if points.is_empty() { return; }
        let left = 76.0;
        let right = f64::from(width) - 12.0;
        let top = 12.0;
        let bottom = f64::from(height) - 22.0;
        let min = points.iter().copied().fold(f64::INFINITY, f64::min);
        let max = points.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let padding = ((max - min) * 0.15).max(0.5);
        // Percentages represent utilization and cannot be negative. Anchor
        // these charts at zero so small values never produce a "-0%" axis.
        let low = if unit == "%" { 0.0 } else { (min - padding).min(0.0) };
        let high = (max + padding).max(low + 1.0);
        let span = high - low;
        context.set_line_width(1.0);
        context.set_source_rgba(0.65, 0.68, 0.72, 0.22);
        for tick in 0..=3 {
            let fraction = tick as f64 / 3.0;
            let y = top + (bottom-top) * fraction;
            context.move_to(left, y);
            context.line_to(right, y);
            let _ = context.stroke();

            let tick_value = high - fraction * span;
            let label = match unit {
                "%" => format!("{tick_value:.0}%"),
                "W" => format!("{tick_value:.0} W"),
                "GiB" => format!("{tick_value:.1} GiB"),
                "°C" => format!("{tick_value:.0} °C"),
                "MiB/s" => format!("{tick_value:.1} MiB/s"),
                "load" => format!("{tick_value:.1}"),
                _ => format!("{tick_value:.1}"),
            };
            context.set_font_size(10.0);
            context.set_source_rgba(0.72, 0.74, 0.77, 0.9);
            context.move_to(4.0, y + 3.0);
            let _ = context.show_text(&label);
        }
        let coords = points.iter().enumerate().map(|(i,v)| {
            // Keep a fixed 60-sample horizontal scale while history accumulates.
            let x = left + i as f64 / 59.0 * (right-left);
            let y = bottom - ((*v-low)/span) * (bottom-top);
            (x,y)
        }).collect::<Vec<_>>();
        context.set_line_width(2.2);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        context.set_line_join(gtk::cairo::LineJoin::Round);
        context.set_source_rgba(0.35, 0.72, 0.95, 1.0);
        context.move_to(coords[0].0, coords[0].1);
        append_smooth_line(context, &coords);
        let _ = context.stroke();
        let (x,y) = *coords.last().unwrap();
        context.arc(x,y,3.5,0.0,std::f64::consts::TAU);
        let _ = context.fill();
        context.set_font_size(10.0);
        context.set_source_rgba(0.72,0.74,0.77,0.9);
        context.move_to(left,bottom+16.0);
        let _ = context.show_text("60 metingen geleden");
        context.move_to(right-18.0,bottom+16.0);
        let _ = context.show_text("nu");
    });
    (chart, history)
}

fn push_detail_sample(
    charts: &BTreeMap<String, (gtk::DrawingArea, Rc<RefCell<VecDeque<f64>>>)>,
    key: &str,
    value: Option<f64>,
) {
    let Some((chart, history)) = charts.get(key) else { return };
    if let Some(value) = value.filter(|v| v.is_finite()) {
        let mut values = history.borrow_mut();
        if values.len() == 60 { values.pop_front(); }
        values.push_back(value);
        drop(values);
        if chart.is_mapped() {
            chart.queue_draw();
        }
    }
}
fn set_detail_if_visible(
    labels: &BTreeMap<String, gtk::Box>,
    expanded_states: &mut BTreeMap<String, BTreeMap<String, bool>>,
    visible_page: &str,
    key: &str,
    value: &str,
) {
    // Detail widgets are expensive to rebuild. Only update the page currently
    // shown; the latest snapshot is applied on the next poll after navigation.
    if visible_page == key {
        set_detail(labels, expanded_states, key, value);
    }
}

fn set_detail(
    labels: &BTreeMap<String, gtk::Box>,
    expanded_states: &mut BTreeMap<String, BTreeMap<String, bool>>,
    key: &str,
    value: &str,
) {
    let Some(container) = labels.get(key) else { return };

    // Bewaar de huidige status voordat de inhoud wordt bijgewerkt.
    // Zo blijft een handmatig geopende sectie open na een nieuwe meting.
    let states = expanded_states.entry(key.to_owned()).or_default();
    let mut child = container.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(frame) = widget.downcast_ref::<gtk::Frame>() {
            if let Some(expander_widget) = frame.child() {
                if let Ok(expander) = expander_widget.downcast::<gtk::Expander>() {
                    if let Some(heading) = expander.label() {
                        states.insert(heading.to_string(), expander.is_expanded());
                    }
                }
            }
        }
    }

    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let sections = value.split("\n\n")
        .filter(|section| !section.trim().is_empty())
        .collect::<Vec<_>>();
    let mut visible_sections = 0usize;

    for section in sections {
        let mut lines = section.lines().filter(|line| !line.trim().is_empty());
        let first = lines.next().unwrap_or("Details");
        let (heading, first_value) = first.strip_prefix("# ")
            .map(|heading| (heading.trim(), None))
            .unwrap_or(("Details", Some(first)));

        let mut content_lines = Vec::new();
        if let Some(line) = first_value {
            content_lines.push(line.to_owned());
        }
        content_lines.extend(lines.map(str::to_owned));

        // Verberg groepen waarvan alle waarden ontbreken; lege dropdowns
        // voegen niets toe en maken de pagina onnodig lang.
        let has_data = content_lines.iter().any(|line| {
            let value = line.split_once(':').map(|(_, value)| value.trim()).unwrap_or(line.trim());
            !value.is_empty()
                && value != "N/B"
                && value != "Niet beschikbaar"
                && !value.starts_with("Geen ")
        });
        if !has_data {
            continue;
        }

        let expander = gtk::Expander::new(Some(heading));
        expander.set_expanded(states.get(heading).copied().unwrap_or(visible_sections == 0));
        expander.add_css_class("gamemon-section");

        let card = gtk::Frame::new(None);
        card.add_css_class("card");
        card.set_hexpand(true);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
        body.set_margin_top(10);
        body.set_margin_bottom(10);
        body.set_margin_start(12);
        body.set_margin_end(12);

        for line in &content_lines {
            append_detail_row(&body, line);
        }

        expander.set_child(Some(&body));
        card.set_child(Some(&expander));
        container.append(&card);
        visible_sections += 1;
    }

    if visible_sections == 0 {
        let empty = gtk::Label::new(Some("Geen meetgegevens beschikbaar"));
        empty.set_halign(Align::Start);
        empty.add_css_class("dim-label");
        container.append(&empty);
    }
}

fn append_detail_row(container: &gtk::Box, text: &str) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_hexpand(true);

    if let Some((label_text, value_text)) = text.split_once(':') {
        let label = gtk::Label::new(Some(label_text.trim()));
        label.set_halign(Align::Start);
        label.set_xalign(0.0);
        label.set_single_line_mode(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_hexpand(true);
        label.set_tooltip_text(Some(label_text.trim()));
        label.add_css_class("dim-label");

        let value = gtk::Label::new(Some(value_text.trim()));
        value.set_halign(Align::End);
        value.set_xalign(1.0);
        value.set_single_line_mode(true);
        value.set_ellipsize(gtk::pango::EllipsizeMode::End);
        value.set_max_width_chars(48);
        value.set_tooltip_text(Some(value_text.trim()));
        value.set_selectable(true);
        value.add_css_class("numeric");

        row.append(&label);
        row.append(&value);
    } else {
        let label = gtk::Label::new(Some(text.trim()));
        label.set_halign(Align::Start);
        label.set_xalign(0.0);
        label.set_single_line_mode(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_hexpand(true);
        label.set_tooltip_text(Some(text.trim()));
        label.set_selectable(true);
        row.append(&label);
    }
    container.append(&row);
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

// Keep network throughput in one fixed unit so the overview labels do not
// jump in width whenever traffic crosses a B/KiB/MiB threshold.
fn format_rate(bytes_per_second: f64) -> String {
    let megabits_per_second = bytes_per_second.max(0.0) * 8.0 / 1_000_000.0;
    format!("{megabits_per_second:8.2} Mb/s")
}
