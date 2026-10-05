# Linux backend on Fedora and Arch Linux (Milestone 34, distribution coverage)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan phase-by-phase. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Status: complete (2026-10-05).** What was verified, found, and owed is recorded in `BUILD_STATUS.md`. Deviations from the plan as written: Fedora and Arch were moved to `E:\WSL\` after a build filled the host's C: drive; Rust was seeded from Ubuntu's toolchains rather than downloaded (a slow link); `noto-fonts` was dropped on Arch (DejaVu suffices); Kali's backend run was not repeated, since no backend code changed; and the AppStream metadata was fixed when `appstreamcli` rejected it.

**Goal:** The Linux backend (`rustnative-linux`) and its tooling are verified on the
three big distribution families, not only the Debian one: Fedora (the RPM
family — RHEL, CentOS Stream, openSUSE share the format) and Arch Linux (the
pacman family — Manjaro, EndeavourOS). Milestone 34 was verified on Ubuntu
24.04 (GTK 4.14) and Kali Rolling (GTK 4.22); both are Debian.

**Spec:** `PLAN.md` Milestone 34; `docs/superpowers/plans/2026-09-29-linux-milestone-34.md`
("Verification gate", Phase 9); `docs/linux.md`; `docs/linux/desktop-matrix.md`.

## Scope decision (2026-10-04)

The user's instruction: *Fedora and Arch Linux are now installed in WSL
(`FedoraLinux-44`, `archlinux`); make sure the backend covers them too, for
the most coverage; plan it thoroughly, do it properly, don't stop until done.*
Therefore:

- **Verification:** the backend's tests pass under Wayland (WSLg), X11
  (Xwayland), and Xvfb on both new distributions, with nothing skipped that
  runs on Ubuntu (a skip prints `skipped:`; the run is read with
  `--nocapture` to prove none fired). The whole Linux gate (fmt, clippy, the
  portable tests, doc, MSRV, deny) runs on each.
- **Fixes:** whatever the new distributions expose — in the backend, in
  `tools/linux-session.sh`, or in a test's assumption — is fixed in the
  code, not worked around in the environment.
- **Tooling, because "covered" includes shipping:** a person on Fedora or
  Arch must be told the right install command and be able to package for
  their own distribution:
  - `rustnative doctor` names the missing packages in the host's own
    package manager (apt, dnf, pacman, zypper), from `/etc/os-release`;
  - `rustnative package linux` gains **`rpm`** (Fedora/RHEL/openSUSE: `dnf
    install ./app.rpm`) and **`pacman`** (Arch: `pacman -U app.pkg.tar`),
    written in Rust like the `.deb` — reproducible, no `rpmbuild` or
    `makepkg` needed — and verified by the real `rpm` and `pacman` reading
    and installing them into a throwaway root;
  - `docs/linux.md` gives the three families' setup.
- **CI:** the `linux-backend` job gains Fedora and Arch container runs on
  Xvfb, so the coverage does not decay.
- **Out of scope:** openSUSE, NixOS, Alpine (musl), and other families are
  not installed; the doctor's zypper row and the RPM format serve openSUSE
  untested, and that is recorded as such. A physical GNOME or Plasma session
  stays owed as in Milestone 34.

## Environment

| Distribution | WSL name | User for tests | Package manager |
|---|---|---|---|
| Ubuntu 24.04 | `Ubuntu` | `omarshehab` | apt |
| Kali Rolling | `kali-linux` | default | apt |
| Fedora 44 | `FedoraLinux-44` | `omarshehab` (packages installed as root) | dnf |
| Arch Linux | `archlinux` | `root` (the image's only user) | pacman |

Each builds in its own `~/.cache/rustnative-target`, never the Windows target.

## Phases

### Phase 0 — provision

- [x] Fedora: `gcc gcc-c++ make pkgconf-pkg-config git gtk4-devel libsoup3-devel at-spi2-core dbus-daemon dbus-tools weston xorg-x11-server-Xvfb xdotool nmap-ncat python3-gobject gnome-keyring libsecret desktop-file-utils appstream mesa-dri-drivers`, the DejaVu and Noto fonts, `glibc-langpack-{en,de,fr,tr}`.
- [x] Arch: `base-devel git gtk4 libsoup3 at-spi2-core dbus weston xorg-server-xvfb xdotool openbsd-netcat python-gobject gnome-keyring libsecret desktop-file-utils appstream mesa ttf-dejavu noto-fonts`; `en_US en_NZ de_DE fr_FR tr_TR` UTF-8 in `/etc/locale.gen`.
- [x] rustup with stable and 1.85, clippy, rustfmt; `cargo-deny`.

### Phase 1 — the backend's tests on both

- [x] `tools/linux-session.sh {wayland,x11,xvfb} cargo test -p rustnative-linux -p adoption-gtk` on each; triage every failure to a cause; fix in code.
- [x] Re-run with `-- --nocapture` and confirm no `skipped:` line fires (except XTest outside `xvfb`, by design).
- [x] Re-run Ubuntu after any fix, so a fix for one distribution does not break another.

### Phase 2 — distribution-aware tooling

- [x] `rustnative::distro` (new module): `/etc/os-release` parsed into `id` and `id_like`; a `Family` (`Debian`, `Fedora`, `Arch`, `Suse`, `Unknown`); each family's install command for the build packages (GTK 4, libsoup 3, a C toolchain, pkg-config).
- [x] `doctor`'s Linux row names the missing package in the host's package manager.
- [x] Unit tests over recorded `os-release` files of Ubuntu, Kali, Fedora, Arch, openSUSE, Manjaro, Linux Mint.

### Phase 3 — RPM and pacman packages

- [x] `package::linux::gzip_stored` — a gzip stream of stored deflate blocks (no compressor's output to depend on; reproducible).
- [x] `package::linux::rpm` — an RPM v4 package: the lead, a signature header (size, payload size, SHA-256 of the header), the main header (name, version, release, summary, description, build time = `SOURCE_DATE_EPOCH`, file list as dirnames/basenames, modes, sizes, SHA-256 digests, owners, `Requires: gtk4 >= 4.14, libsoup3`, the `rpmlib()` features used, payload format and digest), both with their immutable region, and a gzip'd `newc` cpio payload.
- [x] `package::linux::pacman` — a `.pkg.tar`: `.PKGINFO`, a gzip'd `.MTREE` with each file's SHA-256, and the installed files under `usr/`.
- [x] `Format::Rpm` and `Format::Pacman`; `all` adds both; the CLI's usage message.
- [x] Unit tests on structure and reproducibility (on every host); an integration test that, where `rpm` / `pacman` exist, reads the package's metadata and file list and installs it into a throwaway root (`rpm --root --nodeps -i`, `pacman --root --dbpath -U --nodeps`).

### Phase 4 — the gate on all three, and CI

- [x] `rustnative-tools/gate-linux.sh` runs unchanged in each distribution; run in full on Ubuntu, Fedora, and Arch, with the gate's logs in `gate-linux-<id>/`.
- [x] `.github/workflows/ci.yml`: `linux-backend` becomes a matrix over `ubuntu`, `fedora:latest`, `archlinux:latest` containers.

### Phase 5 — documents

- [x] `docs/linux.md`: toolkit row, the three families' setup, `rpm` / `pacman` packaging.
- [x] `docs/linux/desktop-matrix.md`: the verified column names the four distributions and their GTK versions.
- [x] `BUILD_STATUS.md`, `PLAN.md` Milestone 34: what was verified, what was found and fixed, what stays owed.
