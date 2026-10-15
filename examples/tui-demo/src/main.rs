//! `rustnative run tui` in this folder: the to-do list in this terminal.

use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_tui::{Channel, TuiPlatform};
use tui_demo::Todo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::new();
    let services = rustnative_tui::services("dev.rustnative.tui-demo", "Terminal demo", &channel)?;
    let mut application =
        Application::with_services(Todo::new(()), Window::new("To do", Size::new(0, 0)), services);
    TuiPlatform::new().with_channel(channel).run(&mut application)?;
    Ok(())
}
