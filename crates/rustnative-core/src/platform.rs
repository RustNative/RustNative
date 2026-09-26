//! The seam a platform backend implements to run an [`crate::Application`].

use std::any::Any;
use std::error::Error;
use std::fmt;

use crate::application::Application;
use crate::capability::PlatformCapabilities;

/// A native platform adapter capable of running an [`Application`]'s event
/// loop. Implemented by platform crates (for example,
/// `rustnative-windows::WindowsPlatform`); `rustnative-core` itself never
/// implements this for a real backend — see the crate root's module-level
/// documentation for why no platform/OS API dependency belongs here.
pub trait Platform {
    /// This backend's error type for a failed [`Self::run`].
    type Error: Error + Send + Sync + 'static;

    /// Runs the native event loop until the application exits.
    ///
    /// # Errors
    ///
    /// Returns `Self::Error` if the platform backend fails to initialize or
    /// encounters an unrecoverable native error while running — for
    /// `rustnative-windows`, this includes a component panicking inside a
    /// `WNDPROC` callback (see its `Error::ComponentPanicked`).
    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error>;

    /// Reports the portable features available from this adapter at
    /// runtime.
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::default()
    }

    /// How this backend answers each style property — realized,
    /// approximated (and how), or unavailable (and why) — `PLAN.md` 2.14's
    /// per-property form of 2.5. A backend that has not answered reports
    /// everything unavailable.
    fn style_capabilities(&self) -> crate::style::StyleCapabilities {
        crate::style::StyleCapabilities::NONE
    }

    /// How this backend maps the style vocabulary's units onto its own, if
    /// it has recorded one.
    fn unit_mapping(&self) -> Option<crate::style::UnitMapping> {
        None
    }

    /// Explicit escape hatch for backend-specific functionality.
    /// Applications may downcast this value at their platform boundary
    /// without allowing platform types to leak into `rustnative-core`.
    fn native_extension(&self) -> &dyn Any;
}

/// A marker error for a [`Platform`] implementation that has no real
/// backend for the current build (for example, a target with no realized
/// platform adapter yet). Not itself a [`Platform`] impl — it exists to be
/// used as `Platform::Error` by a stub implementation, so that stub fails
/// with a clear, catchable error instead of an opaque abort or a
/// compile-time absence of any `Platform` impl at all.
#[derive(Debug)]
pub struct UnsupportedPlatform;

impl fmt::Display for UnsupportedPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this platform is not implemented by the selected backend")
    }
}

impl Error for UnsupportedPlatform {}
