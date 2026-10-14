//! [`TuiPlatform`]: the terminal backend behind the ordinary [`Platform`]
//! seam.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use rustnative_core::{Application, Platform, PlatformCapabilities, SystemClock};
use rustnative_drawlist::GridMapping;

use crate::color::DetectInput;
use crate::probe::TerminalProfile;
use crate::services::Channel;
use crate::session::{Session, SessionError, TuiConfig, capabilities_for};

/// Why a terminal application stopped.
#[derive(Debug)]
pub enum TuiError {
    /// There is no terminal, or it refused raw mode, or it went away.
    Terminal(std::io::Error),
    /// A component panicked under the terminating policy.
    ComponentPanicked(String),
}

impl fmt::Display for TuiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal(error) => write!(f, "terminal: {error}"),
            Self::ComponentPanicked(message) => write!(f, "a component panicked: {message}"),
        }
    }
}

impl std::error::Error for TuiError {}

/// Runs an [`Application`] in the terminal the process was started in.
///
/// ```no_run
/// use rustnative_core::{Application, Component, Event, Node, Platform, Size, Window};
/// use rustnative_tui::TuiPlatform;
/// # struct Hello;
/// # impl Component for Hello {
/// #     type Props = ();
/// #     type Message = ();
/// #     fn new((): ()) -> Self { Self }
/// #     fn props(&self) -> &() { &() }
/// #     fn set_props(&mut self, (): ()) {}
/// #     fn view(&self) -> Node { Node::label("hello", "Hello, terminal") }
/// #     fn update(&mut self, _event: Event) {}
/// # }
///
/// let channel = rustnative_tui::Channel::new();
/// let services = rustnative_tui::services("com.example.hello", "Hello", &channel)?;
/// let mut application = Application::with_services(Hello, Window::new("Hello", Size::new(0, 0)), services);
/// TuiPlatform::new().with_channel(channel).run(&mut application)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// The window's size is the terminal's: a terminal application is laid out
/// in whatever the terminal has, and follows it as it is resized.
#[derive(Debug, Clone)]
pub struct TuiPlatform {
    config: TuiConfig,
    channel: Arc<Channel>,
    app_id: Option<String>,
}

impl Default for TuiPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiPlatform {
    /// The terminal backend with the defaults and the environment's
    /// overrides (`TuiConfig::from_env`).
    #[must_use]
    pub fn new() -> Self {
        Self { config: TuiConfig::from_env(), channel: Channel::new(), app_id: None }
    }

    /// The same signature as every backend's.
    #[must_use]
    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }

    /// The application id, if one was set.
    #[must_use]
    pub fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }

    /// Lays out with cells of `width` × `height` logical pixels instead of
    /// 8 × 16 (`rustnative_style::TUI_UNITS`).
    #[must_use]
    pub const fn with_cell_size(mut self, width: u32, height: u32) -> Self {
        self.config.mapping = GridMapping::new(width, height);
        self
    }

    /// Replaces the whole configuration.
    #[must_use]
    pub const fn with_config(mut self, config: TuiConfig) -> Self {
        self.config = config;
        self
    }

    /// Uses `channel` — the one the services were built over
    /// ([`crate::services()`]) — so the clipboard and notifications reach
    /// this terminal.
    #[must_use]
    pub fn with_channel(mut self, channel: Arc<Channel>) -> Self {
        self.channel = channel;
        self
    }

    /// The services' channel.
    #[must_use]
    pub fn channel(&self) -> &Arc<Channel> {
        &self.channel
    }

    /// The configuration.
    #[must_use]
    pub const fn config(&self) -> TuiConfig {
        self.config
    }
}

impl Platform for TuiPlatform {
    type Error = TuiError;

    fn run(&mut self, application: &mut Application) -> Result<(), Self::Error> {
        let terminal = crate::term::open().map_err(TuiError::Terminal)?;
        let adopted = application.take();
        let mut session = Session::new(
            terminal,
            adopted,
            Arc::new(SystemClock),
            self.config,
            Arc::clone(&self.channel),
        );
        session.start();
        while !session.is_finished() {
            session.step(None);
        }
        session.finish();
        let failure = session.take_failure();
        let Session { app, terminal, .. } = session;
        drop(terminal);
        *application = app;
        match failure {
            None => Ok(()),
            Some(SessionError::Io(error)) => Err(TuiError::Terminal(error)),
            Some(SessionError::ComponentPanicked(message)) => {
                Err(TuiError::ComponentPanicked(message))
            }
        }
    }

    /// What this terminal is known to realize before it is asked: the
    /// environment's answer. A running session advertises what the
    /// terminal itself answered (`keys::CAPABILITIES`).
    fn capabilities(&self) -> PlatformCapabilities {
        let windows_vt = cfg!(windows);
        let profile = TerminalProfile::from_environment(
            &DetectInput::from_process(windows_vt),
            String::new(),
        );
        capabilities_for(&profile)
    }

    fn style_capabilities(&self) -> rustnative_core::StyleCapabilities {
        rustnative_style::TUI
    }

    fn unit_mapping(&self) -> Option<rustnative_core::UnitMapping> {
        Some(rustnative_style::TUI_UNITS)
    }

    fn native_extension(&self) -> &dyn Any {
        self
    }
}
