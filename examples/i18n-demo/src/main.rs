#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, target_os = "linux"))]
use std::sync::Arc;

use i18n_demo::{Demo, messages};
use rustnative_core::{Application, Component, Platform, Services, Size, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::LinuxPlatform as HostPlatform;
#[cfg(not(target_os = "linux"))]
use rustnative_windows::WindowsPlatform as HostPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let services = Services::default().with_catalogues(messages::catalogues());
    // Numbers, dates, and casing as the host writes them for each locale.
    #[cfg(windows)]
    let services = services.with_locale_service(Arc::new(rustnative_windows::WindowsLocale));
    #[cfg(target_os = "linux")]
    let services = services.with_locale_service(Arc::new(rustnative_linux::LinuxLocale));
    let mut application = Application::with_services(
        Demo::new(()),
        Window::new("Rust Native i18n", Size::new(520, 360)),
        services,
    );
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
