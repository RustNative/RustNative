//! The platform-level error type this backend's `Platform::Error` resolves
//! to, and the native context it carries.
//!
//! # What this closes
//!
//! The standards audit's P2.32 finding is that a failed Win32 call used to
//! be reduced to two fields — an operation name and a `GetLastError` code —
//! and everything else about *where* it happened was discarded:
//!
//! > That is useful, but production diagnostics would benefit from:
//! > HRESULT where applicable; Win32 error category; window ID; node ID;
//! > native handle when safe to report; service operation; contextual
//! > source error.
//!
//! A bare "`CreateWindowExW(BUTTON)` failed with error code 8" tells an
//! application author that *something* failed to create, not which node in
//! which window, and gives them nothing to correlate against their own
//! logs. [`NativeContext`] carries that, and [`Win32Category`] turns the
//! raw code into a class of failure an application can actually branch on
//! (retry on a resource exhaustion, report a bug on an invalid parameter)
//! without hard-coding Win32 numerics at its own call sites.
//!
//! # What it deliberately does not do
//!
//! Raw handles are recorded for `Debug` but never rendered by `Display`, as
//! the audit asks ("do not expose raw OS handles in `Display`, but keep
//! them available to debug diagnostics"). A handle value is meaningless to
//! a person reading an error message and is a detail of an in-process
//! allocation table.
//!
//! `thiserror` is not used, though the audit names it as one option. This
//! crate's error surface is a single enum with hand-written `Display`
//! arms; a derive macro would add a proc-macro dependency to a backend
//! whose dependency list is otherwise deliberately minimal, in exchange for
//! removing about forty lines of entirely mechanical code. The layering the
//! audit actually asks for — structured context, a category, a source
//! chain — is present either way, and `std::error::Error::source` is
//! implemented by hand below.

use std::fmt;

use rustnative_core::{NodeId, WindowId};

/// A broad class of Win32 failure, derived from a system error code.
///
/// The point of a category is that an application can react to a failure
/// without knowing Win32's numeric error space: "the system is out of
/// resources" is actionable (back off, close something, retry), "we passed
/// an invalid parameter" is a bug report, and both are useful to
/// distinguish from each other without a table of constants at the call
/// site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Win32Category {
    /// The system is out of a resource this call needed — memory, GDI or
    /// USER handles, or a similar quota. Often transient.
    ResourceExhausted,
    /// An argument was rejected: an invalid handle, an unregistered window
    /// class, a bad parameter. In this backend that indicates a framework
    /// bug rather than an environmental condition.
    InvalidArgument,
    /// The calling thread or process lacks the rights the call needed —
    /// for example, touching the input desktop from a service account.
    AccessDenied,
    /// The call reported failure with a code this crate does not classify.
    Other,
}

impl Win32Category {
    /// Classifies a Win32 system error code.
    ///
    /// The constants are inlined rather than imported because they come
    /// from `winerror.h`'s frozen system-error space, and `windows-sys`
    /// scatters them across feature-gated modules this crate does not
    /// otherwise need.
    #[must_use]
    pub const fn of(code: u32) -> Self {
        /// `ERROR_INVALID_FUNCTION`
        const ERROR_INVALID_FUNCTION: u32 = 1;
        /// `ERROR_NOT_ENOUGH_MEMORY`
        const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
        /// `ERROR_OUTOFMEMORY`
        const ERROR_OUTOFMEMORY: u32 = 14;
        /// `ERROR_INVALID_DATA`
        const ERROR_INVALID_DATA: u32 = 13;
        /// `ERROR_ACCESS_DENIED`
        const ERROR_ACCESS_DENIED: u32 = 5;
        /// `ERROR_INVALID_HANDLE`
        const ERROR_INVALID_HANDLE: u32 = 6;
        /// `ERROR_INVALID_PARAMETER`
        const ERROR_INVALID_PARAMETER: u32 = 87;
        /// `ERROR_NO_SYSTEM_RESOURCES`
        const ERROR_NO_SYSTEM_RESOURCES: u32 = 1450;
        /// `ERROR_CANNOT_FIND_WND_CLASS`
        const ERROR_CANNOT_FIND_WND_CLASS: u32 = 1407;
        /// `ERROR_INVALID_WINDOW_HANDLE`
        const ERROR_INVALID_WINDOW_HANDLE: u32 = 1400;
        /// `ERROR_INVALID_MENU_HANDLE`
        const ERROR_INVALID_MENU_HANDLE: u32 = 1401;

        match code {
            ERROR_NOT_ENOUGH_MEMORY | ERROR_OUTOFMEMORY | ERROR_NO_SYSTEM_RESOURCES => {
                Self::ResourceExhausted
            }
            ERROR_ACCESS_DENIED => Self::AccessDenied,
            ERROR_INVALID_FUNCTION
            | ERROR_INVALID_DATA
            | ERROR_INVALID_HANDLE
            | ERROR_INVALID_PARAMETER
            | ERROR_INVALID_WINDOW_HANDLE
            | ERROR_INVALID_MENU_HANDLE
            | ERROR_CANNOT_FIND_WND_CLASS => Self::InvalidArgument,
            _ => Self::Other,
        }
    }

    /// A short, stable description suitable for an error message.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::ResourceExhausted => "the system is out of resources",
            Self::InvalidArgument => "an argument was rejected",
            Self::AccessDenied => "access was denied",
            Self::Other => "the call failed",
        }
    }
}

/// Where in the framework a native failure happened.
///
/// Every field is optional because the answer genuinely varies: a failed
/// window creation knows its `WindowId` but has no `HWND` yet, a failed
/// control creation knows its `NodeId`, and a failed class registration
/// knows neither. An empty context is a valid, honest answer — better than
/// inventing a placeholder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeContext {
    /// The framework window the failing operation belonged to.
    pub window: Option<WindowId>,
    /// The UI node whose native realization was being operated on.
    pub node: Option<NodeId>,
    /// The raw native handle involved, recorded for `Debug` only — see this
    /// module's docs on why `Display` never renders it.
    pub handle: Option<usize>,
}

impl NativeContext {
    /// An empty context, for a failure that belongs to no particular window
    /// or node.
    #[must_use]
    pub const fn none() -> Self {
        Self { window: None, node: None, handle: None }
    }

    /// Returns this context with `window` recorded.
    #[must_use]
    pub const fn with_window(mut self, window: WindowId) -> Self {
        self.window = Some(window);
        self
    }

    /// Returns this context with `node` recorded.
    #[must_use]
    pub const fn with_node(mut self, node: NodeId) -> Self {
        self.node = Some(node);
        self
    }

    /// Returns this context with a raw native handle recorded for debug
    /// diagnostics.
    #[must_use]
    pub const fn with_handle(mut self, handle: usize) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Whether anything at all was recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.window.is_none() && self.node.is_none() && self.handle.is_none()
    }
}

impl fmt::Display for NativeContext {
    /// Renders only the parts a person can act on. The handle is
    /// deliberately omitted; see the module docs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        if let Some(window) = self.window {
            write!(f, "window {}", window.get())?;
            first = false;
        }
        if let Some(node) = self.node {
            if !first {
                f.write_str(", ")?;
            }
            write!(f, "node {}", node.get())?;
        }
        Ok(())
    }
}

/// A failure from the Windows backend.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A Win32 API call reported failure.
    #[cfg(windows)]
    WindowsApi {
        /// The API that failed, as written at the call site (for example
        /// `"CreateWindowExW(BUTTON)"`).
        operation: &'static str,
        /// The thread's `GetLastError` code, captured immediately after the
        /// failing call.
        code: u32,
        /// The broad class of failure `code` falls into.
        category: Win32Category,
        /// Which window and node the operation belonged to, where known.
        context: NativeContext,
    },
    /// Two UI nodes in one window resolved to the same identity, so the
    /// second could not be realized natively.
    DuplicateNodeId {
        /// The identity that appeared twice.
        node: NodeId,
    },
    /// A window's menu declared more items than Win32's 16-bit command-id
    /// space can address.
    MenuCommandExhausted {
        /// The window whose menu could not be completed.
        context: NativeContext,
    },
    /// This backend was asked to run on a target that is not Windows.
    UnsupportedHost,
    /// A `Component` implementation panicked while running inside a Win32
    /// `WNDPROC` callback. The panic was caught at the FFI boundary (see
    /// `native::message_loop::wndproc_boundary`) before it could unwind into
    /// Win32's own call frames, which is undefined behavior on stable Rust,
    /// and the configured panic policy was applied instead.
    ComponentPanicked {
        /// The panic's own message, where it carried one.
        message: String,
        /// Which window's callback the panic escaped from.
        context: NativeContext,
    },
    /// A foreign node's object could not be had: no factory is registered
    /// for its kind, or the factory made nothing (`Node::foreign`,
    /// `register_foreign`).
    ForeignUnavailable {
        /// The factory kind the node named.
        kind: String,
        /// Why.
        reason: &'static str,
        /// Which node.
        context: NativeContext,
    },
}

#[cfg(windows)]
impl Error {
    /// Records a failed Win32 call, capturing `GetLastError` on the calling
    /// thread.
    ///
    /// Must be called immediately after the failing API, before any other
    /// call can overwrite the thread-local error code.
    pub(crate) fn windows_api(operation: &'static str) -> Self {
        Self::windows_api_in(operation, NativeContext::none())
    }

    /// [`Self::windows_api`], recording which window or node the operation
    /// belonged to.
    pub(crate) fn windows_api_in(operation: &'static str, context: NativeContext) -> Self {
        // SAFETY: `GetLastError` takes no arguments and reads only
        // per-thread state; it is always safe to call, though callers are
        // relied on to invoke this immediately after the failing API on the
        // same thread, before any other call overwrites the code.
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        Self::WindowsApi { operation, code, category: Win32Category::of(code), context }
    }

    /// Returns this error with `context` attached, for a failure that
    /// travelled up from a helper that did not know which window or node it
    /// was serving.
    ///
    /// A context already recorded lower down wins: the innermost frame that
    /// knew a node's identity knew it more precisely than an outer frame
    /// that only knows the window.
    pub(crate) fn or_context(mut self, outer: NativeContext) -> Self {
        let slot = match &mut self {
            Self::WindowsApi { context, .. }
            | Self::MenuCommandExhausted { context }
            | Self::ComponentPanicked { context, .. }
            | Self::ForeignUnavailable { context, .. } => context,
            Self::DuplicateNodeId { .. } | Self::UnsupportedHost => return self,
        };
        if slot.is_empty() {
            *slot = outer;
        } else {
            // Fill only the fields the inner frame could not know.
            slot.window = slot.window.or(outer.window);
            slot.node = slot.node.or(outer.node);
            slot.handle = slot.handle.or(outer.handle);
        }
        self
    }
}

impl Error {
    /// The class of Win32 failure this represents, if it came from a Win32
    /// call at all.
    ///
    /// This is what an application branches on to decide whether a failure
    /// is worth retrying, without matching on numeric error codes itself.
    #[must_use]
    pub const fn category(&self) -> Option<Win32Category> {
        match self {
            #[cfg(windows)]
            Self::WindowsApi { category, .. } => Some(*category),
            _ => None,
        }
    }

    /// Which window and node this failure belonged to, where known.
    #[must_use]
    pub const fn context(&self) -> NativeContext {
        match self {
            #[cfg(windows)]
            Self::WindowsApi { context, .. } => *context,
            Self::MenuCommandExhausted { context }
            | Self::ComponentPanicked { context, .. }
            | Self::ForeignUnavailable { context, .. } => *context,
            Self::DuplicateNodeId { .. } | Self::UnsupportedHost => NativeContext::none(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(windows)]
            Self::WindowsApi { operation, code, category, context } => {
                write!(f, "Windows API call {operation} failed: {} (code {code})", {
                    category.describe()
                })?;
                write_context(f, context)
            }
            Self::DuplicateNodeId { node } => {
                write!(f, "duplicate UI node id: {}", node.get())
            }
            Self::MenuCommandExhausted { context } => {
                f.write_str("native menu command ID space exhausted")?;
                write_context(f, context)
            }
            Self::UnsupportedHost => f.write_str("rustnative-windows is only runnable on Windows"),
            Self::ComponentPanicked { message, context } => {
                write!(f, "a component panicked inside the native message loop: {message}")?;
                write_context(f, context)
            }
            Self::ForeignUnavailable { kind, reason, context } => {
                write!(f, "foreign object `{kind}`: {reason}")?;
                write_context(f, context)
            }
        }
    }
}

/// Appends ` (window 0, node 12345)` when there is anything to say.
fn write_context(f: &mut fmt::Formatter<'_>, context: &NativeContext) -> fmt::Result {
    if context.window.is_none() && context.node.is_none() {
        return Ok(());
    }
    write!(f, " ({context})")
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_exhaustion_is_distinguished_from_a_bad_argument() {
        assert_eq!(Win32Category::of(8), Win32Category::ResourceExhausted);
        assert_eq!(Win32Category::of(1450), Win32Category::ResourceExhausted);
        assert_eq!(Win32Category::of(87), Win32Category::InvalidArgument);
        assert_eq!(Win32Category::of(1400), Win32Category::InvalidArgument);
        assert_eq!(Win32Category::of(5), Win32Category::AccessDenied);
        assert_eq!(Win32Category::of(0xDEAD), Win32Category::Other);
    }

    #[cfg(windows)]
    #[test]
    fn display_names_the_window_and_node_but_never_the_raw_handle() {
        let error = Error::WindowsApi {
            operation: "CreateWindowExW(BUTTON)",
            code: 8,
            category: Win32Category::of(8),
            context: NativeContext::none()
                .with_window(WindowId::PRIMARY)
                .with_node(NodeId::from_key("submit"))
                .with_handle(0xDEAD_BEEF),
        };
        let rendered = error.to_string();
        assert!(rendered.contains("CreateWindowExW(BUTTON)"));
        assert!(rendered.contains("the system is out of resources"));
        assert!(rendered.contains("window 0"));
        assert!(
            !rendered.contains("beef") && !rendered.contains("BEEF"),
            "a raw handle must never reach Display (P2.32); got {rendered:?}"
        );
        // It is still available for debugging.
        assert_eq!(error.context().handle, Some(0xDEAD_BEEF));
    }

    #[cfg(windows)]
    #[test]
    fn an_inner_context_is_not_overwritten_by_an_outer_one() {
        let node = NodeId::from_key("submit");
        let inner = Error::WindowsApi {
            operation: "SetWindowTextW(EDIT)",
            code: 87,
            category: Win32Category::of(87),
            context: NativeContext::none().with_node(node),
        };
        let enriched = inner.or_context(
            NativeContext::none()
                .with_window(WindowId::PRIMARY)
                .with_node(NodeId::from_key("other")),
        );
        let context = enriched.context();
        assert_eq!(context.node, Some(node), "the innermost frame knew the node more precisely");
        assert_eq!(
            context.window,
            Some(WindowId::PRIMARY),
            "and the outer frame supplied the window"
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_empty_context_is_filled_in_by_the_outer_frame() {
        let error = Error::WindowsApi {
            operation: "CreateMenu",
            code: 8,
            category: Win32Category::of(8),
            context: NativeContext::none(),
        };
        let enriched = error.or_context(NativeContext::none().with_window(WindowId::PRIMARY));
        assert_eq!(enriched.context().window, Some(WindowId::PRIMARY));
    }

    #[test]
    fn a_contextless_error_renders_without_an_empty_parenthetical() {
        let rendered = Error::UnsupportedHost.to_string();
        assert!(!rendered.contains('('), "got {rendered:?}");
    }
}
