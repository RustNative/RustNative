//! Secure storage on Windows (`PLAN.md` Milestone 57): secrets in the
//! Credential Manager, encrypted for the signed-in user by DPAPI before
//! they are stored, so another account (or the same bytes copied to
//! another machine) cannot read them.
//!
//! Traits, stated: `hardware_backed` is false — the Credential Manager is
//! protected by the user's logon secret, not a TPM (a TPM-bound key would
//! be a Windows Hello key credential, a different API); `biometric_gating`
//! is false for this store — Windows Hello consent is not attached to a
//! credential read.

use rustnative_core::ServiceError;
use rustnative_core::product::{SecureStorage, SecureStorageTraits};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData,
};

use crate::native::util::wide;

/// Secrets for application `app` in the Credential Manager.
#[derive(Debug, Clone)]
pub struct WindowsSecureStorage {
    app: String,
}

impl WindowsSecureStorage {
    /// The store for application `app` (its id).
    #[must_use]
    pub fn new(app: &str) -> Self {
        Self { app: app.to_owned() }
    }

    fn target(&self, name: &str) -> Vec<u16> {
        wide(format!("{}/{name}", self.app))
    }
}

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).unwrap_or(0),
        pbData: bytes.as_ptr().cast_mut(),
    }
}

fn last(what: &str) -> ServiceError {
    ServiceError::new(format!("{what}: {}", std::io::Error::last_os_error()))
}

/// Encrypts `plain` for the current user (DPAPI).
fn protect(plain: &[u8]) -> Result<Vec<u8>, ServiceError> {
    let input = blob(plain);
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: valid input and output blobs; no entropy, prompt, or flags.
    let ok = unsafe {
        CryptProtectData(
            &raw const input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &raw mut output,
        )
    };
    if ok == 0 {
        return Err(last("CryptProtectData"));
    }
    // SAFETY: DPAPI filled `output`; it is copied, then freed once.
    let sealed = unsafe {
        std::slice::from_raw_parts(output.pbData, usize::try_from(output.cbData).unwrap_or(0))
    }
    .to_vec();
    // SAFETY: the buffer DPAPI allocated.
    unsafe { LocalFree(output.pbData.cast()) };
    Ok(sealed)
}

/// Decrypts what [`protect`] sealed.
fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, ServiceError> {
    let input = blob(sealed);
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: as above.
    let ok = unsafe {
        CryptUnprotectData(
            &raw const input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &raw mut output,
        )
    };
    if ok == 0 {
        return Err(last("CryptUnprotectData"));
    }
    // SAFETY: as above.
    let plain = unsafe {
        std::slice::from_raw_parts(output.pbData, usize::try_from(output.cbData).unwrap_or(0))
    }
    .to_vec();
    // SAFETY: as above.
    unsafe { LocalFree(output.pbData.cast()) };
    Ok(plain)
}

impl SecureStorage for WindowsSecureStorage {
    fn traits(&self) -> SecureStorageTraits {
        SecureStorageTraits { hardware_backed: false, biometric_gating: false }
    }

    fn put(&self, name: &str, secret: &[u8]) -> Result<(), ServiceError> {
        let sealed = protect(secret)?;
        let target = self.target(name);
        // SAFETY: zeroed is a valid CREDENTIALW before its fields are set.
        let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target.as_ptr().cast_mut();
        credential.CredentialBlobSize = u32::try_from(sealed.len()).unwrap_or(0);
        credential.CredentialBlob = sealed.as_ptr().cast_mut();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        // SAFETY: the credential's strings and blob outlive the call.
        if unsafe { CredWriteW(&raw const credential, 0) } == 0 {
            return Err(last("CredWriteW"));
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        let target = self.target(name);
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: a NUL-terminated target and an out-pointer.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut credential) } == 0 {
            // ERROR_NOT_FOUND: nothing stored.
            if std::io::Error::last_os_error().raw_os_error() == Some(1168) {
                return Ok(None);
            }
            return Err(last("CredReadW"));
        }
        // SAFETY: CredReadW returned a valid credential, freed below.
        let sealed = unsafe {
            let credential = &*credential;
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                usize::try_from(credential.CredentialBlobSize).unwrap_or(0),
            )
            .to_vec()
        };
        // SAFETY: the buffer CredReadW allocated.
        unsafe { CredFree(credential.cast()) };
        unprotect(&sealed).map(Some)
    }

    fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let target = self.target(name);
        // SAFETY: a NUL-terminated target.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
            && std::io::Error::last_os_error().raw_os_error() != Some(1168)
        {
            return Err(last("CredDeleteW"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips_sealed_for_this_user() {
        let store = WindowsSecureStorage::new(&format!(
            "dev.rustnative.secure-test-{}",
            std::process::id()
        ));
        store.put("token", b"s3cret").unwrap();
        assert_eq!(store.get("token").unwrap().as_deref(), Some(&b"s3cret"[..]));

        // What the Credential Manager holds is DPAPI's sealed form, not the
        // secret.
        let target = store.target("token");
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: a NUL-terminated target and an out-pointer.
        let read = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut credential) };
        assert_ne!(read, 0);
        // SAFETY: CredReadW returned a valid credential, freed below.
        let raw = unsafe {
            let credential = &*credential;
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                usize::try_from(credential.CredentialBlobSize).unwrap(),
            )
            .to_vec()
        };
        // SAFETY: the buffer CredReadW allocated.
        unsafe { CredFree(credential.cast()) };
        assert!(!raw.windows(6).any(|window| window == b"s3cret"), "stored sealed");

        store.delete("token").unwrap();
        assert_eq!(store.get("token").unwrap(), None);
        store.delete("token").unwrap();
        assert!(!store.traits().hardware_backed, "stated, not claimed");
    }
}
