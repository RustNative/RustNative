#![cfg_attr(windows, windows_subsystem = "windows")]

use gallery::Gallery;
use rustnative_core::{Application, Component, Platform, Size, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::LinuxPlatform as HostPlatform;
#[cfg(not(target_os = "linux"))]
use rustnative_windows::WindowsPlatform as HostPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application =
        Application::new(Gallery::new(()), Window::new("Rust Native gallery", Size::new(900, 720)));
    application.set_theme(gallery::theme());
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
