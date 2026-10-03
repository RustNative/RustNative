//! The internal seam between the Linux backend and the native toolkit that
//! draws its widgets (`PLAN.md` Milestone 34: "the toolkit behind an
//! internal seam so a second one can be added without touching the core").
//!
//! The shared half of the backend (`crate::desktop`, the service types, the
//! platform entry point) talks to a toolkit only through [`Toolkit`]. GTK 4
//! is the one implementation today (`crate::gtk`); a second toolkit is a
//! second module implementing the same trait and a second [`ToolkitKind`]
//! variant — the core, the portable model, and the desktop services do not
//! change.

#[cfg(target_os = "linux")]
use rustnative_core::{Application, Capability};

#[cfg(target_os = "linux")]
use crate::Error;
#[cfg(target_os = "linux")]
use crate::desktop::Session;

/// A native widget toolkit the Linux backend can realize the tree with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ToolkitKind {
    /// GTK 4 (4.14 or newer): GTK widgets, Pango text, GDK input, GTK's
    /// AT-SPI2 accessibility, and `GdkFrameClock` animation.
    #[default]
    Gtk4,
}

impl ToolkitKind {
    /// The toolkit's name, as a person would write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gtk4 => "GTK 4",
        }
    }
}

/// What a toolkit implementation provides to the shared half.
#[cfg(target_os = "linux")]
pub(crate) trait Toolkit {
    /// Runs `application` until its last window closes.
    fn run(
        &self,
        application: &mut Application,
        options: &RunOptions,
        session: &Session,
    ) -> Result<(), Error>;

    /// The capabilities this toolkit realizes in `session`, beyond the
    /// toolkit-independent ones the shared half answers itself.
    fn capabilities(&self, session: &Session) -> Vec<Capability>;
}

/// What `LinuxPlatform` hands its toolkit to run with.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Default)]
pub(crate) struct RunOptions {
    /// A URL the application was launched with (the first `scheme://…`
    /// argument), delivered as `Event::DeepLink` once its windows exist.
    pub(crate) launch_url: Option<String>,
    /// The application's id (`LinuxPlatform::with_app_id`): its desktop
    /// entry's name, its single-instance bus name, and its state's home.
    pub(crate) app_id: Option<String>,
}

/// The implementation of `kind`.
#[cfg(target_os = "linux")]
fn implementation(kind: ToolkitKind) -> impl Toolkit {
    match kind {
        ToolkitKind::Gtk4 => crate::gtk::Gtk4,
    }
}

/// Runs `application` on `kind` (what `LinuxPlatform::run` does).
#[cfg(target_os = "linux")]
pub(crate) fn run(
    kind: ToolkitKind,
    application: &mut Application,
    options: &RunOptions,
    session: &Session,
) -> Result<(), Error> {
    implementation(kind).run(application, options, session)
}

/// Everything the backend realizes in `session` with `kind`: the shared
/// half's answers and the toolkit's.
#[cfg(target_os = "linux")]
pub(crate) fn capabilities(
    kind: ToolkitKind,
    session: &Session,
) -> rustnative_core::PlatformCapabilities {
    rustnative_core::PlatformCapabilities::new(
        crate::desktop::capabilities(session)
            .into_iter()
            .chain(implementation(kind).capabilities(session)),
    )
}

/// The first command-line argument that looks like a URL — how a desktop
/// entry's `%u` hands one to the application.
#[cfg(target_os = "linux")]
pub(crate) fn launch_url(mut arguments: impl Iterator<Item = String>) -> Option<String> {
    // The program name is not a candidate.
    let _ = arguments.next();
    arguments.find(|argument| {
        argument.split_once("://").is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn the_launch_url_is_the_first_url_shaped_argument() {
        let arguments = ["app", "--verbose", "notes://open/42", "other://x"].map(str::to_owned);
        assert_eq!(launch_url(arguments.into_iter()), Some("notes://open/42".to_owned()));
        let program_only = ["notes://not-an-argument"].map(str::to_owned);
        assert_eq!(launch_url(program_only.into_iter()), None);
        let none = ["app", "file.txt", "://empty-scheme"].map(str::to_owned);
        assert_eq!(launch_url(none.into_iter()), None);
    }
}
