//! Secrets on the local machine: where a process keeps the files only its
//! own user may read, and how a presented token is compared.
//!
//! The inspection server publishes its token in a file, and the
//! development agent writes the executables it is sent before running
//! them. Neither belongs in the shared temporary directory, where another
//! local user can read a file left at its default mode, pre-create the
//! directory, or plant a symbolic link at the name about to be written.
//! [`user_directory`] names a per-user location instead,
//! [`private_directory`] creates it readable by its owner alone (and
//! refuses one that another user owns or can reach), and
//! [`create_private_file`] creates a file in it that did not exist before,
//! without following a link.
//!
//! | Platform | Base directory |
//! |---|---|
//! | Linux and other Unix | `$XDG_RUNTIME_DIR`, else `$XDG_CACHE_HOME`, else `$HOME/.cache` |
//! | macOS | `$TMPDIR` (the per-user `/var/folders/…` directory) |
//! | Windows | `%LOCALAPPDATA%` |
//!
//! Where none of these is set, the shared temporary directory is used with
//! the user's id in the name, and [`private_directory`]'s checks are what
//! keep another user out.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use subtle::ConstantTimeEq as _;

/// Whether `left` and `right` are equal, compared without leaking where
/// the first difference is. The length is not secret: slices of different
/// lengths compare unequal at once.
///
/// ```
/// use rustnative_core::secret::constant_time_eq;
///
/// assert!(constant_time_eq(b"token", b"token"));
/// assert!(!constant_time_eq(b"token", b"tokem"));
/// assert!(!constant_time_eq(b"token", b"tok"));
/// ```
#[must_use]
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.ct_eq(right).into()
}

/// The per-user directory `name` lives in (see the module's table). It is
/// only a location: [`private_directory`] creates and checks it.
#[must_use]
pub fn user_directory(name: &str) -> PathBuf {
    match user_base() {
        Some(base) => base.join(name),
        None => shared_fallback(name),
    }
}

/// An absolute directory from the environment variable `variable`; a
/// relative one is ignored, as the XDG specification requires.
#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
fn variable_directory(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable).map(PathBuf::from).filter(|path| path.is_absolute())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn user_base() -> Option<PathBuf> {
    variable_directory("XDG_RUNTIME_DIR")
        .or_else(|| variable_directory("XDG_CACHE_HOME"))
        .or_else(|| variable_directory("HOME").map(|home| home.join(".cache")))
}

#[cfg(target_os = "macos")]
fn user_base() -> Option<PathBuf> {
    variable_directory("TMPDIR")
}

#[cfg(windows)]
fn user_base() -> Option<PathBuf> {
    variable_directory("LOCALAPPDATA")
}

#[cfg(not(any(unix, windows)))]
fn user_base() -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn shared_fallback(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{name}-{}", effective_user_id()))
}

#[cfg(not(unix))]
fn shared_fallback(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

#[cfg(unix)]
fn effective_user_id() -> u32 {
    // SAFETY: `geteuid` takes no arguments, touches no memory of ours, and
    // cannot fail.
    unsafe { libc::geteuid() }
}

/// Creates `path` (and any missing parents) readable by its owner alone —
/// mode `0700` on Unix — or, when it exists, checks it is still that:
/// see [`check_private_directory`].
///
/// # Errors
///
/// The directory cannot be created, or the one there is not private.
pub fn private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path)?;
    check_private_directory(path)
}

/// Checks `path` is a directory itself — not a symbolic link to one — and,
/// on Unix, that it belongs to this process's user and that no other user
/// has any access to it. A reader calls this before trusting what it finds
/// there.
///
/// # Errors
///
/// `path` cannot be read (`NotFound` when it does not exist), or it fails
/// a check (`PermissionDenied`, naming which).
pub fn check_private_directory(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let refuse = |why: String| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} {why}; remove it and try again", path.display()),
        ))
    };
    if !metadata.file_type().is_dir() {
        return refuse("is not a directory (or is a symbolic link)".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let user = effective_user_id();
        if metadata.uid() != user {
            return refuse(format!("belongs to user {}, not {user}", metadata.uid()));
        }
        let mode = metadata.mode() & 0o777;
        if mode & 0o077 != 0 {
            return refuse(format!("is open to other users (mode {mode:03o}, not 700)"));
        }
    }
    Ok(())
}

/// Creates `path` for writing, readable and writable by its owner alone
/// (mode `0600` on Unix). It must not exist: nothing already there — a
/// file, or a symbolic link planted at the name — is opened or followed.
///
/// # Errors
///
/// Something is already at `path` (`AlreadyExists`), or it cannot be
/// created.
pub fn create_private_file(path: &Path) -> io::Result<File> {
    create_new(path, 0o600)
}

/// [`create_private_file`] for an executable: mode `0700` on Unix, so its
/// owner can run it and nobody else can read, write, or run it.
///
/// # Errors
///
/// As [`create_private_file`].
pub fn create_private_executable(path: &Path) -> io::Result<File> {
    create_new(path, 0o700)
}

#[cfg(unix)]
fn create_new(path: &Path, mode: u32) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    // `create_new` is `O_CREAT | O_EXCL`, which already refuses a link at
    // the final component; `O_NOFOLLOW` says so explicitly.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(not(unix))]
fn create_new(path: &Path, _mode: u32) -> io::Result<File> {
    // `CREATE_NEW` on Windows: fails on anything already at the name,
    // links included. The file inherits the per-user directory's ACL.
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("rustnative-secret-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn equal_and_unequal_tokens() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"0123abcd", b"0123abcd"));
        assert!(!constant_time_eq(b"0123abcd", b"0123abce"));
        assert!(!constant_time_eq(b"0123abcd", b"0123abc"));
    }

    #[test]
    fn a_private_file_is_new_and_never_followed() {
        let directory = scratch("file");
        private_directory(&directory).unwrap();
        let path = directory.join("endpoint.json");
        drop(create_private_file(&path).unwrap());
        assert_eq!(
            create_private_file(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists,
            "an existing file is not reused"
        );
        #[cfg(unix)]
        {
            let target = directory.join("target");
            let link = directory.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(create_private_file(&link).is_err(), "a planted link is not followed");
            assert!(!target.exists());
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn modes_are_owner_only_and_an_open_directory_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

        let directory = scratch("modes");
        private_directory(&directory).unwrap();
        assert_eq!(mode(&directory), 0o700);
        let file = directory.join("file");
        drop(create_private_file(&file).unwrap());
        assert_eq!(mode(&file), 0o600);
        let executable = directory.join("executable");
        drop(create_private_executable(&executable).unwrap());
        assert_eq!(mode(&executable), 0o700);

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        let refused = private_directory(&directory).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied, "{refused}");

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let link = scratch("modes-link");
        std::os::unix::fs::symlink(&directory, &link).unwrap();
        assert!(private_directory(&link).is_err(), "a link to a directory is not one");

        std::fs::remove_file(&link).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
