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

/// The runtime's URL below `base` (`/_rn/`): its name carries its hash, so
/// it is cached forever and replaced by a new URL when it changes.
#[must_use]
pub fn runtime_url(base: &str) -> String {
    format!("{base}rn.{}.js", crate::hash::class_name("", RUNTIME_JS))
}
