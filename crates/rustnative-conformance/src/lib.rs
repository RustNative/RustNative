//! The conformance suites (`PLAN.md` Milestone 41; every guarantee and
//! its test is listed in `docs/guarantees.md`):
//!
//! - **syntax equivalence** — every node kind and every modifier written
//!   three ways, with the builder syntax, with markup in `rsx!`, and with
//!   markup in a `.rsx` file, and asserted to produce equal `Node` values
//!   (`tests/syntax_equivalence.rs`); a case added to one spelling and not
//!   the others fails the suite, the only mechanism that keeps two
//!   authoring surfaces equal over years (2.9);
//! - **style equivalence** (`tests/style_equivalence.rs`, 2.14);
//! - **the behavioural guarantees** ([`suites`]) — the transient fast path,
//!   batching, scope-bound cancellation, native-object lifetime — written
//!   once over a [`host::ConformanceHost`] and run on every backend: the
//!   headless backend in `tests/guarantees.rs`, Windows in its own test
//!   module over its native harness;
//! - **layout conformance** (`tests/layout_conformance.rs`).
#![deny(missing_docs)]

pub mod builder_cases;
pub mod host;
pub mod macro_cases;
pub mod reference;
pub mod suites;
pub mod syntax;

rustnative_core::rsx_mod!(
    /// The `.rsx` spelling of every case (`src/markup_file.rsx`).
    pub markup_file
);
