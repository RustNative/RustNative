#![cfg_attr(windows, windows_subsystem = "windows")]

use rustnative_core::{Application, Component, Platform, Window};
use rustnative_windows::WindowsPlatform;
use span::{DESKTOP_WINDOW, Thermostat};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application =
        Application::new(Thermostat::new(()), Window::new("Thermostat", DESKTOP_WINDOW));
    WindowsPlatform::new().run(&mut application)?;
    Ok(())
}
