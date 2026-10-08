//! The error type this backend's `Platform::Error` resolves to, and the
//! native context it carries — the Android counterpart of
//! `rustnative_windows::Error` and `rustnative_linux::Error`, with the same
//! shape: which operation failed, on which window and node, and why.

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

/// A failure of the Android backend.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// This build is not running on Android, so there is no activity to
    /// realize anything in — what `AndroidPlatform::run` answers on every
    /// other OS.
    UnsupportedHost,
    /// `AndroidPlatform::run` was called outside the entry
    /// (`rustnative_android::export_main!`): there is no activity to attach
    /// the application to.
    NoActivity,
    /// A Java method threw, or the JNI call itself failed.
    Java {
        /// The operation, named by the Java method or JNI function
        /// (`RnViews.create`, `GetStaticMethodID`).
        operation: String,
        /// The exception's class and message, or the JNI failure.
        detail: String,
        /// Which window and node it was realizing.
        context: NativeContext,
    },
    /// A foreign node's view could not be made.
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
    /// A component panicked inside a callback from Java. The panic was
    /// caught at the JNI boundary (unwinding into the JVM is undefined
    /// behaviour), the application's panic policy was applied, and the
    /// application ended.
    ComponentPanicked {
        /// The panic's message, when it was a string.
        message: String,
    },
}

impl Error {
    /// A Java failure of `operation`.
    pub(crate) fn java(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Java {
            operation: operation.into(),
            detail: detail.into(),
            context: NativeContext::none(),
        }
    }

    /// Fills in context the failing site could not know, without
    /// overwriting context it already carries.
    #[must_use]
    pub fn or_context(self, context: NativeContext) -> Self {
        match self {
            Self::Java { operation, detail, context: existing } => Self::Java {
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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedHost => f.write_str("the Android backend runs only on Android"),
            Self::NoActivity => f.write_str(
                "there is no activity to run in: on Android, `run` is called from the application's \
                 `main`, which `rustnative_android::export_main!` exports for the host to start",
            ),
            Self::Java { operation, detail, context } => {
                write!(f, "{operation} failed")?;
                if !context.is_empty() {
                    write!(f, " ({context})")?;
                }
                if !detail.is_empty() {
                    write!(f, ": {detail}")?;
                }
                Ok(())
            }
            Self::ForeignUnavailable { kind, reason, context } => {
                write!(f, "the foreign view `{kind}` could not be made: {reason}")?;
                if !context.is_empty() {
                    write!(f, " ({context})")?;
                }
                Ok(())
            }
            Self::DuplicateNodeId { node } => {
                write!(f, "two nodes in one window have the identity {node:?}")
            }
            Self::ComponentPanicked { message } => write!(f, "a component panicked: {message}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_fills_in_without_overwriting() {
        let error = Error::java("RnViews.create", "boom")
            .or_context(NativeContext::none().with_window(WindowId::PRIMARY));
        let Error::Java { context, .. } = &error else { panic!("a Java error") };
        assert_eq!(context.window, Some(WindowId::PRIMARY));
        assert!(error.to_string().starts_with("RnViews.create failed (window"));
        assert!(Error::NoActivity.to_string().contains("export_main!"));
    }
}
