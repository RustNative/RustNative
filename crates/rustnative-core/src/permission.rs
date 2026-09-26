//! Permission states as hosts actually report them.
//!
//! A boolean capability cannot say what a mobile host returns: that the
//! person was never asked, that they granted access to *some* photos, that
//! they denied it once, or that they denied it and asked never to be asked
//! again. [`PermissionState`] can, and [`PermissionService`] is the portable
//! request flow over it. Each backend documents its mapping in
//! `docs/conformance/permissions.md`.

use crate::services::ServiceError;

/// A protected resource an application must be allowed to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Permission {
    /// The camera.
    Camera,
    /// The microphone.
    Microphone,
    /// The device's location.
    Location,
    /// Posting notifications.
    Notifications,
    /// The person's contacts.
    Contacts,
    /// The person's photos.
    Photos,
    /// Bluetooth devices.
    Bluetooth,
}

/// Where a permission stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionState {
    /// The person has not been asked; requesting will ask them.
    NotAsked,
    /// Allowed.
    Granted,
    /// Allowed in part — some photos, approximate location.
    Limited,
    /// Refused; asking again may be possible.
    Denied,
    /// Refused, and the host will not ask again: only the person, in the
    /// host's settings, can change it.
    PermanentlyDenied,
}

impl PermissionState {
    /// Whether the resource may be used now (fully or in part).
    #[must_use]
    pub const fn allows_use(self) -> bool {
        matches!(self, Self::Granted | Self::Limited)
    }

    /// Whether asking would reach the person.
    #[must_use]
    pub const fn can_request(self) -> bool {
        matches!(self, Self::NotAsked | Self::Denied)
    }
}

/// Querying and requesting permissions.
#[async_trait::async_trait]
pub trait PermissionService: Send + Sync {
    /// The current state of `permission`, without asking anyone.
    ///
    /// # Errors
    ///
    /// The host could not be queried.
    fn state(&self, permission: Permission) -> Result<PermissionState, ServiceError>;

    /// Asks the person for `permission` where the host lets an application
    /// ask, and returns the resulting state. On a host that never prompts
    /// (Windows, for a desktop application) this returns the current state,
    /// and [`Self::open_settings`] is the flow to offer instead.
    ///
    /// # Errors
    ///
    /// The host could not be queried.
    async fn request(&self, permission: Permission) -> Result<PermissionState, ServiceError>;

    /// Opens the host's settings page for `permission`, where the person
    /// can change it. Returns whether a page was opened.
    fn open_settings(&self, permission: Permission) -> bool;
}

/// A permission service with fixed answers, for tests and the headless
/// backend. `request` moves `NotAsked` to the configured answer.
#[derive(Debug, Default)]
pub struct FixedPermissions {
    states: parking_lot::Mutex<std::collections::HashMap<Permission, PermissionState>>,
    answers: std::collections::HashMap<Permission, PermissionState>,
}

impl FixedPermissions {
    /// Every permission `NotAsked`, and granted when requested.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts `permission` at `state`.
    #[must_use]
    pub fn with_state(self, permission: Permission, state: PermissionState) -> Self {
        self.states.lock().insert(permission, state);
        self
    }

    /// Answers a request for `permission` with `answer`.
    #[must_use]
    pub fn answering(mut self, permission: Permission, answer: PermissionState) -> Self {
        self.answers.insert(permission, answer);
        self
    }
}

#[async_trait::async_trait]
impl PermissionService for FixedPermissions {
    fn state(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        Ok(self.states.lock().get(&permission).copied().unwrap_or(PermissionState::NotAsked))
    }

    async fn request(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        let mut states = self.states.lock();
        let current = states.get(&permission).copied().unwrap_or(PermissionState::NotAsked);
        if !current.can_request() {
            return Ok(current);
        }
        let answer = self.answers.get(&permission).copied().unwrap_or(PermissionState::Granted);
        states.insert(permission, answer);
        Ok(answer)
    }

    fn open_settings(&self, _permission: Permission) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permanent_denial_is_not_requested_again() {
        let service = FixedPermissions::new()
            .with_state(Permission::Camera, PermissionState::PermanentlyDenied);
        let state = crate::scheduler::block_on_for_test(service.request(Permission::Camera));
        assert_eq!(state.ok(), Some(PermissionState::PermanentlyDenied));
        assert!(!PermissionState::PermanentlyDenied.can_request());
        assert!(PermissionState::Limited.allows_use());
    }

    #[test]
    fn requesting_moves_not_asked_to_the_answer() {
        let service =
            FixedPermissions::new().answering(Permission::Photos, PermissionState::Limited);
        assert_eq!(service.state(Permission::Photos).ok(), Some(PermissionState::NotAsked));
        let state = crate::scheduler::block_on_for_test(service.request(Permission::Photos));
        assert_eq!(state.ok(), Some(PermissionState::Limited));
    }
}
