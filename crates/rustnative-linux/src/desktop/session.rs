//! What kind of Linux session the application is running in: which display
//! server, and which desktop environment.
//!
//! Milestone 34 asks for both display servers "with their differences
//! reported rather than hidden", and for a desktop-environment conformance
//! matrix whose conventions are "answered per environment rather than
//! assumed". Both start here: [`Session`] is read from the process
//! environment (the variables every display manager and compositor sets),
//! and the toolkit then confirms the display server it actually opened —
//! `GDK_BACKEND` can say one thing and the toolkit fall back to the other.

use std::fmt;

/// The display server protocol a window is shown through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayServer {
    /// A Wayland compositor.
    Wayland,
    /// An X11 server (including Xwayland, which is an X server).
    X11,
}

impl fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Wayland => "Wayland",
            Self::X11 => "X11",
        })
    }
}

/// A desktop environment or compositor whose conventions differ enough to
/// be answered separately (`docs/linux/desktop-matrix.md`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesktopEnvironment {
    /// GNOME (GNOME Shell, and Ubuntu's GNOME session).
    Gnome,
    /// KDE Plasma.
    Kde,
    /// Xfce.
    Xfce,
    /// Cinnamon.
    Cinnamon,
    /// MATE.
    Mate,
    /// LXQt.
    Lxqt,
    /// Budgie.
    Budgie,
    /// Sway, or another wlroots compositor that reports itself as one.
    Sway,
    /// Hyprland.
    Hyprland,
    /// Windows Subsystem for Linux GUI: a Weston-based compositor whose
    /// windows are shown as Windows desktop windows, with Xwayland beside
    /// it and no desktop shell of its own.
    Wslg,
    /// A desktop this backend does not have a row for, by the name it gave.
    Other(String),
    /// Nothing said which desktop this is.
    Unknown,
}

impl DesktopEnvironment {
    /// Classifies an `XDG_CURRENT_DESKTOP`-style value — a colon-separated
    /// list, most specific first (`ubuntu:GNOME`) — case-insensitively.
    #[must_use]
    pub fn from_names(names: &str) -> Self {
        for name in names.split(':').map(str::trim).filter(|name| !name.is_empty()) {
            let known = match name.to_ascii_lowercase().as_str() {
                "gnome" | "gnome-classic" | "gnome-flashback" | "ubuntu" | "pop" => Self::Gnome,
                "kde" | "plasma" => Self::Kde,
                "xfce" | "xfce4" => Self::Xfce,
                "x-cinnamon" | "cinnamon" => Self::Cinnamon,
                "mate" => Self::Mate,
                "lxqt" => Self::Lxqt,
                "budgie" | "budgie:gnome" | "budgie-desktop" => Self::Budgie,
                "sway" | "wlroots" => Self::Sway,
                "hyprland" => Self::Hyprland,
                _ => continue,
            };
            return known;
        }
        names
            .split(':')
            .map(str::trim)
            .find(|name| !name.is_empty())
            .map_or(Self::Unknown, |name| Self::Other(name.to_owned()))
    }

    /// The environment's display name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Gnome => "GNOME",
            Self::Kde => "KDE Plasma",
            Self::Xfce => "Xfce",
            Self::Cinnamon => "Cinnamon",
            Self::Mate => "MATE",
            Self::Lxqt => "LXQt",
            Self::Budgie => "Budgie",
            Self::Sway => "Sway",
            Self::Hyprland => "Hyprland",
            Self::Wslg => "WSLg",
            Self::Other(name) => name,
            Self::Unknown => "unknown",
        }
    }
}

/// The session the application runs in, as the environment describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    display_server: Option<DisplayServer>,
    desktop: DesktopEnvironment,
    wsl: bool,
}

impl Session {
    /// Reads the session from this process's environment.
    #[must_use]
    pub fn detect() -> Self {
        let wsl_mount = std::path::Path::new("/mnt/wslg").is_dir();
        Self::from_environment(|name| std::env::var(name).ok(), wsl_mount)
    }

    /// Reads the session from `variable` (an environment lookup), and
    /// whether WSLg's shared mount is present — the testable core of
    /// [`Self::detect`].
    #[must_use]
    pub fn from_environment(variable: impl Fn(&str) -> Option<String>, wslg_mount: bool) -> Self {
        let set = |name: &str| variable(name).filter(|value| !value.is_empty());
        let backend = set("GDK_BACKEND").map(|value| value.to_ascii_lowercase());
        let wayland = set("WAYLAND_DISPLAY").is_some();
        let x11 = set("DISPLAY").is_some();
        // `GDK_BACKEND` is a request the toolkit honours when it can, and
        // may be a list (`wayland,x11`): its first entry that the session
        // offers wins, as it does in GDK.
        let requested = backend.as_deref().and_then(|list| {
            list.split(',').map(str::trim).find_map(|entry| match entry {
                "wayland" if wayland => Some(DisplayServer::Wayland),
                "x11" if x11 => Some(DisplayServer::X11),
                _ => None,
            })
        });
        let display_server = requested.or(if wayland {
            Some(DisplayServer::Wayland)
        } else if x11 {
            Some(DisplayServer::X11)
        } else {
            None
        });
        let wsl = set("WSL_DISTRO_NAME").is_some() || set("WSL_INTEROP").is_some();
        let desktop = set("XDG_CURRENT_DESKTOP")
            .or_else(|| set("XDG_SESSION_DESKTOP"))
            .or_else(|| set("DESKTOP_SESSION"))
            .map_or_else(
                // WSLg sets none of the desktop variables: its compositor
                // is not a desktop shell.
                || {
                    if wsl && wslg_mount {
                        DesktopEnvironment::Wslg
                    } else {
                        DesktopEnvironment::Unknown
                    }
                },
                |names| DesktopEnvironment::from_names(&names),
            );
        Self { display_server, desktop, wsl }
    }

    /// The same session, with the display server the toolkit actually
    /// opened — which wins over what the environment suggested.
    #[must_use]
    pub const fn with_display_server(mut self, display_server: DisplayServer) -> Self {
        self.display_server = Some(display_server);
        self
    }

    /// The display server, if one is reachable.
    #[must_use]
    pub const fn display_server(&self) -> Option<DisplayServer> {
        self.display_server
    }

    /// The desktop environment.
    #[must_use]
    pub const fn desktop(&self) -> &DesktopEnvironment {
        &self.desktop
    }

    /// Whether this is Windows Subsystem for Linux.
    #[must_use]
    pub const fn is_wsl(&self) -> bool {
        self.wsl
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn session(pairs: &[(&str, &str)], wslg: bool) -> Session {
        let map: HashMap<String, String> =
            pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        Session::from_environment(|name| map.get(name).cloned(), wslg)
    }

    #[test]
    fn wayland_is_preferred_when_both_are_offered() {
        let both = session(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")], false);
        assert_eq!(both.display_server(), Some(DisplayServer::Wayland));
    }

    #[test]
    fn gdk_backend_picks_the_first_entry_the_session_offers() {
        let forced = session(
            &[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0"), ("GDK_BACKEND", "x11")],
            false,
        );
        assert_eq!(forced.display_server(), Some(DisplayServer::X11));
        let fallback = session(&[("DISPLAY", ":0"), ("GDK_BACKEND", "wayland,x11")], false);
        assert_eq!(fallback.display_server(), Some(DisplayServer::X11));
        assert_eq!(session(&[], false).display_server(), None);
    }

    #[test]
    fn desktops_are_classified_most_specific_first() {
        assert_eq!(DesktopEnvironment::from_names("ubuntu:GNOME"), DesktopEnvironment::Gnome);
        assert_eq!(DesktopEnvironment::from_names("KDE"), DesktopEnvironment::Kde);
        assert_eq!(DesktopEnvironment::from_names("X-Cinnamon"), DesktopEnvironment::Cinnamon);
        assert_eq!(DesktopEnvironment::from_names("sway"), DesktopEnvironment::Sway);
        assert_eq!(
            DesktopEnvironment::from_names("Enlightenment"),
            DesktopEnvironment::Other("Enlightenment".to_owned())
        );
        assert_eq!(DesktopEnvironment::from_names(""), DesktopEnvironment::Unknown);
    }

    #[test]
    fn wslg_is_recognized_by_its_distribution_and_mount() {
        let wslg =
            session(&[("WSL_DISTRO_NAME", "Ubuntu"), ("WAYLAND_DISPLAY", "wayland-0")], true);
        assert_eq!(wslg.desktop(), &DesktopEnvironment::Wslg);
        assert!(wslg.is_wsl());
        // A real desktop inside WSL says so, and wins.
        let gnome =
            session(&[("WSL_DISTRO_NAME", "Ubuntu"), ("XDG_CURRENT_DESKTOP", "GNOME")], true);
        assert_eq!(gnome.desktop(), &DesktopEnvironment::Gnome);
    }
}
