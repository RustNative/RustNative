//! `rustnative`, the Rust Native command.
//!
//! ```text
//! rustnative new <name> --syntax builder|markup [--path DIR] [--framework-path DIR]
//! rustnative build <platform> [--release]
//! rustnative run   <platform> [--release]
//! rustnative check [platform]
//! rustnative test  [-- cargo test arguments]
//! rustnative package <platform> [--format zip|msix|all] [--sign CERT --password-env VAR]
//! rustnative doctor [--json]
//! rustnative expand <file>
//! rustnative fmt [files...] [--check]
//! rustnative lsp [--server rust-analyzer]
//! ```
//!
//! `rustnative` **orchestrates** the toolchains rather than replacing them: a build
//! is Cargo's build, with Cargo's own output, and the Windows SDK tools are
//! found where they are installed rather than bundled. Platforms whose
//! backend the roadmap has not reached are recognized and refused by name,
//! with the milestone that will bring them — never quietly built for
//! Windows instead.
//!
//! A project is a folder with a `rustnative.toml` (see [`config`]), which holds
//! the identity the application's saved state, single-instance mutex, and
//! package are keyed by.

#![deny(missing_docs)]

mod bench;
mod cli;
mod compliance;
mod config;
mod crash;
mod db;
mod deploy;
mod describe;
mod dev;
mod diagnostics;
mod doctor;
mod emulate;
mod error;
mod generate;
mod i18n;
mod inspect;
mod kits;
mod lsp;
mod lsp_assist;
mod markup;
mod markup_edit;
mod package;
mod packages;
mod pgo;
mod platform;
mod project;
mod tokens;
mod toolchain;
mod upgrade;
mod web;

use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    match cli::Cli::parse().run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
