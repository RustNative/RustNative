#![cfg_attr(windows, windows_subsystem = "windows")]

use rustnative_core::{Application, Component, Platform, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::LinuxPlatform as HostPlatform;
#[cfg(not(target_os = "linux"))]
use rustnative_windows::WindowsPlatform as HostPlatform;
use span::{DESKTOP_WINDOW, Thermostat};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application =
        Application::new(Thermostat::new(()), Window::new("Thermostat", DESKTOP_WINDOW));
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
