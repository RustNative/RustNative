//! Platform-independent primitives for the framework.
//!
//! The core owns application components, the declarative UI tree, events,
//! layout, styling, structured concurrency, and tree diffing. Platform
//! crates (e.g. `rustnative-windows`) translate the resulting model into
//! native objects; **this crate never talks to an operating system** — it
//! has no `cfg(windows)`/`cfg(target_os = ...)` code and no dependency on a
//! native windowing/graphics API. Its only non-`std` dependencies are
//! portable: `tokio` (an OS-abstracted async runtime, not a UI binding),
//! `parking_lot`, and `async-trait`.
//!
//! # Getting started
//!
//! A component owns state and describes UI as a function of it; an
//! `Application` owns one or more windows' component trees; a platform
//! backend drives the whole thing. Only the last step is platform-specific.
//!
//! ```
//! use rustnative_core::{Application, Component, Event, Node, NodeId, Size, Window};
//!
//! struct Greeter {
//!     greeted: bool,
//! }
//!
//! impl Component for Greeter {
//!     type Props = ();
//!     type Message = ();
//!
//!     fn new((): Self::Props) -> Self {
//!         Self { greeted: false }
//!     }
//!     fn props(&self) -> &Self::Props {
//!         &()
//!     }
//!     fn set_props(&mut self, (): Self::Props) {}
//!
//!     fn view(&self) -> Node {
//!         Node::column(
//!             "root",
//!             [
//!                 Node::label("greeting", if self.greeted { "Hello!" } else { "..." }),
//!                 Node::button("greet", "Greet"),
//!             ],
//!         )
//!     }
//!
//!     fn update(&mut self, event: Event) {
//!         if matches!(event, Event::Click { target } if target == NodeId::from_key("greet")) {
//!             self.greeted = true;
//!         }
//!     }
//! }
//!
//! let mut application =
//!     Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//!
//! // A backend would deliver this from a real click; here it stands in for
//! // one, which is also how a headless test drives a component.
//! application.dispatch(Event::Click { target: NodeId::from_key("greet") });
//!
//! let Node::Column(root) = application.view() else { panic!("the root is a column") };
//! let Node::Label(label) = &root.children()[0] else { panic!("first child is the label") };
//! assert_eq!(label.text(), "Hello!");
//!
//! // The greeted view, in markup:
//! let markup = rustnative_core::rsx! {
//!     <Column key="root">
//!         <Label key="greeting" text="Hello!" />
//!         <Button key="greet" text="Greet" />
//!     </Column>
//! };
//! assert_eq!(markup, application.view());
//! ```
//!
//! On Windows, the last line of `main` hands it to the backend:
//!
//! ```no_run
//! # use rustnative_core::{Application, Component, Event, Node, Platform, Size, Window};
//! # struct Greeter;
//! # impl Component for Greeter {
//! #     type Props = ();
//! #     type Message = ();
//! #     fn new((): Self::Props) -> Self { Self }
//! #     fn props(&self) -> &Self::Props { &() }
//! #     fn set_props(&mut self, (): Self::Props) {}
//! #     fn view(&self) -> Node { Node::label("greeting", "Hello") }
//! #     fn update(&mut self, _event: Event) {}
//! # }
//! # fn run<P: Platform>(platform: &mut P) -> Result<(), P::Error> {
//! let mut application =
//!     Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//! // e.g. `rustnative_windows::WindowsPlatform::new().run(&mut application)?;`
//! platform.run(&mut application)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Module map
//!
//! | Module | Owns |
//! |---|---|
//! | [`animation`] | Easing, springs, and the [`Timeline`] a backend drives frames from |
//! | [`accessibility`] | The portable accessibility model and its [`AccessibilityTree`] projection |
//! | [`identity`] | [`NodeId`]/[`ComponentId`]/[`WindowId`] allocation and the collision-free key interner |
//! | [`event`] | [`Event`], keyboard/accessibility types |
//! | [`graphics`] | Canvas draw lists and native surfaces: the custom-rendering escape hatch |
//! | [`input`] | Pointer/wheel/gesture/IME/clipboard/drag/gamepad payloads, [`GestureRecognizer`], [`GamepadPoller`] |
//! | [`node`] | The declarative [`Node`] tree a component's `view`/`render` returns |
//! | [`component`] | [`Component`], [`ComponentContext`], and the framework-managed [`ComponentTree`] |
//! | [`reconcile`] | Snapshotting a [`Node`] tree and diffing two snapshots |
//! | [`layout`] | Geometry, per-node constraints, intrinsic measurement, and the layout engine |
//! | [`style`] | [`Theme`] and per-node [`VisualStyle`] resolution |
//! | [`scheduler`] | Structured concurrency: [`Scheduler`], [`TaskScope`], the pluggable [`Executor`] backend |
//! | [`services`] | Platform-independent service contracts (HTTP, storage, clipboard, ...) |
//! | [`virtualization`] | Realizing only the visible window of a very long list |
//! | [`navigation`] | Routes, navigation stacks, and tabs over the managed component tree |
//! | [`lifecycle`] | Suspend, resume, and terminate, as the platform reports them |
//! | [`persistence`] | Component state kept across runs, buffered and flushed at lifecycle points |
//! | [`window`]/[`menu`] | Window-domain state and native menu definitions |
//! | [`mod@panic`] | What an application does when a component panics |
//! | [`clock`] | The host clock every timestamp is read from |
//! | [`environment`] | Typed values flowing down the tree, and preferences flowing up |
//! | [`command`] | Actions with identity, bound by menus, buttons, and shortcuts |
//! | [`permission`] | Permission states as hosts report them, and the request flow |
//! | [`grant`] | Scoped grants: which services a part of the application may reach |
//! | [`handle`]/[`affinity`] | The escape-hatch contract, and thread affinity |
//! | [`teardown`] | What a backend restores on exit and on panic |
//! | [`application`] | Multi-window orchestration |
//! | [`capability`]/[`platform`] | The seam a platform backend implements and declares support through |
//!
//! Each module owns one coherent responsibility and depends only on the
//! modules "below" it in the table above; nothing here depends back up
//! toward [`application`] except [`platform`] (which must, since it is the
//! seam a backend drives an `Application` through) — see the standards
//! audit's P1.21/P2.22 findings on module boundaries, which this structure
//! is a direct response to.
//!
//! The public API re-exported below is intentionally flat
//! (`rustnative_core::NodeId`, `rustnative_core::Component`, ...) even though
//! the implementation is now modular: this keeps every existing call site
//! in `rustnative-windows`, the example, and application code unchanged
//! across the internal reorganization.
//!
//! # Documentation coverage
//!
//! Every public item — including individual struct fields and enum
//! variants, not just the types that contain them — carries a doc comment,
//! enforced permanently by `#![deny(missing_docs)]` below (standards audit
//! P2.23, completed in full: an earlier pass documented every type but left
//! the lint itself un-enabled, deferring roughly 380 field/variant-level
//! warnings as a "legitimate, bounded follow-up"; that follow-up is done,
//! and the lint is now `deny` rather than `warn` so it cannot silently
//! regress).
#![deny(missing_docs)]

/// The markup syntax: elements with typed attributes, nested children, and
/// Rust in braces, evaluating to a [`Node`] (`PLAN.md` 2.9).
///
/// | Markup | Builder |
/// |---|---|
/// | `<Column key="k" padding={p}>…</Column>` | `Node::column_with_layout("k", children, LayoutStyle::default(), ColumnStyle::default().padding(p))` |
/// | `<Label key="k" text="Hi" width={w} />` | `Node::label_with_layout("k", "Hi", LayoutStyle::default().width(w))` |
/// | `disabled`, `hidden` | `.disabled(true)`, `.hidden(true)` |
/// | `accessibility={a}` (any `with_*` modifier) | `.with_accessibility(a)` |
/// | `..{f}` | `f(node)` — any `FnOnce(Node) -> Node` |
/// | `{expr}` in child position | a `Node`, or anything iterable over `Node`s |
/// | `if`/`else`, `match`, `for`, `<>…</>` | the same Rust control flow over children |
/// | `<Screen key="s" prop={v} />` | `context.child_with_props::<Screen, _>("s", Props { prop: v }, Screen::new)` |
///
/// Builder:
///
/// ```
/// use rustnative_core::{ColumnStyle, EdgeInsets, LayoutStyle, Node};
///
/// let items = ["one", "two"];
/// let builder = Node::column_with_layout(
///     "list",
///     items.iter().map(|item| Node::label(*item, *item)),
///     LayoutStyle::default(),
///     ColumnStyle::default().padding(EdgeInsets::all(8)),
/// );
/// # let _ = builder;
/// ```
///
/// Markup (in a `.rsx` file, the same element with no `rsx!` around it):
///
/// ```
/// use rustnative_core::{EdgeInsets, Node, rsx};
///
/// let items = ["one", "two"];
/// let markup: Node = rsx! {
///     <Column key="list" padding={EdgeInsets::all(8)}>
///         for item in items {
///             <Label key={item} text={item} />
///         }
///     </Column>
/// };
/// # let builder = rustnative_core::Node::column_with_layout(
/// #     "list",
/// #     items.iter().map(|item| Node::label(*item, *item)),
/// #     rustnative_core::LayoutStyle::default(),
/// #     rustnative_core::ColumnStyle::default().padding(EdgeInsets::all(8)),
/// # );
/// assert_eq!(markup, builder);
/// ```
#[cfg(feature = "markup")]
pub use rustnative_macros::rsx;

// Lets `rsx!` expansions (which name `::rustnative_core`) work inside this
// crate's own tests as they do in every other crate.
extern crate self as rustnative_core;

/// Declares a module written in a `.rsx` file, which
/// `rustnative_build::compile_rsx()` lowered into `OUT_DIR`: the markup
/// counterpart of `mod name;`.
///
/// ```ignore
/// rustnative_core::rsx_mod!(inbox); // src/inbox.rsx
/// ```
#[macro_export]
macro_rules! rsx_mod {
    ($(#[$attribute:meta])* $visibility:vis $name:ident) => {
        $(#[$attribute])*
        $visibility mod $name {
            include!(concat!(env!("OUT_DIR"), "/rsx/", stringify!($name), ".rs"));
        }
    };
}

/// Declares the module `rustnative_build::compile_messages()` generated from
/// `locales/*.ftl` (`PLAN.md` Milestone 46): `messages::catalogues()` and
/// one typed function per message.
///
/// ```ignore
/// rustnative_core::messages_mod!();
/// // Node::label("count", messages::inbox_count(n))
/// ```
#[macro_export]
macro_rules! messages_mod {
    () => {
        /// The application's messages, generated from `locales/*.ftl`.
        pub mod messages {
            include!(concat!(env!("OUT_DIR"), "/messages.rs"));
        }
    };
}

/// Utility classes — the vocabulary of Tailwind CSS v4.1.13 — compiled
/// into a [`style::DeclarationSet`] (`PLAN.md` 2.14, Milestone 58).
///
/// An unknown class is a compile error naming the nearest one; a class
/// setting a property the target's backend cannot realize (a shadow, on
/// Windows) is a compile error for that target, at the class. Tokens and
/// project utilities come from the crate's `app.css` (or the file
/// `rustnative.toml`'s `[style] file` names) over the default theme.
///
/// The two spellings of one style, which resolve to the same node:
///
/// ```
/// use rustnative_core::{Application, Color, Node, Size, Theme, VisualStyle, Window, classes};
/// use rustnative_core::{Component, Event};
///
/// struct Card;
/// impl Component for Card {
///     type Props = ();
///     type Message = ();
///     fn new((): ()) -> Self { Self }
///     fn props(&self) -> &() { &() }
///     fn set_props(&mut self, (): ()) {}
///     fn view(&self) -> Node {
///         Node::column("card", [
///             Node::label("utility", "Utility").with_class(classes!("text-white bg-[#1e90ff] rounded-[6px]")),
///             Node::label("typed", "Typed").with_style(
///                 VisualStyle::new()
///                     .foreground(Color::rgb(255, 255, 255))
///                     .background(Color::rgb(0x1e, 0x90, 0xff))
///                     .border_radius(6),
///             ),
///         ])
///     }
///     fn update(&mut self, _: Event) {}
/// }
///
/// let application = Application::new(Card, Window::new("Card", Size::new(200, 100)));
/// let Node::Column(card) = application.view() else { unreachable!() };
/// assert_eq!(card.children()[0].visual_style(), card.children()[1].visual_style());
///
/// // The same classes in markup:
/// let markup = rustnative_core::rsx! { <Label key="utility" text="Utility" class="text-white bg-[#1e90ff] rounded-[6px]" /> };
/// assert_eq!(markup.declarations(), Node::label("utility", "Utility")
///     .with_class(classes!("text-white bg-[#1e90ff] rounded-[6px]")).declarations());
/// ```
#[cfg(feature = "markup")]
pub use rustnative_macros::classes;

/// A declaration block — `padding: 1rem; color: var(--color-red-500)` —
/// compiled into a [`style::DeclarationSet`], with the same checks as
/// [`classes!`]. The markup spelling is `style="…"` with a string.
#[cfg(feature = "markup")]
pub use rustnative_macros::styles;

/// The theme `rustnative_build::compile_styles()` compiled from the
/// project's `app.css`: the default theme with the file's tokens over it.
///
/// ```ignore
/// let application = Application::new(App::new(()), window);
/// application.set_theme(rustnative_core::app_theme!());
/// ```
#[macro_export]
macro_rules! app_theme {
    () => {
        include!(concat!(env!("OUT_DIR"), "/app_theme.rs"))
    };
}
pub mod accessibility;
pub mod affinity;
pub mod animation;
pub mod api_schema;
pub mod application;
pub mod capability;
pub mod clock;
pub mod command;
pub mod component;
pub mod control;
pub mod dev;
pub mod environment;
pub mod event;
pub mod grant;
pub mod graphics;
pub mod handle;
mod host_content;
pub mod i18n;
pub mod identity;
pub mod industrial;
pub mod input;
pub mod inspect;
pub mod layout;
pub mod lifecycle;
pub mod localization;
pub mod menu;
pub mod navigation;
pub mod node;
pub mod package;
pub mod panic;
pub mod perf;
pub mod permission;
pub mod persistence;
pub mod platform;
pub mod preview;
pub mod product;
pub mod reconcile;
pub mod scheduler;
pub mod server_fn;
pub mod services;
pub mod state;
pub mod style;
pub mod surfaces;
pub mod teardown;
mod text_profile;
pub mod virtualization;
pub mod window;
pub mod wire;

pub use accessibility::{
    AccessibilityTree, AccessibleAction, AccessibleActionKind, AccessibleNode, AccessibleValue,
    CheckedState, LiveRegion, Relation, VirtualElement,
};
pub use affinity::{ThreadAffinity, UiThread};
pub use animation::{
    AnimatedOverrides, AnimatedProperty, AnimatedValue, Animation, AnimationId, AnimationOwner,
    Easing, Fill, Finished, Frame, FrameClock, ManualFrameClock, MatchedGeometry, MotionPreference,
    NodeOverrides, ReducedMotion, Repeat, TickOutput, Timeline, Transition, matched_geometry,
};
pub use application::Application;
pub use capability::{Capability, PlatformCapabilities, SurfaceKind};
pub use clock::{Clock, ManualClock, SystemClock};
pub use command::{Command, CommandId, CommandRegistry, Shortcut};
pub use component::{
    AnimationRequest, AnimationRequests, Callback, Component, ComponentContext, ComponentHost,
    ComponentTree, Deferred, EffectCleanup, EffectContext, Failure, InputRequest, InputRequests,
    Pure, PureComponent, RenderCause, RenderError, RenderRecord, WindowRequests,
};
pub use control::{CalendarDate, Control};
pub use environment::{
    Breakpoint, ColorScheme, Contrast, EnvKey, EnvValue, Environment, Locale, PointerPrecision,
    Posture, Preference, PreferenceKey, SizeClass, SizeClasses, WindowMode, keys,
};
pub use event::{AccessibilityInfo, AccessibilityRole, Event, KeyCode, KeyModifiers};
pub use grant::{Grant, GrantSet, Granted, ScopedServices};
pub use graphics::{
    DrawCommand, DrawList, ImageData, ImageError, Paint, Path, PathSegment, RectF, SurfaceId,
    Transform2D, Vec2,
};
pub use handle::{Live, NativeHandle, StaleHandle, Unchecked};
pub use host_content::{HostContent, host_content};
pub use identity::{ComponentId, NodeId, WindowId};
pub use input::{
    ClipboardAction, Composition, Cursor, DragData, DropEffect, GamepadAxis, GamepadButton,
    GamepadInput, GamepadPoller, GamepadSource, GamepadState, Gesture, GestureConfig,
    GestureConflict, GesturePhase, GesturePolicy, GestureRecognizer, InputInterest, PointerButton,
    PointerButtons, PointerEvent, PointerKind, PointerPhase, Scalar, WheelDelta, Winner, arbitrate,
};
pub use layout::{
    Alignment, ColumnStyle, Constraints, DefaultIntrinsicMeasurer, EdgeInsets, GridPlacement,
    GridStyle, IntrinsicMeasurer, LayoutDirection, LayoutEngine, LayoutInvalidation, LayoutResult,
    LayoutStyle, MeasuredItem, Overflow, Point, Rect, RowStyle, Size, SizeMode, Track,
};
pub use lifecycle::Lifecycle;
pub use menu::{MenuBar, MenuItem};
pub use navigation::{
    EntryId, NavigationCommand, NavigationEntry, NavigationStack, Navigator, Route, RouteError,
    RouteParams, Router, url_path,
};
pub use node::{
    Button, Canvas, Column, ControlNode, IntoChildren, Label, Node, NodeKind, NodeTransition, Row,
    Surface, SurfaceContent, TabBar, Tabs, TextInput, TreeError,
};
pub use panic::{PanicAction, PanicPolicy, PanicReport};
pub use permission::{FixedPermissions, Permission, PermissionService, PermissionState};
pub use persistence::{MemoryStateStore, Persisted, StateStore};
pub use platform::{Platform, UnsupportedPlatform};
pub use reconcile::{TreeDiff, TreeNode, TreeOp, TreeSnapshot};
#[cfg(feature = "threads")]
pub use scheduler::TokioExecutor;
pub use scheduler::{
    Background, Executor, ExecutorHandle, HostExecutor, LocalBoxedTask, LocalExecutor, LocalPool,
    ManualExecutor, Offloaded, Priority, Scheduler, SleepFuture, Supervised, SupervisionPolicy,
    SuspendRule, TaskFailure, TaskHandle, TaskId, TaskScope,
};
pub use services::{
    CertificatePins, ClipboardService, FileDialogKind, FileDialogRequest, FileDialogService,
    HttpRequest, HttpResponse, HttpService, MemoryClipboard, MemoryStorage, Method, ServiceError,
    Services, StorageService, SystemService,
};
pub use state::{Derived, Store, StoreSnapshot};
pub use style::{
    Color, ComponentStyle, ControlState, DeclarationSet, HostPalette, HostRole, ResolvedStyle,
    ShadowLayer, StateStyles, StyleCapabilities, StyleOverride, StyleProperty, StyleSupport,
    StyleValue, Theme, TokenTable, Typography, UnitMapping, VisualStyle,
};
pub use teardown::{Restoration, TeardownPolicy};
pub use text_profile::{Script, TextProfile, UnsupportedText};
pub use virtualization::{
    Axis, ExtentCache, ItemExtent, ScrollAnchor, VirtualListStyle, VirtualRange,
};
pub use window::{Window, WindowPresentation, WindowState};
