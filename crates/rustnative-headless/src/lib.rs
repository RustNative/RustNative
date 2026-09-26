//! The headless reference backend.
//!
//! `PLAN.md` 2.13 caps what can be verified at the hardware in front of the
//! project. A headless backend lifts most of that cap for everything above
//! the realization layer (Milestone 45): it realizes the tree into an
//! inspectable model — through the same [`rustnative_core::TreeSnapshot`]
//! and [`rustnative_core::TreeDiff`] a native backend consumes, laid out with
//! deterministic metrics ([`HeadlessMeasurer`]) — and drives it with
//! synthetic input that travels the real input path.
//!
//! It is a backend in its own right, not a mock: it implements
//! [`rustnative_core::Platform`] ([`HeadlessPlatform`]), answers capabilities
//! honestly, and is the second backend the conformance suites of
//! Milestone 41 run against.
//!
//! | Item | For |
//! |---|---|
//! | [`HeadlessApp`] | driving an application: click, type, tab, scroll, advance time, restart |
//! | [`Query`] | finding nodes by role, accessible name, text, or label (`C60`) |
//! | [`HeadlessTree`] | the realized model, and its golden description |
//! | [`assert_golden!`] | comparing realized output against a reviewed file |
//! | [`MockHttp`] | scripted HTTP with expectations, checked by exhaustive mode (`C11`) |
//! | [`HeadlessPlatform`] | running an [`rustnative_core::Application`] with no host at all |
#![deny(missing_docs)]

mod app;
mod golden;
mod inspect;
mod measure;
mod platform;
mod query;
mod services;
mod tree;

pub use app::{HeadlessApp, STATE_FLUSH_DELAY};
pub use golden::{check_golden, diff, preview_goldens};
pub use inspect::HeadlessInspect;
pub use measure::HeadlessMeasurer;
pub use platform::HeadlessPlatform;
pub use query::{Found, Query, QueryError};
pub use services::MockHttp;
pub use tree::{HeadlessTree, RealizationStats, RealizedNode};
