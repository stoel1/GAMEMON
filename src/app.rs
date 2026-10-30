use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
use gtk::{Align, Orientation};

use crate::collectors::{cpu, memory};

const REFRESH_INTERVAL_SECONDS: u32 = 2;

pub fn activate(application: &Application) {
    let title = gtk::Label::builder()
        .label("GAMEMON")
        .halign(Align::Start)
        .css_classes(["title-1"])
        .build();
    let cpu_label = gtk::Label::new(None);
    cpu_label.set_halign(Align::Start);
    cpu_label.set_selectable(true);
    let memory_label = gtk::Label::new(None);
    memory_label.set_halign(Align::Start);
    memory_label.set_selectable(true);

    let content = gtk::Box::new(Orientation::Vertical, 12);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&title);
    content.append(&cpu_label);
    content.append(&memory_label);

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&HeaderBar::new());
    toolbar.set_content(Some(&content));
    let window = ApplicationWindow::builder()
        .application(application)
        .title("GAMEMON")
        .default_width(900)
        .default_height(650)
        .content(&toolbar)
        .build();

    let collector = Rc::new(RefCell::new(cpu::CpuCollector::default()));
    refresh(&collector, &cpu_label, &memory_label);

    let cpu_label = cpu_label.downgrade();
    let memory_label = memory_label.downgrade();
    gtk::glib::timeout_add_seconds_local(REFRESH_INTERVAL_SECONDS, move || {
        let (Some(cpu_label), Some(memory_label)) = (cpu_label.upgrade(), memory_label.upgrade())
        else {
            return gtk::glib::ControlFlow::Break;
        };
        refresh(&collector, &cpu_label, &memory_label);
        gtk::glib::ControlFlow::Continue
    });

    window.present();
}

fn refresh(
    collector: &Rc<RefCell<cpu::CpuCollector>>,
    cpu_label: &gtk::Label,
    memory_label: &gtk::Label,
) {
    let cpu = collector.borrow_mut().sample();
    let model = cpu.model.as_deref().unwrap_or("Niet beschikbaar");
    let online = cpu.online.as_deref().unwrap_or("Niet beschikbaar");
    let utilization = match cpu.utilization {
        cpu::CpuUtilization::Available(value) => format!("{value:.1}%"),
        cpu::CpuUtilization::WarmingUp => "Meten…".to_owned(),
        cpu::CpuUtilization::Unavailable => "Niet beschikbaar".to_owned(),
    };
    cpu_label.set_text(&format!(
        "CPU\n{model}\nOnline logical CPU's: {online}\nGebruik: {utilization}"
    ));

    let memory = memory::sample();
    let memory_text = match (memory.used_bytes(), memory.total_bytes) {
        (Some(used), Some(total)) => format!(
            "RAM gebruikt: {:.1} GiB / {:.1} GiB",
            used as f64 / 1_073_741_824.0,
            total as f64 / 1_073_741_824.0
        ),
        _ => "RAM-gebruik: Niet beschikbaar".to_owned(),
    };
    memory_label.set_text(&format!("Geheugen\n{memory_text}"));
}
