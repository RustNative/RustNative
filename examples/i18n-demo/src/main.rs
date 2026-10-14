#![cfg_attr(windows, windows_subsystem = "windows")]

use i18n_demo::{Demo, messages};
use rustnative_core::{Application, Component, Size, Window};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let identity =
        rustnative_host::AppIdentity::new("dev.rustnative.i18n-demo", "Rust Native i18n");
    // The host's services include its locale formatting: numbers, dates,
    // and casing as the host writes them for each locale.
    let services = rustnative_host::services(&identity)?.with_catalogues(messages::catalogues());
    let mut application = Application::with_services(
        Demo::new(()),
        Window::new("Rust Native i18n", Size::new(520, 360)),
        services,
    );
    rustnative_host::run(&mut application, &identity)?;
    Ok(())
}
