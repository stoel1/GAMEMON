use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
use gtk::{Align, Orientation};
use crate::collectors::{cpu, memory};

pub fn activate(application: &Application) {
    let title = gtk::Label::builder().label("GAMEMON").halign(Align::Start).css_classes(["title-1"]).build();
    let cpu_label = gtk::Label::new(Some(&cpu::snapshot()));
    cpu_label.set_halign(Align::Start);
    let memory_label = gtk::Label::new(Some(&memory::snapshot()));
    memory_label.set_halign(Align::Start);
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
    let window = ApplicationWindow::builder().application(application).title("GAMEMON").default_width(900).default_height(650).content(&toolbar).build();
    window.present();
}
