//! Host colors for semantic role tokens (`PLAN.md` Milestone 48,
//! `docs/tokens.md`).
//!
//! A design system's tokens are of two kinds. *Brand* values are absolute —
//! the brand's blue is the brand's blue on every host. *Semantic roles* —
//! the accent, the surface, the text on it — may instead follow the host:
//! on Windows the accent is the one the person chose in Settings. A theme
//! records which of its tokens follow which [`HostRole`]
//! ([`crate::Theme::with_host_role`]), and the backend supplies the host's
//! [`HostPalette`], re-reading it when the person changes their settings.

use crate::style::Color;

/// A color the host defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostRole {
    /// The person's accent color.
    Accent,
    /// Text on the accent color.
    OnAccent,
    /// A window's background.
    Surface,
    /// Text on a window's background.
    OnSurface,
    /// Selected content's background.
    Highlight,
    /// Selected content's text.
    OnHighlight,
    /// A control's border.
    Border,
    /// Secondary, de-emphasized text.
    Muted,
}

impl HostRole {
    /// The role named `name` in a token file (`accent`, `on-surface`, …).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "accent" => Self::Accent,
            "on-accent" => Self::OnAccent,
            "surface" => Self::Surface,
            "on-surface" => Self::OnSurface,
            "highlight" => Self::Highlight,
            "on-highlight" => Self::OnHighlight,
            "border" => Self::Border,
            "muted" => Self::Muted,
            _ => return None,
        })
    }

    /// Its name in a token file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Accent => "accent",
            Self::OnAccent => "on-accent",
            Self::Surface => "surface",
            Self::OnSurface => "on-surface",
            Self::Highlight => "highlight",
            Self::OnHighlight => "on-highlight",
            Self::Border => "border",
            Self::Muted => "muted",
        }
    }
}

/// The host's colors for every [`HostRole`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPalette {
    /// [`HostRole::Accent`].
    pub accent: Color,
    /// [`HostRole::OnAccent`].
    pub on_accent: Color,
    /// [`HostRole::Surface`].
    pub surface: Color,
    /// [`HostRole::OnSurface`].
    pub on_surface: Color,
    /// [`HostRole::Highlight`].
    pub highlight: Color,
    /// [`HostRole::OnHighlight`].
    pub on_highlight: Color,
    /// [`HostRole::Border`].
    pub border: Color,
    /// [`HostRole::Muted`].
    pub muted: Color,
}

impl Default for HostPalette {
    /// A fixed reference palette (Windows' light defaults), the same on
    /// every machine — what the headless backend and tests use.
    fn default() -> Self {
        Self {
            accent: Color::rgb(0x00, 0x78, 0xd4),
            on_accent: Color::rgb(0xff, 0xff, 0xff),
            surface: Color::rgb(0xff, 0xff, 0xff),
            on_surface: Color::rgb(0x00, 0x00, 0x00),
            highlight: Color::rgb(0x00, 0x78, 0xd7),
            on_highlight: Color::rgb(0xff, 0xff, 0xff),
            border: Color::rgb(0xa0, 0xa0, 0xa0),
            muted: Color::rgb(0x6d, 0x6d, 0x6d),
        }
    }
}

impl HostPalette {
    /// The host's color for `role`.
    #[must_use]
    pub const fn color(&self, role: HostRole) -> Color {
        match role {
            HostRole::Accent => self.accent,
            HostRole::OnAccent => self.on_accent,
            HostRole::Surface => self.surface,
            HostRole::OnSurface => self.on_surface,
            HostRole::Highlight => self.highlight,
            HostRole::OnHighlight => self.on_highlight,
            HostRole::Border => self.border,
            HostRole::Muted => self.muted,
        }
    }
}
