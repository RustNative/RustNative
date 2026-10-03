#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, target_os = "linux"))]
use std::sync::Arc;

use product_services::Product;
use rustnative_core::product::Flags;
use rustnative_core::{Application, Component, Platform, Services, Size, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::LinuxPlatform as HostPlatform;
#[cfg(not(target_os = "linux"))]
use rustnative_windows::WindowsPlatform as HostPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Remote configuration would be fetched with `Flags::refresh`; locally,
    // `RUSTNATIVE_FLAGS=compact-layout=true` turns the feature on.
    let services = Services::default().with_flags(Flags::new());
    #[cfg(windows)]
    let services = services
        .with_secure_storage(Arc::new(rustnative_windows::WindowsSecureStorage::new(
            "dev.rustnative.product-services",
        )))
        .with_push(Arc::new(rustnative_windows::WindowsPush))
        .with_commerce(Arc::new(rustnative_windows::WindowsStore));
    #[cfg(target_os = "linux")]
    let services = services
        .with_secure_storage(Arc::new(rustnative_linux::LinuxSecureStorage::new(
            "dev.rustnative.product-services",
        )))
        .with_push(Arc::new(rustnative_linux::LinuxPush))
        .with_commerce(Arc::new(rustnative_linux::LinuxStore));
    let mut application = Application::with_services(
        Product::new(()),
        Window::new("Product services", Size::new(480, 320)),
        services,
    );
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
