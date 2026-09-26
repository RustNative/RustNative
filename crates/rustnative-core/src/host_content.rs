//! Host content controls (`PLAN.md` Milestone 48, `C28`): embedded web
//! content, media playback, and camera preview, as capability-guarded
//! nodes.
//!
//! Each is a [`Node::foreign`] whose kind names the content
//! ([`HostContent::kind`]), so it is laid out, clipped, and destroyed by
//! the same rules as any foreign object, and a backend realizes it with the
//! host's own control. Whether a backend can is a [`Capability`]:
//! [`host_content`] builds the node where the capability is present and
//! the application's fallback where it is not, so a missing player is a
//! stated alternative rather than an empty rectangle.
//!
//! ```
//! use rustnative_core::{
//!     Capability, HostContent, LayoutStyle, Node, PlatformCapabilities, host_content,
//! };
//!
//! let video = HostContent::Media { source: "intro.mp4".into() };
//! let without = PlatformCapabilities::new([]);
//! let node = host_content("intro", &video, LayoutStyle::new(), &without, || {
//!     Node::label("intro-fallback", "Video playback is not available here")
//! });
//! assert_eq!(node.foreign_kind(), None, "the fallback, not an empty player");
//!
//! let with = PlatformCapabilities::new([Capability::MediaPlayback]);
//! let node = host_content("intro", &video, LayoutStyle::new(), &with, || unreachable!());
//! assert_eq!(node.foreign_kind(), Some("rustnative.media:intro.mp4"));
//!
//! // The same player in markup:
//! let markup = rustnative_core::rsx! {
//!     <Foreign key="intro" kind={video.kind()} />
//! };
//! assert_eq!(markup, node);
//! ```
//!
//! Adding another kind of host content follows the same pattern: a variant
//! here with its capability, and a backend factory for its kind (on
//! Windows, `rustnative_windows::register_foreign` for an application's own).

use crate::capability::{Capability, PlatformCapabilities};
use crate::layout::LayoutStyle;
use crate::node::Node;

/// Content a host control shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostContent {
    /// A web page.
    Web {
        /// Its address.
        url: String,
    },
    /// Audio or video, with the host's playback controls.
    Media {
        /// A file path or URL the host's player opens.
        source: String,
    },
    /// The live preview of a camera.
    Camera {
        /// Which camera, by the host's index (0 is the default).
        device: u32,
    },
}

const WEB: &str = "rustnative.web:";
const MEDIA: &str = "rustnative.media:";
const CAMERA: &str = "rustnative.camera:";

impl HostContent {
    /// The capability a backend must have to show it.
    #[must_use]
    pub const fn capability(&self) -> Capability {
        match self {
            Self::Web { .. } => Capability::WebContent,
            Self::Media { .. } => Capability::MediaPlayback,
            Self::Camera { .. } => Capability::Camera,
        }
    }

    /// The foreign kind that carries it.
    #[must_use]
    pub fn kind(&self) -> String {
        match self {
            Self::Web { url } => format!("{WEB}{url}"),
            Self::Media { source } => format!("{MEDIA}{source}"),
            Self::Camera { device } => format!("{CAMERA}{device}"),
        }
    }

    /// The content a foreign kind carries, if it is host content.
    #[must_use]
    pub fn from_kind(kind: &str) -> Option<Self> {
        if let Some(url) = kind.strip_prefix(WEB) {
            Some(Self::Web { url: url.to_owned() })
        } else if let Some(source) = kind.strip_prefix(MEDIA) {
            Some(Self::Media { source: source.to_owned() })
        } else {
            kind.strip_prefix(CAMERA)?.parse().ok().map(|device| Self::Camera { device })
        }
    }
}

/// `content` as a node where `capabilities` can show it, and `fallback()`
/// where they cannot (see [`HostContent`]).
pub fn host_content(
    key: impl AsRef<str>,
    content: &HostContent,
    layout: LayoutStyle,
    capabilities: &PlatformCapabilities,
    fallback: impl FnOnce() -> Node,
) -> Node {
    if capabilities.supports(content.capability()) {
        Node::foreign(key, content.kind(), layout)
    } else {
        fallback()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_round_trips() {
        for content in [
            HostContent::Web { url: "https://example.com/a:b".into() },
            HostContent::Media { source: "C:\\clips\\intro.mp4".into() },
            HostContent::Camera { device: 2 },
        ] {
            assert_eq!(HostContent::from_kind(&content.kind()), Some(content));
        }
        assert_eq!(HostContent::from_kind("month-calendar"), None);
    }
}
