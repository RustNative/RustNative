//! Secrets sealed with an AES-GCM key held by the Android Keystore — in a
//! `StrongBox` secure element where the device has one, else the TEE — and
//! kept in the application's private files. The key never leaves the
//! Keystore; a copied file is useless on another device or after the
//! application is reinstalled.

use rustnative_core::ServiceError;
use rustnative_core::product::{SecureStorage, SecureStorageTraits};

use super::{checked, java};
use crate::jni_host::{Arg, Class, Ret};

/// The Keystore-backed store.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidSecureStorage;

impl SecureStorage for AndroidSecureStorage {
    fn traits(&self) -> SecureStorageTraits {
        SecureStorageTraits {
            hardware_backed: java(Class::Services, "secureHardware", "()Z", &[])
                .is_ok_and(Ret::bool),
            // A user-authentication-bound key is a separate key; this store's
            // is not bound, so reading never prompts.
            biometric_gating: false,
        }
    }

    fn put(&self, name: &str, secret: &[u8]) -> Result<(), ServiceError> {
        checked(java(
            Class::Services,
            "sealSecret",
            "(Ljava/lang/String;[B)Ljava/lang/String;",
            &[Arg::Str(name), Arg::Bytes(secret)],
        )?)
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        match java(Class::Services, "openSecret", "(Ljava/lang/String;)[B", &[Arg::Str(name)])? {
            Ret::Bytes(bytes) => Ok(bytes),
            _ => Ok(None),
        }
    }

    fn delete(&self, name: &str) -> Result<(), ServiceError> {
        if java(Class::Services, "deleteSecret", "(Ljava/lang/String;)Z", &[Arg::Str(name)])?.bool()
        {
            Ok(())
        } else {
            Err(ServiceError::new(format!("could not delete the secret `{name}`")))
        }
    }
}
