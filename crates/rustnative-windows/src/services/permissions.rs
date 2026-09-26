//! `PermissionService` on Windows.
//!
//! Windows gates three resources for desktop applications — camera,
//! microphone, and location — through the privacy settings' consent store,
//! and never prompts an unpackaged application: the person decides in
//! Settings. So `request` reports the current state and `open_settings`
//! opens the right Settings page. Every other permission is not gated for a
//! desktop application at all, and is reported as granted.
//!
//! | Permission | Source | Mapping |
//! |---|---|---|
//! | Camera, Microphone, Location | `HKCU\…\CapabilityAccessManager\ConsentStore\<resource>` `Value`, and its `NonPackaged` subkey | either `Deny` → `PermanentlyDenied` (only Settings can change it); otherwise `Granted` |
//! | Notifications, Contacts, Photos, Bluetooth | none | `Granted` |

use rustnative_core::{Permission, PermissionService, PermissionState, ServiceError};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::native::util::wide;

/// Windows' permission service. Stateless: every answer is read from the
/// consent store when asked, so a change in Settings is seen immediately.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsPermissions;

impl WindowsPermissions {
    /// The service.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

fn consent_key(permission: Permission) -> Option<&'static str> {
    match permission {
        Permission::Camera => Some("webcam"),
        Permission::Microphone => Some("microphone"),
        Permission::Location => Some("location"),
        _ => None,
    }
}

fn settings_page(permission: Permission) -> Option<&'static str> {
    match permission {
        Permission::Camera => Some("ms-settings:privacy-webcam"),
        Permission::Microphone => Some("ms-settings:privacy-microphone"),
        Permission::Location => Some("ms-settings:privacy-location"),
        Permission::Notifications => Some("ms-settings:notifications"),
        Permission::Contacts => Some("ms-settings:privacy-contacts"),
        Permission::Photos => Some("ms-settings:privacy-pictures"),
        Permission::Bluetooth => Some("ms-settings:bluetooth"),
        _ => None,
    }
}

fn read_string(subkey: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide("Value");
    let mut buffer = [0_u16; 64];
    let mut size = u32::try_from(std::mem::size_of_val(&buffer)).unwrap_or(128);
    // SAFETY: both names are NUL-terminated; `buffer`/`size` describe a
    // writable buffer of that many bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

fn consent_state(resource: &str) -> PermissionState {
    let base = format!(
        r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{resource}"
    );
    let global = read_string(&base);
    let desktop = read_string(&format!(r"{base}\NonPackaged"));
    let denied = [global, desktop].iter().flatten().any(|value| value.eq_ignore_ascii_case("Deny"));
    if denied { PermissionState::PermanentlyDenied } else { PermissionState::Granted }
}

#[async_trait::async_trait]
impl PermissionService for WindowsPermissions {
    fn state(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        Ok(consent_key(permission).map_or(PermissionState::Granted, consent_state))
    }

    async fn request(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        // Windows does not prompt a desktop application; the answer is
        // whatever the person set in Settings.
        self.state(permission)
    }

    fn open_settings(&self, permission: Permission) -> bool {
        let Some(page) = settings_page(permission) else { return false };
        let verb = wide("open");
        let target = wide(page);
        // SAFETY: both strings are NUL-terminated and outlive the call; a
        // null window and directory are documented as allowed.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // `ShellExecuteW` reports success as a value greater than 32.
        result as isize > 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_has_an_answer_on_this_machine() {
        let service = WindowsPermissions::new();
        for permission in [
            Permission::Camera,
            Permission::Microphone,
            Permission::Location,
            Permission::Notifications,
            Permission::Contacts,
            Permission::Photos,
            Permission::Bluetooth,
        ] {
            let state = service.state(permission).expect("the consent store is readable");
            assert!(
                matches!(state, PermissionState::Granted | PermissionState::PermanentlyDenied),
                "{permission:?}: {state:?}"
            );
        }
    }
}
