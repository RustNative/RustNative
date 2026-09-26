//! Windows platform backend.
//!
//! The backend owns native Win32 objects and reconciles them against the
//! platform-independent Rust UI tree. Containers are native child windows,
//! which makes the Rust tree hierarchy correspond to a real Win32 hierarchy.
//!
//! Module map (private modules; the public API is re-exported flat from
//! this crate root, listed below):
//! - `error`: the [`Error`] type `Platform::Error` resolves to, plus the
//!   [`NativeContext`]/[`Win32Category`] it carries.
//! - `platform`: [`WindowsPlatform`], the `Platform` trait implementation.
//! - `ffi`: tiny string/memory helpers shared across services and the
//!   native backend.
//! - `services`: clipboard, shell/system, file dialogs, and tray
//!   notifications — the non-window OS integrations.
//! - `native`: the Win32 window backend proper (tree reconciliation,
//!   layout, the message loop, and the window procedures).
//!
//! # Getting started
//!
//! ```no_run
//! use rustnative_core::{Application, Component, Event, Node, Platform, Size, Window};
//! use rustnative_windows::WindowsPlatform;
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
//!
//!     // Blocks until the last window closes. A failure here carries the
//!     // window and node it concerns — see [`Error`] and [`NativeContext`].
//!     WindowsPlatform::new().run(&mut application)?;
//!     Ok(())
//! }
//! ```
//!
//! Register this backend's OS services so components can reach them through
//! the portable contracts in `rustnative_core::Services`:
//!
//! ```
//! use std::sync::Arc;
//!
//! use rustnative_core::Services;
//! # #[cfg(windows)]
//! use rustnative_windows::{WindowsClipboard, WindowsFileDialogs, WindowsSystem};
//!
//! # #[cfg(windows)]
//! let services = Services::default()
//!     .with_clipboard(Arc::new(WindowsClipboard))
//!     .with_file_dialogs(Arc::new(WindowsFileDialogs))
//!     .with_system(Arc::new(WindowsSystem));
//! # #[cfg(windows)]
//! assert!(services.clipboard().is_some());
//! ```
//!
//! # Documentation coverage
//!
//! Every public item — fields and enum variants included — carries a doc
//! comment, enforced by `#![deny(missing_docs)]` below. `rustnative-core` has
//! had this since the standards audit's P2.23 pass; this crate did not, even
//! though its public surface is the one an application actually touches to
//! stand a backend up (`WindowsPlatform`, `Error`, the service types). The
//! lint is `deny` rather than `warn` so the coverage cannot silently regress.
#![deny(missing_docs)]

#[cfg(windows)]
pub mod crash;
mod error;
mod ffi;
mod handle;
#[cfg(windows)]
pub mod isolated;
mod mappers;
#[cfg(windows)]
mod native;
mod platform;
mod services;
#[cfg(windows)]
mod surface;
#[cfg(windows)]
pub mod update;

pub use error::{Error, NativeContext, Win32Category};
pub use handle::{native_handle, validate_native_handle};
pub use mappers::{
    MappedProperty, MapperContext, MapperInfo, MapperMode, MapperTarget, active_mappers,
    clear_mappers, register_mapper,
};
#[cfg(windows)]
pub use native::embed::{EmbeddedRoot, ExternalLoop};
#[cfg(windows)]
pub use native::foreign::{ForeignControl, Ownership, register_foreign};
pub use platform::WindowsPlatform;
#[cfg(windows)]
pub use services::clipboard::WindowsClipboard;
#[cfg(windows)]
pub use services::data::{WicDecoder, WindowsConditions};
#[cfg(windows)]
pub use services::dialogs::WindowsFileDialogs;
#[cfg(windows)]
pub use services::http::WinHttp;
#[cfg(windows)]
pub use services::industrial::{WindowsPrinting, WindowsSerial};
#[cfg(windows)]
pub use services::locale::WindowsLocale;
#[cfg(windows)]
pub use services::permissions::WindowsPermissions;
#[cfg(windows)]
pub use services::product::{WindowsPush, WindowsStore};
#[cfg(windows)]
pub use services::secure_storage::WindowsSecureStorage;
#[cfg(windows)]
pub use services::state_store::FileStateStore;
#[cfg(windows)]
pub use services::system::WindowsSystem;
#[cfg(windows)]
pub use surface::{SurfaceHandle, native_surface};

/// Runs the preview catalogue (`rustnative_core::preview::Catalogue`) on
/// this backend, opened at the preview named `first` — what an
/// application's `main` does when `rustnative preview` runs it
/// (`rustnative_core::preview::requested`).
///
/// # Errors
///
/// As [`WindowsPlatform`]'s `run`.
pub fn run_catalogue(
    previews: Vec<rustnative_core::preview::Preview>,
    first: &str,
) -> Result<(), Error> {
    use rustnative_core::Platform as _;
    let mut application = rustnative_core::Application::new(
        rustnative_core::preview::Catalogue::open(previews, first),
        rustnative_core::Window::new("Previews", rustnative_core::Size::new(960, 640)),
    );
    WindowsPlatform::new().run(&mut application)
}

/// The top-level `HWND` of window `id`, as an integer, if it is open on
/// this backend — for code at the platform boundary (the escape hatch,
/// dialogs owned by a window). `None` on every other operating system.
#[must_use]
pub fn native_window_handle(id: rustnative_core::WindowId) -> Option<isize> {
    #[cfg(windows)]
    {
        native::window_handles::get(id)
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        None
    }
}
