//! The component library (`PLAN.md` Milestone 48): what applications are
//! built from, above the native primitives.
//!
//! - [`behaviour`]: the headless behaviour layer — focus, keyboard, and
//!   selection for lists, tabs, menus, trees, grids, comboboxes, and date
//!   entry, independent of appearance (`C19`);
//! - [`widgets`]: the composite components, each a component usable as a
//!   builder call and as a markup element, bound to its parent through
//!   stores and commands;
//! - [`chart`]: line, area, bar, scatter, and pie charts on the draw-list
//!   path, each with an accessible data table;
//! - [`idioms`]: the per-host idiom table (`C23`);
//! - the role tokens every style is written against, and
//!   [`with_roles`] to supply them.
//!
//! The guide is `docs/components.md`; the tokens are `docs/tokens.md`.

pub mod behaviour;
pub mod chart;
pub mod idioms;
pub mod widgets;

pub use chart::{Chart, ChartKind, ChartProps, Series};
pub use idioms::{IDIOMS, Idioms};
pub use widgets::*;

/// The library's role tokens with their default values
/// (`components.css`), as a theme.
#[must_use]
pub fn role_theme() -> rustnative_core::Theme {
    include!(concat!(env!("OUT_DIR"), "/app_theme.rs"))
}

/// The names of the role tokens the library's styles use.
pub const ROLES: [&str; 9] = [
    "color-accent",
    "color-on-accent",
    "color-danger",
    "color-on-danger",
    "color-surface",
    "color-on-surface",
    "color-muted",
    "color-border",
    "color-subtle",
];

/// `theme` with every role token it does not define added at its default
/// value — so an application's own token set wins where it says something,
/// and the library still resolves everywhere else. The default accent,
/// surface, text, border, and muted roles follow the host's colors
/// (`docs/tokens.md`); danger is a brand value.
#[must_use]
pub fn with_roles(theme: rustnative_core::Theme) -> rustnative_core::Theme {
    let defaults = role_theme();
    let mut tokens = theme.tokens().clone();
    let mut added = Vec::new();
    for role in ROLES {
        if tokens.get(role).is_none() {
            if let Some(value) = defaults.tokens().get(role) {
                tokens.insert(role, value.clone());
                added.push(role);
            }
        }
    }
    // A default role that follows the host keeps following it.
    let mut theme = theme.with_tokens(tokens);
    for (name, host) in defaults.host_roles() {
        if added.contains(&name.as_ref()) {
            theme = theme.with_host_role(name.clone(), *host);
        }
    }
    theme
}
