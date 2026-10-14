//! The terminal backend (`PLAN.md` Milestone 38).
//!
//! A terminal is a real host with real conventions (`PLAN.md` 2.3): its
//! host object is the cell grid, with its own colour depth and text
//! attributes, its key and mouse protocols, its resize notifications, its
//! alternate screen and cursor, and — where it has one — its clipboard
//! sequence. This crate realizes the same application model there: the
//! same components, state, layout, identity, and events as every other
//! backend, with one small, honestly answered capability set.
//!
//! | Item | For |
//! |---|---|
//! | [`TuiPlatform`] | running an [`rustnative_core::Application`] in the process's terminal |
//! | [`services()`], [`Channel`] | the terminal's services (OSC 52 clipboard, notifications, state files) |
//! | [`TuiHarness`] | driving an application on a synthetic terminal and asserting its screen |
//! | [`Width`] | Unicode display width: what text takes in cells |
//! | [`guard`] | restoration on exit, signal, and panic |
//! | [`rustnative_style::TUI`], [`rustnative_style::TUI_UNITS`] | the style table and unit mapping |
//!
//! Lower layers are public for hosts and tests that need them: the input
//! [`decode`]r, the output [`encode`]r and cell [`grid`], the VT
//! [`emulator`] the harness asserts through, the [`term`]inal seam, the
//! terminal [`probe`], the realized [`model`], and the widget [`paint`]er.
//!
//! The guide is `docs/tui.md`.
#![deny(missing_docs)]

pub mod color;
pub mod decode;
pub mod edit;
pub mod emulator;
pub mod encode;
pub mod grid;
pub mod guard;
pub mod harness;
pub mod inspect;
pub mod measure;
pub mod model;
pub mod paint;
mod platform;
pub mod probe;
pub mod services;
pub mod session;
pub mod term;
pub mod width;

pub use color::ColorDepth;
pub use harness::{SyntheticTerminal, TuiHarness};
pub use inspect::TuiInspect;
pub use measure::TuiMeasurer;
pub use platform::{TuiError, TuiPlatform};
pub use services::{Channel, Osc52Clipboard, TerminalSystem, services};
pub use session::{Session, SessionStats, TuiConfig};
pub use width::Width;
