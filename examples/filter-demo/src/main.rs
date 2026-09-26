#![cfg_attr(windows, windows_subsystem = "windows")]

use filter_demo::App;
use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_windows::WindowsPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut application =
        Application::new(App::new(()), Window::new("Rust Native filter", Size::new(520, 640)));
    WindowsPlatform::new().run(&mut application)?;
    Ok(())
}
