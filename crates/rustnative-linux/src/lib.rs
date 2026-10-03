//! Linux platform backend (`PLAN.md` Milestone 34).
//!
//! The backend realizes the portable tree as native widgets of a Linux
//! toolkit — GTK 4 today, behind an internal seam (`toolkit`) so a second
//! toolkit can be added without touching the core — and reaches the desktop
//! through the freedesktop services every Linux desktop shares: portals,
//! the notification and tray protocols, AT-SPI2, logind, and the Secret
//! Service.
//!
//! Module map (private modules; the public API is re-exported flat):
//! - `error`: the [`Error`] type `Platform::Error` resolves to.
//! - `platform`: [`LinuxPlatform`], the `Platform` implementation.
//! - `toolkit`: the seam, and [`ToolkitKind`].
//! - `desktop`: the toolkit-independent half — the [`Session`] model and
//!   the D-Bus services.
//! - `gtk`: the GTK 4 realization (Linux only).
//!
//! # Getting started
//!
//! ```no_run
//! use rustnative_core::{Application, Component, Event, Node, Platform, Size, Window};
//! use rustnative_linux::LinuxPlatform;
//!
//! # struct Greeter;
//! # impl Component for Greeter {
//! #     type Props = ();
//! #     type Message = ();
//! #     fn new((): Self::Props) -> Self { Self }
//! #     fn props(&self) -> &Self::Props { &() }
//! #     fn set_props(&mut self, (): Self::Props) {}
//! #     fn view(&self) -> Node { Node::label("greeting", "Hello") }
//! #     fn update(&mut self, _event: Event) {}
//! # }
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut application =
//!         Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//!     // Blocks until the last window closes.
//!     LinuxPlatform::new().run(&mut application)?;
//!     Ok(())
//! }
//! ```
#![deny(missing_docs)]

pub mod desktop;
mod error;
#[cfg(target_os = "linux")]
mod mappers;
mod platform;
mod toolkit;

#[cfg(all(test, target_os = "linux"))]
mod capability_tests;
#[cfg(target_os = "linux")]
mod gtk;

#[cfg(target_os = "linux")]
mod services;
#[cfg(target_os = "linux")]
pub use services::{
    FileStateStore, LinuxClipboard, LinuxConditions, LinuxFileDialogs, LinuxLocale,
    LinuxPermissions, LinuxPrinting, LinuxPush, LinuxSecureStorage, LinuxSerial, LinuxStore,
    LinuxSystem, PixbufDecoder, SoupHttp,
};

#[cfg(target_os = "linux")]
pub use gtk::embed::{EmbeddedRoot, ExternalLoop};
#[cfg(target_os = "linux")]
pub use gtk::foreign::{ForeignWidget, Ownership, register_foreign};
#[cfg(target_os = "linux")]
pub use gtk::surface::{SurfaceHandle, native_surface};
#[cfg(target_os = "linux")]
pub use mappers::{
    MappedProperty, MapperContext, MapperInfo, MapperMode, MapperTarget, active_mappers,
    clear_mappers, register_mapper,
};

pub use desktop::{DesktopEnvironment, DisplayServer, Session};
pub use error::{Error, NativeContext};
pub use platform::LinuxPlatform;
pub use toolkit::ToolkitKind;

/// Runs the preview catalogue (`rustnative_core::preview::Catalogue`) on
/// this backend, opened at the preview named `first` — what an
/// application's `main` does when `rustnative preview` runs it
/// (`rustnative_core::preview::requested`).
///
/// # Errors
///
/// As [`LinuxPlatform`]'s `run`.
pub fn run_catalogue(
    previews: Vec<rustnative_core::preview::Preview>,
    first: &str,
) -> Result<(), Error> {
    use rustnative_core::Platform as _;
    let mut application = rustnative_core::Application::new(
        rustnative_core::preview::Catalogue::open(previews, first),
        rustnative_core::Window::new("Previews", rustnative_core::Size::new(960, 640)),
    );
    LinuxPlatform::new().run(&mut application)
}
