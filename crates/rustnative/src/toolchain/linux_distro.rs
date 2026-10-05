//! Which Linux distribution this is, read from `os-release(5)` — so that
//! what `rustnative` tells a person to install, and which package it builds
//! for their own machine, is in their distribution's terms.
//!
//! Distributions are grouped by family, the unit their packages and package
//! names are shared across: Debian (Ubuntu, Kali, Mint, Pop!_OS), Fedora
//! (RHEL, CentOS Stream, Rocky, Alma), Arch (Manjaro, EndeavourOS), and SUSE
//! (openSUSE, SLES). A derivative is recognised by its `ID_LIKE`, so a
//! distribution this file has never heard of still lands in its family.

/// A family of distributions sharing a package format and package names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Debian and its derivatives: `apt`, `.deb`.
    Debian,
    /// Fedora, RHEL, and their derivatives: `dnf`, `.rpm`.
    Fedora,
    /// Arch Linux and its derivatives: `pacman`, `.pkg.tar`.
    Arch,
    /// openSUSE and SLES: `zypper`, `.rpm`.
    Suse,
}

/// A library the Linux backend builds against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Library {
    /// GTK 4's development files.
    Gtk,
    /// libsoup 3's development files.
    Soup,
}

impl Family {
    /// Classifies one `ID` or `ID_LIKE` word.
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "debian" | "ubuntu" | "kali" | "linuxmint" | "pop" | "raspbian" | "elementary"
            | "zorin" | "neon" => Self::Debian,
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara" => Self::Fedora,
            "arch" | "archarm" | "manjaro" | "endeavouros" | "garuda" | "cachyos" => Self::Arch,
            "suse" | "opensuse" | "opensuse-tumbleweed" | "opensuse-leap" | "sles" | "sled" => {
                Self::Suse
            }
            _ => return None,
        })
    }

    /// The family's package for `library`.
    #[must_use]
    pub const fn package(self, library: Library) -> &'static str {
        match (self, library) {
            (Self::Debian, Library::Gtk) => "libgtk-4-dev",
            (Self::Debian, Library::Soup) => "libsoup-3.0-dev",
            (Self::Fedora | Self::Suse, Library::Gtk) => "gtk4-devel",
            (Self::Fedora, Library::Soup) => "libsoup3-devel",
            (Self::Arch, Library::Gtk) => "gtk4",
            (Self::Arch, Library::Soup) => "libsoup3",
            (Self::Suse, Library::Soup) => "libsoup-devel",
        }
    }

    /// The command that installs `packages`.
    #[must_use]
    pub fn install(self, packages: &[&str]) -> String {
        let manager = match self {
            Self::Debian => "sudo apt install",
            Self::Fedora => "sudo dnf install",
            Self::Arch => "sudo pacman -S --needed",
            Self::Suse => "sudo zypper install",
        };
        format!("{manager} {}", packages.join(" "))
    }

    /// Everything the Linux backend needs to build: a C toolchain,
    /// `pkg-config`, GTK 4, and libsoup 3.
    #[must_use]
    pub fn build_packages(self) -> Vec<&'static str> {
        let toolchain: &[&str] = match self {
            Self::Debian => &["build-essential", "pkg-config"],
            Self::Fedora => &["gcc", "pkgconf-pkg-config"],
            Self::Arch => &["base-devel"],
            Self::Suse => &["gcc", "pkgconf"],
        };
        let mut packages = toolchain.to_vec();
        packages.extend([self.package(Library::Gtk), self.package(Library::Soup)]);
        packages
    }

    /// The `rustnative package linux --format` this family installs
    /// natively.
    #[must_use]
    pub const fn native_format(self) -> &'static str {
        match self {
            Self::Debian => "deb",
            Self::Fedora | Self::Suse => "rpm",
            Self::Arch => "pacman",
        }
    }
}

/// The running distribution, as its `os-release` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    /// `PRETTY_NAME` (or `NAME`, or `ID`): what to call it in a message.
    pub name: String,
    /// `ID`, lower case (`fedora`, `arch`, `ubuntu`).
    pub id: String,
    /// The family, from `ID` and then each `ID_LIKE` word; `None` for a
    /// distribution outside the four families.
    pub family: Option<Family>,
}

impl Distribution {
    /// Reads an `os-release` file's contents: `KEY=value` lines, the value
    /// optionally quoted, `#` comments.
    #[must_use]
    pub fn parse(contents: &str) -> Self {
        let field = |key: &str| {
            contents.lines().find_map(|line| {
                let value = line.trim().strip_prefix(key)?.strip_prefix('=')?;
                Some(value.trim().trim_matches(|c| c == '"' || c == '\'').to_owned())
            })
        };
        let id = field("ID").unwrap_or_else(|| "linux".to_owned()).to_ascii_lowercase();
        let like = field("ID_LIKE").unwrap_or_default().to_ascii_lowercase();
        let family =
            std::iter::once(id.as_str()).chain(like.split_whitespace()).find_map(Family::from_id);
        let name = field("PRETTY_NAME").or_else(|| field("NAME")).unwrap_or_else(|| id.clone());
        Self { name, id, family }
    }

    /// The running system's distribution: `/etc/os-release`, or
    /// `/usr/lib/os-release` where `/etc` has none; `None` off Linux or
    /// with neither file.
    #[must_use]
    pub fn detect() -> Option<Self> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        ["/etc/os-release", "/usr/lib/os-release"]
            .iter()
            .find_map(|path| std::fs::read_to_string(path).ok())
            .map(|contents| Self::parse(&contents))
    }

    /// What to run to get everything the Linux backend builds against, in
    /// this distribution's terms, or a generic description outside the
    /// known families.
    #[must_use]
    pub fn setup_hint(&self) -> String {
        match self.family {
            Some(family) => family.install(&family.build_packages()),
            None => {
                "install a C compiler, pkg-config, and the development files of GTK 4 and libsoup 3"
                    .to_owned()
            }
        }
    }

    /// What to run to get `library`'s package, in this distribution's
    /// terms, or a generic description outside the known families.
    #[must_use]
    pub fn install_hint(&self, library: Library) -> String {
        match self.family {
            Some(family) => family.install(&[family.package(library)]),
            None => match library {
                Library::Gtk => "install GTK 4's development files".to_owned(),
                Library::Soup => "install libsoup 3's development files".to_owned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UBUNTU: &str = "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nID=ubuntu\nID_LIKE=debian\n";
    const KALI: &str = "PRETTY_NAME=\"Kali GNU/Linux Rolling\"\nNAME=\"Kali GNU/Linux\"\nID=kali\nID_LIKE=debian\n";
    const MINT: &str = "NAME=\"Linux Mint\"\nID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
    const FEDORA: &str = "NAME=\"Fedora Linux\"\nVERSION=\"44 (WSL)\"\nRELEASE_TYPE=stable\nID=fedora\nVERSION_ID=44\nPRETTY_NAME=\"Fedora Linux 44 (WSL)\"\n";
    const ROCKY: &str = "NAME=\"Rocky Linux\"\nID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n";
    const ARCH: &str =
        "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";
    const MANJARO: &str = "NAME=\"Manjaro Linux\"\nID=manjaro\nID_LIKE=arch\n";
    const TUMBLEWEED: &str = "NAME=\"openSUSE Tumbleweed\"\n# a comment\nID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"\n";
    const ALPINE: &str = "NAME=\"Alpine Linux\"\nID=alpine\n";

    #[test]
    fn each_distribution_lands_in_its_family_by_id_or_id_like() {
        for (contents, family) in [
            (UBUNTU, Some(Family::Debian)),
            (KALI, Some(Family::Debian)),
            (MINT, Some(Family::Debian)),
            (FEDORA, Some(Family::Fedora)),
            (ROCKY, Some(Family::Fedora)),
            (ARCH, Some(Family::Arch)),
            (MANJARO, Some(Family::Arch)),
            (TUMBLEWEED, Some(Family::Suse)),
            (ALPINE, None),
        ] {
            assert_eq!(Distribution::parse(contents).family, family, "{contents}");
        }
    }

    #[test]
    fn the_name_is_the_pretty_one_and_quotes_are_removed() {
        let fedora = Distribution::parse(FEDORA);
        assert_eq!(fedora.name, "Fedora Linux 44 (WSL)");
        assert_eq!(fedora.id, "fedora");
        assert_eq!(Distribution::parse(MINT).name, "Linux Mint");
        assert_eq!(Distribution::parse(ROCKY).id, "rocky");
        // `ID_LIKE=` is not read as `ID=`.
        assert_eq!(Distribution::parse("ID_LIKE=debian\nID=foo\n").id, "foo");
        assert_eq!(Distribution::parse("").id, "linux");
    }

    #[test]
    fn hints_are_in_the_distributions_own_package_manager() {
        assert_eq!(
            Distribution::parse(UBUNTU).install_hint(Library::Gtk),
            "sudo apt install libgtk-4-dev"
        );
        assert_eq!(
            Distribution::parse(FEDORA).install_hint(Library::Soup),
            "sudo dnf install libsoup3-devel"
        );
        assert_eq!(
            Distribution::parse(ARCH).install_hint(Library::Gtk),
            "sudo pacman -S --needed gtk4"
        );
        assert_eq!(
            Distribution::parse(TUMBLEWEED).install_hint(Library::Gtk),
            "sudo zypper install gtk4-devel"
        );
        assert!(Distribution::parse(ALPINE).install_hint(Library::Soup).contains("libsoup 3"));
        assert_eq!(
            Distribution::parse(FEDORA).setup_hint(),
            "sudo dnf install gcc pkgconf-pkg-config gtk4-devel libsoup3-devel"
        );
        assert_eq!(
            Distribution::parse(ARCH).setup_hint(),
            "sudo pacman -S --needed base-devel gtk4 libsoup3"
        );
        assert_eq!(
            Distribution::parse(KALI).setup_hint(),
            "sudo apt install build-essential pkg-config libgtk-4-dev libsoup-3.0-dev"
        );
        assert!(Distribution::parse(ALPINE).setup_hint().contains("GTK 4"));
    }

    #[test]
    fn each_family_packages_natively() {
        assert_eq!(Family::Debian.native_format(), "deb");
        assert_eq!(Family::Fedora.native_format(), "rpm");
        assert_eq!(Family::Suse.native_format(), "rpm");
        assert_eq!(Family::Arch.native_format(), "pacman");
    }

    #[test]
    fn the_running_system_is_read_on_linux_only() {
        if cfg!(target_os = "linux") {
            // Every system this runs on has an os-release (systemd's and
            // every family's convention), WSL included.
            assert!(Distribution::detect().is_some());
        } else {
            assert_eq!(Distribution::detect(), None);
        }
    }
}
