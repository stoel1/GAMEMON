mod app;
mod collectors;
mod core;

use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let application = adw::Application::builder()
        .application_id("io.github.stoel1.GAMEMON")
        .build();

    application.connect_activate(app::activate);
    application.run();
}
