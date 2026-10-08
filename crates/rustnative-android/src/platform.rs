//! The `Platform` entry point this backend exposes to `rustnative_core`.

use rustnative_core::{Application, Platform, PlatformCapabilities};

use crate::Error;

/// The Android backend an application hands its `Application` to.
///
/// # `run` on Android
///
/// Android owns the main thread's loop, so `run` does not block: called
/// from the application's `main` — which `export_main!` exports for the
/// launcher activity to start — it **adopts** the application (it moves it
/// out of the caller's binding, leaving an empty one behind:
/// `Application::take`), shows its primary window in the activity, and
/// returns `Ok(())`. From then on the main looper drives the application
/// until its primary activity finishes; it outlives configuration changes
/// and activity recreation. Called anywhere else, `run` returns
/// [`Error::NoActivity`].
#[derive(Debug, Default, Clone)]
pub struct AndroidPlatform {
    _private: (),
}

impl AndroidPlatform {
    /// Creates the backend. Equivalent to [`Default::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// The platform, as the desktop backends take an application id. On
    /// Android the id is the package's (`rustnative.toml`'s `app.id`,
    /// fixed at build time), so this is the same platform.
    #[must_use]
    pub const fn with_app_id(self, _app_id: &str) -> Self {
        self
    }
}

#[cfg(target_os = "android")]
impl Platform for AndroidPlatform {
    type Error = Error;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        use rustnative_core::WindowId;
        let pending = crate::entry::take_pending().ok_or(Error::NoActivity)?;
        rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::RuntimeReady);
        crate::inspect::enable_from_intent(application, pending.intent.as_ref());
        let backend = crate::backend::Backend::start(application.take());
        let attached = backend.guarded(WindowId::PRIMARY, |registry| {
            registry.attach_activity(
                WindowId::PRIMARY,
                pending.activity,
                pending.root,
                pending.intent.as_ref(),
            )?;
            Ok((registry.flow(), ()))
        });
        backend.drain();
        if let Some(error) = backend.take_error() {
            return Err(error);
        }
        if attached.is_some() {
            rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::FirstFrame);
            // The first idle after this marks content and interaction.
            let _ = crate::jni_host::call_static(
                crate::jni_host::Class::Bridge,
                "scheduleIdle",
                "()V",
                &[],
            );
        }
        Ok(())
    }

    fn style_capabilities(&self) -> rustnative_core::StyleCapabilities {
        rustnative_style::ANDROID
    }

    fn unit_mapping(&self) -> Option<rustnative_core::UnitMapping> {
        Some(rustnative_style::ANDROID_UNITS)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        crate::capabilities::current()
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(not(target_os = "android"))]
impl Platform for AndroidPlatform {
    type Error = Error;

    fn run(&mut self, _application: &mut Application) -> Result<(), Self::Error> {
        Err(Error::UnsupportedHost)
    }

    fn style_capabilities(&self) -> rustnative_core::StyleCapabilities {
        rustnative_style::ANDROID
    }

    fn unit_mapping(&self) -> Option<rustnative_core::UnitMapping> {
        Some(rustnative_style::ANDROID_UNITS)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::default()
    }

    fn native_extension(&self) -> &dyn std::any::Any {
        self
    }
}
