#![cfg_attr(windows, windows_subsystem = "windows")]

use filter_demo::App;
use rustnative_core::{Application, Component, Platform, Size, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::LinuxPlatform as HostPlatform;
#[cfg(not(target_os = "linux"))]
use rustnative_windows::WindowsPlatform as HostPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application =
        Application::new(App::new(()), Window::new("Rust Native filter", Size::new(520, 640)));
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
