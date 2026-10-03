//! The error type this backend's `Platform::Error` resolves to, and the
//! native context it carries — the Linux counterpart of
//! `rustnative_windows::Error`, with the same shape: which operation failed,
//! on which window and node, and why, with the raw detail kept for `Debug`
//! rather than rendered by `Display`.

use std::fmt;

use rustnative_core::{NodeId, WindowId};

/// Where a native failure happened: the window and node it concerns, when
/// they are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeContext {
    /// The window the failing operation was realizing.
    pub window: Option<WindowId>,
    /// The node the failing operation was realizing.
    pub node: Option<NodeId>,
}

impl NativeContext {
    /// No window or node known.
    #[must_use]
    pub const fn none() -> Self {
        Self { window: None, node: None }
    }

    /// The same context, naming `window`.
    #[must_use]
    pub const fn with_window(mut self, window: WindowId) -> Self {
        self.window = Some(window);
        self
    }

    /// The same context, naming `node`.
    #[must_use]
    pub const fn with_node(mut self, node: NodeId) -> Self {
        self.node = Some(node);
        self
    }

    const fn is_empty(&self) -> bool {
        self.window.is_none() && self.node.is_none()
    }
}

impl fmt::Display for NativeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.window, self.node) {
            (Some(window), Some(node)) => write!(f, "window {}, node {node:?}", window.get()),
            (Some(window), None) => write!(f, "window {}", window.get()),
            (None, Some(node)) => write!(f, "node {node:?}"),
            (None, None) => Ok(()),
        }
    }
}

/// A failure of the Linux backend.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// This build is not running on Linux, so there is no toolkit to run
    /// on — what `LinuxPlatform::run` answers on every other OS, mirroring
    /// `rustnative_windows::Error::UnsupportedHost` on Linux.
    UnsupportedHost,
    /// The toolkit could not be initialized: no display server was
    /// reachable (neither `WAYLAND_DISPLAY` nor `DISPLAY` answered), or the
    /// toolkit refused the one it found.
    NoDisplay {
        /// What the toolkit reported.
        detail: String,
    },
    /// A toolkit operation failed.
    Toolkit {
        /// The operation, named the way the toolkit names it
        /// (`gtk_window_new`, `GtkCssProvider`, …).
        operation: &'static str,
        /// What the toolkit reported, if anything.
        detail: String,
        /// Which window and node it was realizing.
        context: NativeContext,
    },
    /// A foreign node's widget could not be made.
    ForeignUnavailable {
        /// The node's foreign kind.
        kind: String,
        /// Why.
        reason: &'static str,
        /// Which node.
        context: NativeContext,
    },
    /// Two nodes in one window's tree have the same identity.
    DuplicateNodeId {
        /// The duplicated identity.
        node: NodeId,
    },
    /// A component panicked inside a toolkit callback. The panic was caught
    /// at the callback boundary (unwinding into C is undefined behaviour),
    /// the application's panic policy was applied, and the loop ended.
    ComponentPanicked {
        /// The panic's message, when it was a string.
        message: String,
    },
}

impl Error {
    /// Fills in context the failing site could not know (the window, when
    /// a helper that does not know its window failed), without overwriting
    /// context it already carries.
    #[must_use]
    pub fn or_context(self, context: NativeContext) -> Self {
        match self {
            Self::Toolkit { operation, detail, context: existing } => Self::Toolkit {
                operation,
                detail,
                context: NativeContext {
                    window: existing.window.or(context.window),
                    node: existing.node.or(context.node),
                },
            },
            other => other,
        }
    }

    /// The window and node this failure concerns, when known.
    #[must_use]
    pub const fn context(&self) -> NativeContext {
        match self {
            Self::Toolkit { context, .. } | Self::ForeignUnavailable { context, .. } => *context,
            Self::DuplicateNodeId { node } => NativeContext::none().with_node(*node),
            Self::UnsupportedHost | Self::NoDisplay { .. } | Self::ComponentPanicked { .. } => {
                NativeContext::none()
            }
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedHost => {
                f.write_str("the Linux backend runs only on Linux; this build is for another OS")
            }
            Self::NoDisplay { detail } => write!(
                f,
                "no display server is reachable (neither Wayland nor X11 answered): {detail}"
            ),
            Self::Toolkit { operation, detail, context } => {
                write!(f, "`{operation}` failed")?;
                if !detail.is_empty() {
                    write!(f, ": {detail}")?;
                }
                if !context.is_empty() {
                    write!(f, " ({context})")?;
                }
                Ok(())
            }
            Self::ForeignUnavailable { kind, reason, context } => {
                write!(f, "no widget for foreign kind `{kind}`: {reason}")?;
                if !context.is_empty() {
                    write!(f, " ({context})")?;
                }
                Ok(())
            }
            Self::DuplicateNodeId { node } => {
                write!(f, "two nodes in one window share the identity {node:?}")
            }
            Self::ComponentPanicked { message } => {
                write!(f, "a component panicked inside a toolkit callback: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_is_filled_in_without_overwriting() {
        let error = Error::Toolkit {
            operation: "gtk_label_new",
            detail: String::new(),
            context: NativeContext::none().with_node(NodeId::from_key("a")),
        }
        .or_context(
            NativeContext::none().with_window(WindowId::PRIMARY).with_node(NodeId::from_key("b")),
        );
        assert_eq!(error.context().window, Some(WindowId::PRIMARY));
        assert_eq!(error.context().node, Some(NodeId::from_key("a")));
        assert!(error.to_string().contains("gtk_label_new"));
    }

    #[test]
    fn an_unsupported_host_says_which_os_it_needs() {
        assert!(Error::UnsupportedHost.to_string().contains("only on Linux"));
    }
}
