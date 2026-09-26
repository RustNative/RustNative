//! Embedding static assets into the binary (`PLAN.md` Milestone 50): the
//! single-artifact deployment. See `rustnative_server::assets` for serving
//! them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The media type for a file name.
#[must_use]
pub fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn files(directory: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(
                    ALPHABET[usize::try_from((bits >> (18 - 6 * index)) & 63).unwrap_or(0)],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The Rust source declaring the assets under `directory` as a
/// `rustnative_server::assets::Asset` slice named `ASSETS`: each file with
/// its content-hashed name and integrity digest, its bytes included.
///
/// # Errors
///
/// The directory cannot be read.
pub fn assets_source(directory: &Path) -> std::io::Result<String> {
    let mut paths = Vec::new();
    files(directory, &mut paths)?;
    let mut source = String::from(
        "/// The embedded assets (`rustnative_build::embed_assets`).\npub static ASSETS: &[::rustnative_server::assets::Asset] = &[\n",
    );
    for path in paths {
        let bytes = std::fs::read(&path)?;
        let digest = Sha256::digest(&bytes);
        let relative =
            path.strip_prefix(directory).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let short = digest.iter().take(4).fold(String::new(), |mut short, byte| {
            let _ = write!(short, "{byte:02x}");
            short
        });
        let hashed = match relative.rsplit_once('.') {
            Some((stem, extension)) => format!("{stem}.{short}.{extension}"),
            None => format!("{relative}.{short}"),
        };
        let _ = writeln!(
            source,
            "    ::rustnative_server::assets::Asset {{ path: {relative:?}, hashed: {hashed:?}, content_type: {:?}, integrity: \"sha256-{}\", bytes: include_bytes!({:?}) }},",
            content_type(&relative),
            base64(&digest),
            path.to_string_lossy()
        );
    }
    source.push_str("];\n");
    Ok(source)
}

/// From a build script: embeds `directory` (relative to the package) as
/// `OUT_DIR/assets.rs`; include it with
/// `include!(concat!(env!("OUT_DIR"), "/assets.rs"))`.
///
/// # Panics
///
/// Outside a build script, or when the directory cannot be read — a build
/// that cannot embed its assets must not produce an artifact without them.
#[allow(clippy::panic, reason = "a build script reports failure by panicking")]
pub fn embed_assets(directory: &str) {
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| panic!("embed_assets runs in a build script"));
    let out =
        std::env::var("OUT_DIR").unwrap_or_else(|_| panic!("embed_assets runs in a build script"));
    let directory = Path::new(&root).join(directory);
    println!("cargo::rerun-if-changed={}", directory.display());
    let source = assets_source(&directory)
        .unwrap_or_else(|error| panic!("assets {}: {error}", directory.display()));
    std::fs::write(Path::new(&out).join("assets.rs"), source)
        .unwrap_or_else(|error| panic!("assets.rs: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_is_standard_base64_of_sha256() {
        // echo -n "hello" | openssl dgst -sha256 -binary | base64
        assert_eq!(
            base64(&Sha256::digest(b"hello")),
            "LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ="
        );
    }
}
