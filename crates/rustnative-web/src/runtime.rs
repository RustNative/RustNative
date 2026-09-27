//! The browser runtime's source: fixed JavaScript the framework ships
//! (never generated), served as a hashed, immutable asset.
//!
//! `runtime/rn.js` holds the client subset's semantics (Rust's integer
//! overflow, float and string formatting), the realizer that mirrors
//! [`crate::dom`] and [`crate::css`], the DOM patcher, and the island
//! runtime: attaching generated modules to server-rendered markup,
//! delivering events, and carrying out effects.

/// `rn.js`.
pub const RUNTIME_JS: &str = include_str!("runtime/rn.js");

/// `sw.js`, the service worker (`crate::pwa`), before its build's
/// values are written in.
pub const SW_JS: &str = include_str!("runtime/sw.js");

/// The runtime's URL below `base` (`/_rn/`): its name carries its hash, so
/// it is cached forever and replaced by a new URL when it changes.
#[must_use]
pub fn runtime_url(base: &str) -> String {
    format!("{base}rn.{}.js", crate::hash::class_name("", RUNTIME_JS))
}

/// The Web Worker script a worker-run WebAssembly subtree starts: it imports
/// the runtime, beside it, and hands itself over.
#[must_use]
pub fn worker_js() -> String {
    format!(
        "import {{ wasmWorker }} from \"./rn.{}.js\";\nwasmWorker();\n",
        crate::hash::class_name("", RUNTIME_JS)
    )
}

/// The worker script's URL below `base`.
#[must_use]
pub fn worker_url(base: &str) -> String {
    format!("{base}worker.{}.js", crate::hash::class_name("", &worker_js()))
}
