//! `rustnative generate kit` (`PLAN.md` Milestone 52, `C57-3`): working,
//! tested features written into the project, on the server application
//! model. The sources are the ones `examples/kits` compiles and tests.

use std::path::Path;

use crate::error::{Error, Result};

/// A feature kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Kit {
    /// Accounts: sign-up, sign-in, sign-out, and who is signed in.
    Auth,
    /// The administration surface, for administrators (needs `auth`).
    Admin,
    /// A catalogue and a checkout with server-side receipt validation.
    Commerce,
}

impl Kit {
    const fn module(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Admin => "admin",
            Self::Commerce => "commerce",
        }
    }

    const fn source(self) -> &'static str {
        match self {
            Self::Auth => include_str!("../templates/kits/auth.rs"),
            Self::Admin => include_str!("../templates/kits/admin.rs"),
            Self::Commerce => include_str!("../templates/kits/commerce.rs"),
        }
    }
}

fn io(what: String) -> impl FnOnce(std::io::Error) -> Error {
    move |cause| Error::Io { what, cause }
}

/// Writes `kit` into the project at `root`.
///
/// # Errors
///
/// The project does not use the server model, the kit needs another first,
/// it is already there, or a file cannot be written.
pub fn generate(root: &Path, kit: Kit) -> Result<()> {
    let cargo =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(io("read Cargo.toml".into()))?;
    if !cargo.contains("rustnative-server") {
        return Err(Error::Usage(
            "the kits are written on the server application model: add `rustnative-server` to the project's \
             dependencies first (`PLAN.md` Milestone 49)"
                .into(),
        ));
    }
    let kits = root.join("src").join("kits");
    if kit == Kit::Admin && !kits.join("auth.rs").is_file() {
        return Err(Error::Usage(
            "the admin kit builds on accounts: `rustnative generate kit auth` first".into(),
        ));
    }
    let target = kits.join(format!("{}.rs", kit.module()));
    if target.exists() {
        return Err(Error::Usage(format!("{} already exists", target.display())));
    }
    std::fs::create_dir_all(&kits).map_err(io(format!("create {}", kits.display())))?;
    std::fs::write(&target, kit.source()).map_err(io(format!("write {}", target.display())))?;

    let module = kits.join("mod.rs");
    let mut listing = std::fs::read_to_string(&module)
        .unwrap_or_else(|_| "//! Feature kits (`rustnative generate kit`).\n\n".to_owned());
    listing.push_str("pub mod ");
    listing.push_str(kit.module());
    listing.push_str(";\n");
    std::fs::write(&module, listing).map_err(io(format!("write {}", module.display())))?;

    let lib_path = root.join("src").join("lib.rs");
    let lib =
        std::fs::read_to_string(&lib_path).map_err(io(format!("read {}", lib_path.display())))?;
    if !lib.lines().any(|line| line.trim() == "pub mod kits;") {
        std::fs::write(&lib_path, format!("{}\npub mod kits;\n", lib.trim_end()))
            .map_err(io(format!("write {}", lib_path.display())))?;
    }
    println!(
        "generate: wrote {} (add its routes to your ServerApp; see the module's documentation)",
        target.display()
    );
    Ok(())
}
