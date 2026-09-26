//! Portable platform-capability discovery.

use std::collections::BTreeSet;

/// A portable capability exposed by a platform adapter at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Reading/writing plain-text system clipboard content.
    Clipboard,
    /// Posting system notifications.
    Notifications,
    /// Capturing photos/video from a camera, and showing its preview
    /// ([`crate::HostContent::Camera`]).
    Camera,
    /// Embedded web content ([`crate::HostContent::Web`]).
    WebContent,
    /// Audio and video playback with the host's controls
    /// ([`crate::HostContent::Media`]).
    MediaPlayback,
    /// Bluetooth device access.
    Bluetooth,
    /// Persistent key-value storage.
    Storage,
    /// Geolocation.
    Location,
    /// Native open/save/pick-folder file dialogs.
    FileDialogs,
    /// The platform's native share sheet/dialog.
    SystemShare,
    /// Launching URLs with the system's default handler.
    UrlLaunch,
    /// Opening more than one top-level window.
    MultipleWindows,
    /// Programmatic window placement/state management.
    WindowManagement,
    /// Reading the system's light/dark appearance/accent-color settings.
    SystemAppearance,
    /// Drag-and-drop input.
    DragAndDrop,
    /// Native menu bars/context menus.
    Menus,
    /// Touch-screen contacts delivered as pointer events.
    Touch,
    /// Pen/stylus input delivered as pointer events (with pressure).
    Pen,
    /// Game-controller input.
    Gamepad,
    /// Input-method (IME) composition events for custom text targets.
    Ime,
    /// Time-based animation of native properties.
    Animations,
    /// Reporting the system's reduced-motion preference.
    ReducedMotionPreference,
    /// Canvas nodes drawn from a portable display list.
    CustomDrawing,
    /// Native surfaces handed to an application's own GPU renderer.
    NativeSurfaces,
    /// Durable storage for persisted component state.
    StatePersistence,
    /// Opening the application for a URL, and handing a second launch's
    /// URL to the running instance.
    DeepLinks,
    /// Reporting suspend, resume, and termination.
    Lifecycle,
    /// Pointer cursor shapes per node.
    Cursors,
    /// Pointer hover (enter/leave without a press).
    Hover,
    /// Keyboard shortcuts bound to commands, working from anywhere in a
    /// window.
    CommandShortcuts,
    /// Right-to-left layout, mirrored by the host or by the backend.
    RightToLeft,
    /// The host's settings (colour scheme, text scale, contrast, locale)
    /// fed into the environment and followed when they change.
    HostTraits,
    /// Permission states and requests beyond a yes/no answer.
    Permissions,
    /// A surface beyond the main window (`C49-1`), realized by Milestone 57.
    Surface(SurfaceKind),
    /// Printing documents ([`crate::industrial::PrintService`]).
    Printing,
    /// Serial ports ([`crate::industrial::SerialService`]).
    SerialPorts,
    /// A hardware accelerator for compute and inference (`C89-1`),
    /// answered from what the machine actually has.
    Accelerator(AcceleratorKind),
}

impl Capability {
    /// Every capability a backend can answer, in declaration order — what
    /// inspection reports against and `rustnative describe` lists.
    pub const ALL: &'static [Self] = &[
        Self::Clipboard,
        Self::Notifications,
        Self::Camera,
        Self::WebContent,
        Self::MediaPlayback,
        Self::Bluetooth,
        Self::Storage,
        Self::Location,
        Self::FileDialogs,
        Self::SystemShare,
        Self::UrlLaunch,
        Self::MultipleWindows,
        Self::WindowManagement,
        Self::SystemAppearance,
        Self::DragAndDrop,
        Self::Menus,
        Self::Touch,
        Self::Pen,
        Self::Gamepad,
        Self::Ime,
        Self::Animations,
        Self::ReducedMotionPreference,
        Self::CustomDrawing,
        Self::NativeSurfaces,
        Self::StatePersistence,
        Self::DeepLinks,
        Self::Lifecycle,
        Self::Cursors,
        Self::Hover,
        Self::CommandShortcuts,
        Self::RightToLeft,
        Self::HostTraits,
        Self::Permissions,
        Self::Surface(SurfaceKind::Widget),
        Self::Surface(SurfaceKind::LiveActivity),
        Self::Surface(SurfaceKind::Tile),
        Self::Surface(SurfaceKind::Extension),
        Self::Surface(SurfaceKind::InstantApp),
        Self::Surface(SurfaceKind::CompanionDevice),
        Self::Surface(SurfaceKind::TrayExtra),
        Self::Surface(SurfaceKind::JumpList),
        Self::Surface(SurfaceKind::TaskbarProgress),
        Self::Printing,
        Self::SerialPorts,
        Self::Accelerator(AcceleratorKind::Gpu),
        Self::Accelerator(AcceleratorKind::Npu),
    ];
}

/// Kinds of compute accelerator (`C89-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum AcceleratorKind {
    /// A graphics processor usable for general compute.
    Gpu,
    /// A neural processing unit.
    Npu,
}

/// Surfaces an application can have beyond its windows (`C49-1`). Every
/// backend answers each one honestly from its first day; realizing them is
/// Milestone 57.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SurfaceKind {
    /// A home-screen or desktop widget.
    Widget,
    /// A live activity or ongoing notification.
    LiveActivity,
    /// A tile (a quick-settings tile, a live tile).
    Tile,
    /// A share or action extension.
    Extension,
    /// An instant, install-free application.
    InstantApp,
    /// A companion-device surface (a watch face, a car display).
    CompanionDevice,
    /// A tray icon or menu-bar extra.
    TrayExtra,
    /// A jump list or dock menu.
    JumpList,
    /// Progress shown on the taskbar or dock icon.
    TaskbarProgress,
}

/// The set of [`Capability`]s one platform adapter actually realizes.
///
/// A backend should only ever advertise a capability once it genuinely
/// realizes the corresponding behavior — never speculatively, and never for
/// a portable contract that exists in `rustnative-core` but has no native
/// implementation yet. See `rustnative-windows`'s own
/// `capabilities_only_advertise_realized_backend_features` test for how a
/// backend is expected to enforce this about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformCapabilities {
    available: BTreeSet<Capability>,
}

impl PlatformCapabilities {
    /// Creates a capability set from the capabilities a backend realizes.
    pub fn new(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self { available: capabilities.into_iter().collect() }
    }

    /// Returns whether `capability` is realized.
    #[must_use]
    pub fn supports(&self, capability: Capability) -> bool {
        self.available.contains(&capability)
    }

    /// Iterates every realized capability, in a stable order.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.available.iter().copied()
    }
}
