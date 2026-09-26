//! The reference application of the published comparison
//! (`docs/comparison/methodology.md`): the layout conformance suite's
//! reference screen, realized by the Windows backend. `--pseudo` shows it
//! pseudo-localized; `--rtl` mirrored.
#![cfg_attr(windows, windows_subsystem = "windows")]

use rustnative_conformance::reference::{ReferenceScreen, Variant};
use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_windows::WindowsPlatform;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let variant = Variant {
        pseudo: arguments.iter().any(|argument| argument == "--pseudo"),
        right_to_left: arguments.iter().any(|argument| argument == "--rtl"),
    };
    let mut application = Application::new(
        ReferenceScreen::new(variant),
        Window::new("Reference application", Size::new(480, 600)),
    );
    if let Err(error) = WindowsPlatform::new().run(&mut application) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
