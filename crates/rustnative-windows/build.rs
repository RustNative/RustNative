//! Embeds an application manifest in this crate's **test** executables.
//!
//! One Windows feature this backend realizes is gated on the application
//! declaring which Windows versions it was built for: a *layered child
//! window*, which is how `Node::with_opacity` is realized, is only honored
//! for a process whose manifest claims Windows 8 or later. Without that, the
//! style is silently refused and nodes stay opaque.
//!
//! An application gets its manifest from packaging (Milestone 32). A test
//! executable has no packaging step, so the manifest is linked in here —
//! otherwise the opacity tests would be testing the absence of a manifest
//! rather than the backend.
//!
//! The flags go to every linked target of *this* package — which, since a
//! library has no link step of its own, means its test executable — and
//! only with the MSVC linker, the only one this crate's tests are built
//! with. (`rustc-link-arg-tests` would be narrower, but it applies only to
//! `tests/` directory targets, and this crate tests itself from inside.)

fn main() {
    println!("cargo:rerun-if-changed=tests.manifest");

    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !windows || !msvc {
        return;
    }

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests.manifest");
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // The manifest declares nothing about elevation, and the linker's
    // default UAC fragment would conflict with merging one in.
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
}
