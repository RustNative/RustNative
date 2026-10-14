#![cfg_attr(windows, windows_subsystem = "windows")]

use product_services::Product;
use rustnative_core::product::Flags;
use rustnative_core::{Application, Component, Size, Window};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The host's services: secure storage, push, and store billing among
    // them (answered "unavailable, because…" where the host has none).
    // Remote configuration would be fetched with `Flags::refresh`; locally,
    // `RUSTNATIVE_FLAGS=compact-layout=true` turns the feature on.
    let identity =
        rustnative_host::AppIdentity::new("dev.rustnative.product-services", "Product services");
    let services = rustnative_host::services(&identity)?.with_flags(Flags::new());
    let mut application = Application::with_services(
        Product::new(()),
        Window::new("Product services", Size::new(480, 320)),
        services,
    );
    rustnative_host::run(&mut application, &identity)?;
    Ok(())
}
