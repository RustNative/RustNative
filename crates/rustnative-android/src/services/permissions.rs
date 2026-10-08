//! Runtime permissions in the five portable states.
//!
//! | Portable | Android |
//! |---|---|
//! | `Camera` | `CAMERA` |
//! | `Microphone` | `RECORD_AUDIO` |
//! | `Location` | `ACCESS_FINE_LOCATION`; `ACCESS_COARSE_LOCATION` alone is `Limited` |
//! | `Notifications` | `POST_NOTIFICATIONS` (API 33+); below, whether notifications are enabled |
//! | `Contacts` | `READ_CONTACTS` |
//! | `Photos` | `READ_MEDIA_IMAGES`; `READ_MEDIA_VISUAL_USER_SELECTED` alone (API 34+) is `Limited`; `READ_EXTERNAL_STORAGE` below API 33 |
//! | `Bluetooth` | `BLUETOOTH_CONNECT` (API 31+); install-time below |
//!
//! Android does not say whether a refused permission may be asked again
//! before asking: `shouldShowRequestPermissionRationale` is false both for
//! "never asked" and for "don't ask again". The backend keeps a record of
//! what this application asked, which settles it: not asked → `NotAsked`;
//! asked, refused, rationale shown → `Denied`; asked, refused, no
//! rationale → `PermanentlyDenied`. The permission must be declared in the
//! manifest (`[android] permissions` in `rustnative.toml`); an undeclared
//! one is refused without a prompt.

use rustnative_core::{Permission, PermissionService, PermissionState, ServiceError};

use super::{abandon, java, pending};
use crate::jni_host::{Arg, Class, Ret};

/// The runtime permission flow.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidPermissions;

/// `RnServices.permissionsFor`'s numbering; `None` for a permission this
/// backend does not map yet.
const fn code(permission: Permission) -> Option<i32> {
    Some(match permission {
        Permission::Camera => 0,
        Permission::Microphone => 1,
        Permission::Location => 2,
        Permission::Notifications => 3,
        Permission::Contacts => 4,
        Permission::Photos => 5,
        Permission::Bluetooth => 6,
        _ => return None,
    })
}

fn mapped(permission: Permission) -> Result<i32, ServiceError> {
    code(permission).ok_or_else(|| {
        ServiceError::new(format!("{permission:?} has no Android permission mapped"))
    })
}

/// `RnServices`' state numbers.
const fn state(code: i32) -> PermissionState {
    match code {
        1 => PermissionState::Granted,
        2 => PermissionState::Limited,
        3 => PermissionState::Denied,
        4 => PermissionState::PermanentlyDenied,
        _ => PermissionState::NotAsked,
    }
}

#[async_trait::async_trait]
impl PermissionService for AndroidPermissions {
    fn state(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        let ret =
            java(Class::Services, "permissionState", "(I)I", &[Arg::Int(mapped(permission)?)])?;
        Ok(state(if let Ret::Int(value) = ret { value } else { 0 }))
    }

    async fn request(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        let current = self.state(permission)?;
        if matches!(current, PermissionState::Granted | PermissionState::PermanentlyDenied) {
            return Ok(current);
        }
        let (token, answer) = pending();
        let asked = java(
            Class::Services,
            "requestPermission",
            "(II)Z",
            &[Arg::Int(token), Arg::Int(mapped(permission)?)],
        )?
        .bool();
        if !asked {
            abandon(token);
            return Ok(current);
        }
        let _ = answer.await;
        self.state(permission)
    }

    fn open_settings(&self, _permission: Permission) -> bool {
        java(Class::Services, "openSettings", "()Z", &[]).is_ok_and(Ret::bool)
    }
}
