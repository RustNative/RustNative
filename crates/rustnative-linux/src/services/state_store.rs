//! Persisted component state on disk, under the XDG state directory:
//! `$XDG_STATE_HOME/<app-id>/state/` (by default `~/.local/state/…`), in the
//! crash-safe format of `rustnative_core::services::FileStateStore`.

use std::path::{Path, PathBuf};

use rustnative_core::{ServiceError, StateStore};

/// A [`StateStore`] that keeps each key in its own crash-safe file.
///
/// ```no_run
/// use std::sync::Arc;
///
/// use rustnative_core::Services;
/// use rustnative_linux::FileStateStore;
///
/// let store = FileStateStore::for_app("com.example.Notes")?;
/// let services = Services::default().with_state_store(Arc::new(store));
/// # Ok::<(), rustnative_core::ServiceError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileStateStore(rustnative_core::services::FileStateStore);

impl FileStateStore {
    /// The store for application `app_id`, in `$XDG_STATE_HOME/<app-id>/state`
    /// (`~/.local/state/<app-id>/state` when the variable is unset).
    ///
    /// # Errors
    ///
    /// `app_id` is not a usable directory name, no home directory is known,
    /// or the directory could not be created.
    pub fn for_app(app_id: &str) -> Result<Self, ServiceError> {
        let valid = !app_id.is_empty()
            && app_id != "."
            && app_id != ".."
            && !app_id.chars().any(|character| character.is_control() || character == '/');
        if !valid {
            return Err(ServiceError::new(format!("`{app_id}` is not a usable application id")));
        }
        let base = state_home(|name| std::env::var_os(name))
            .ok_or_else(|| ServiceError::new("neither XDG_STATE_HOME nor HOME is set"))?;
        Self::in_directory(base.join(app_id).join("state"))
    }

    /// A store in `directory`, created if it does not exist.
    ///
    /// # Errors
    ///
    /// The directory could not be created or listed.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Result<Self, ServiceError> {
        rustnative_core::services::FileStateStore::in_directory(directory).map(Self)
    }

    /// The directory this store writes to.
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.0.directory()
    }
}

/// `$XDG_STATE_HOME`, or `$HOME/.local/state` per the XDG Base Directory
/// specification (a relative `XDG_STATE_HOME` is invalid and ignored).
fn state_home(variable: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    variable("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| variable("HOME").map(|home| PathBuf::from(home).join(".local").join("state")))
}

impl StateStore for FileStateStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        self.0.load(key)
    }

    fn save(&self, key: &str, value: &[u8]) -> Result<(), ServiceError> {
        self.0.save(key, value)
    }

    fn remove(&self, key: &str) -> Result<(), ServiceError> {
        self.0.remove(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_home_follows_the_xdg_specification() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).into())
            }
        };
        assert_eq!(
            state_home(env(&[("XDG_STATE_HOME", "/s"), ("HOME", "/h")])),
            Some(PathBuf::from("/s"))
        );
        assert_eq!(
            state_home(env(&[("XDG_STATE_HOME", "relative"), ("HOME", "/h")])),
            Some(PathBuf::from("/h/.local/state"))
        );
        assert_eq!(state_home(env(&[])), None);
        assert!(FileStateStore::for_app("a/b").is_err());
        assert!(FileStateStore::for_app("..").is_err());
    }
}
