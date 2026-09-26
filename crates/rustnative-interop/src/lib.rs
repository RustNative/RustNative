//! Library-only mode (`PLAN.md` Milestone 40): a Rust Native library —
//! its model, state, and services, with no UI — compiled into an existing
//! application in another language, through one interface description.
//!
//! - [`idl`]: the `.ril` description, with ownership and threading
//!   annotated;
//! - [`generate`]: the C header, the C# bindings, and the Rust
//!   implementation shims generated from it (`rustnative bindgen`, or a
//!   build script calling [`generate_rust`]);
//! - [`runtime`]: what the generated shims call — the handle table, panic
//!   containment, the thread check, and string ownership.
//!
//! The implementing crate is a `cdylib`:
//!
//! ```text
//! // build.rs
//! fn main() { rustnative_interop::generate_rust("counter.ril"); }
//!
//! // src/lib.rs
//! include!(concat!(env!("OUT_DIR"), "/counter.rs"));
//! struct MyCounter { count: u32 }
//! impl Counter for MyCounter { /* … */ }
//! export_counter!(MyCounter);
//! ```
#![deny(missing_docs)]

pub mod generate;
pub mod idl;
pub mod runtime;

pub use idl::{Idl, IdlError, parse_idl};

/// Build-script support: reads the `.ril` file at `path` (relative to the
/// crate), and writes `OUT_DIR/<library>.rs` for `include!`.
///
/// # Panics
///
/// When the description does not parse — a mistake in the project, which
/// must stop the build — with its file, line, and column.
pub fn generate_rust(path: &str) {
    let root =
        std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from).unwrap_or_default();
    let out = std::env::var_os("OUT_DIR").map(std::path::PathBuf::from).unwrap_or_default();
    let file = root.join(path);
    println!("cargo:rerun-if-changed={}", file.display());
    let source = std::fs::read_to_string(&file)
        .unwrap_or_else(|cause| panic!("reading {}: {cause}", file.display()));
    let idl = parse_idl(&source).unwrap_or_else(|error| panic!("{}:{error}", file.display()));
    let target = out.join(format!("{}.rs", idl.library));
    std::fs::write(&target, generate::rust(&idl))
        .unwrap_or_else(|cause| panic!("writing {}: {cause}", target.display()));
}
