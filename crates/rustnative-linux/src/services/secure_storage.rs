//! Secure storage on Linux (`PLAN.md` Milestone 57): secrets in the
//! desktop's keyring through the freedesktop Secret Service
//! (`org.freedesktop.secrets` — GNOME Keyring, KWallet, KeePassXC), the
//! store `libsecret` and every desktop's password manager use.
//!
//! Each secret is an item in the default collection (the "login" keyring),
//! found by the attributes `application` (the app id) and `name`. A locked
//! collection or item is unlocked through the service, which prompts the
//! person if it must.
//!
//! Traits, stated: `hardware_backed` is false — the keyring is encrypted
//! with the person's login password, not a TPM; `biometric_gating` is
//! false — no Secret Service gates a read on a fingerprint.
//!
//! The secret crosses the session bus in the `plain` algorithm: the session
//! bus is the person's own, and the transfer encryption the specification
//! offers protects only against a process already on it.
// ponytail: `plain` transfer, `dh-ietf1024-sha256-aes128-cbc-pkcs7` when a
// threat model counts other processes on the person's own session bus.

use std::collections::HashMap;
use std::time::Duration;

use gio::prelude::*;
use rustnative_core::ServiceError;
use rustnative_core::product::{SecureStorage, SecureStorageTraits};

const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE_INTERFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_INTERFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_INTERFACE: &str = "org.freedesktop.Secret.Item";
const PROMPT_INTERFACE: &str = "org.freedesktop.Secret.Prompt";
/// How long a call may take; a prompt waits for the person instead.
const CALL_TIMEOUT_MS: i32 = 10_000;
/// How long a prompt may wait for the person.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

type Secret = (glib::variant::ObjectPath, Vec<u8>, Vec<u8>, String);

/// Secrets for application `app` in the desktop's keyring.
#[derive(Debug, Clone)]
pub struct LinuxSecureStorage {
    app: String,
}

impl LinuxSecureStorage {
    /// The store for application `app` (its id).
    #[must_use]
    pub fn new(app: &str) -> Self {
        Self { app: app.to_owned() }
    }

    fn attributes(&self, name: &str) -> HashMap<String, String> {
        HashMap::from([
            ("application".to_owned(), self.app.clone()),
            ("name".to_owned(), name.to_owned()),
        ])
    }
}

fn error(what: &str, detail: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(format!("the Secret Service could not {what}: {detail}"))
}

fn path(text: &str) -> Result<glib::variant::ObjectPath, ServiceError> {
    glib::variant::ObjectPath::try_from(text.to_owned())
        .map_err(|_| error("answer", format!("`{text}` is not an object path")))
}

/// A connection with a session open on it: the session is the connection's.
struct Session {
    connection: gio::DBusConnection,
    session: glib::variant::ObjectPath,
}

impl Session {
    fn open() -> Result<Self, ServiceError> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .map_err(|e| error("be reached", e))?;
        let mut this = Self { connection, session: path("/")? };
        let reply = this.call(
            SERVICE_PATH,
            SERVICE_INTERFACE,
            "OpenSession",
            &("plain", "".to_variant()).to_variant(),
            "open a session",
        )?;
        this.session =
            reply.child_value(1).get().ok_or_else(|| error("open a session", "no session path"))?;
        Ok(this)
    }

    fn call(
        &self,
        object: &str,
        interface: &str,
        method: &str,
        arguments: &glib::Variant,
        what: &str,
    ) -> Result<glib::Variant, ServiceError> {
        self.connection
            .call_sync(
                Some(SERVICE),
                object,
                interface,
                method,
                Some(arguments),
                None,
                gio::DBusCallFlags::NONE,
                CALL_TIMEOUT_MS,
                gio::Cancellable::NONE,
            )
            .map_err(|e| error(what, e))
    }

    /// Completes `prompt` (`/` is none) and returns whether it was not
    /// dismissed.
    fn prompt(&self, prompt: &glib::variant::ObjectPath) -> Result<bool, ServiceError> {
        if prompt.as_str() == "/" {
            return Ok(true);
        }
        // The Completed signal is delivered to a context of our own, so a
        // prompt completes on any thread — the GTK thread included.
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let outcome = std::rc::Rc::new(std::cell::Cell::new(None));
                let seen = std::rc::Rc::clone(&outcome);
                let _subscription = self.connection.subscribe_to_signal(
                    Some(SERVICE),
                    Some(PROMPT_INTERFACE),
                    Some("Completed"),
                    Some(prompt.as_str()),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        seen.set(Some(
                            signal.parameters.child_value(0).get::<bool>() != Some(true),
                        ));
                    },
                );
                self.call(
                    prompt.as_str(),
                    PROMPT_INTERFACE,
                    "Prompt",
                    &("",).to_variant(),
                    "prompt",
                )?;
                let until = std::time::Instant::now() + PROMPT_TIMEOUT;
                while outcome.get().is_none() {
                    if std::time::Instant::now() > until {
                        let _ = self.call(
                            prompt.as_str(),
                            PROMPT_INTERFACE,
                            "Dismiss",
                            &().to_variant(),
                            "dismiss a prompt",
                        );
                        return Err(error("prompt", "the person did not answer"));
                    }
                    // Polled, so the deadline holds even if the service
                    // never answers.
                    if !context.iteration(false) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                Ok(outcome.get().unwrap_or(false))
            })
            .map_err(|e| error("prompt", e))?
    }

    /// Unlocks `objects`, prompting if the service must.
    fn unlock(&self, objects: Vec<glib::variant::ObjectPath>) -> Result<(), ServiceError> {
        let reply = self.call(
            SERVICE_PATH,
            SERVICE_INTERFACE,
            "Unlock",
            &(objects,).to_variant(),
            "unlock",
        )?;
        let prompt = reply.child_value(1).get().ok_or_else(|| error("unlock", "no prompt path"))?;
        if self.prompt(&prompt)? { Ok(()) } else { Err(error("unlock", "the person declined")) }
    }

    /// The default collection.
    fn collection(&self) -> Result<glib::variant::ObjectPath, ServiceError> {
        let reply = self.call(
            SERVICE_PATH,
            SERVICE_INTERFACE,
            "ReadAlias",
            &("default",).to_variant(),
            "find the default keyring",
        )?;
        let collection: glib::variant::ObjectPath = reply
            .child_value(0)
            .get()
            .ok_or_else(|| error("find the default keyring", "no path"))?;
        if collection.as_str() == "/" {
            return Err(error(
                "find the default keyring",
                "there is none (create one in the desktop's password manager)",
            ));
        }
        Ok(collection)
    }

    /// The items matching `attributes`, unlocked.
    fn search(
        &self,
        attributes: &HashMap<String, String>,
    ) -> Result<Vec<glib::variant::ObjectPath>, ServiceError> {
        let reply = self.call(
            SERVICE_PATH,
            SERVICE_INTERFACE,
            "SearchItems",
            &(attributes,).to_variant(),
            "search",
        )?;
        let mut unlocked: Vec<glib::variant::ObjectPath> =
            reply.child_value(0).get().unwrap_or_default();
        let locked: Vec<glib::variant::ObjectPath> = reply.child_value(1).get().unwrap_or_default();
        if !locked.is_empty() {
            self.unlock(locked.clone())?;
            unlocked.extend(locked);
        }
        Ok(unlocked)
    }

    fn secret(&self, value: &[u8]) -> Secret {
        (self.session.clone(), Vec::new(), value.to_vec(), "application/octet-stream".to_owned())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.session.as_str() != "/" {
            let _ = self.call(
                self.session.as_str(),
                "org.freedesktop.Secret.Session",
                "Close",
                &().to_variant(),
                "close the session",
            );
        }
    }
}

impl SecureStorage for LinuxSecureStorage {
    fn traits(&self) -> SecureStorageTraits {
        SecureStorageTraits { hardware_backed: false, biometric_gating: false }
    }

    fn put(&self, name: &str, secret: &[u8]) -> Result<(), ServiceError> {
        let session = Session::open()?;
        let collection = session.collection()?;
        let properties: HashMap<String, glib::Variant> = HashMap::from([
            (format!("{ITEM_INTERFACE}.Label"), format!("{} — {name}", self.app).to_variant()),
            (format!("{ITEM_INTERFACE}.Attributes"), self.attributes(name).to_variant()),
        ]);
        let arguments = (properties, session.secret(secret), true).to_variant();
        let create = || {
            session.call(
                collection.as_str(),
                COLLECTION_INTERFACE,
                "CreateItem",
                &arguments,
                "store the secret",
            )
        };
        // A locked keyring: unlock it, then store.
        let reply = create().or_else(|_| {
            session.unlock(vec![collection.clone()])?;
            create()
        })?;
        let prompt = reply
            .child_value(1)
            .get()
            .ok_or_else(|| error("store the secret", "no prompt path"))?;
        if session.prompt(&prompt)? {
            Ok(())
        } else {
            Err(error("store the secret", "the person declined"))
        }
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        let session = Session::open()?;
        let Some(item) = session.search(&self.attributes(name))?.into_iter().next() else {
            return Ok(None);
        };
        let reply = session.call(
            item.as_str(),
            ITEM_INTERFACE,
            "GetSecret",
            &(session.session.clone(),).to_variant(),
            "read the secret",
        )?;
        let (_, _, value, _) = reply
            .child_value(0)
            .get::<Secret>()
            .ok_or_else(|| error("read the secret", "an unexpected answer"))?;
        Ok(Some(value))
    }

    fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let session = Session::open()?;
        for item in session.search(&self.attributes(name))? {
            let reply = session.call(
                item.as_str(),
                ITEM_INTERFACE,
                "Delete",
                &().to_variant(),
                "delete the secret",
            )?;
            let prompt = reply
                .child_value(0)
                .get()
                .ok_or_else(|| error("delete the secret", "no prompt path"))?;
            if !session.prompt(&prompt)? {
                return Err(error("delete the secret", "the person declined"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real GNOME Keyring on the test's private bus, its login keyring
    /// created and unlocked with a throwaway password in a scratch home.
    fn keyring() -> Option<std::process::Child> {
        use std::io::Write as _;
        let home = std::env::temp_dir().join(format!("rustnative-keyring-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).ok()?;
        let mut daemon = std::process::Command::new("gnome-keyring-daemon")
            .args(["--foreground", "--unlock", "--components=secrets"])
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_RUNTIME_DIR", &home)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        daemon.stdin.take()?.write_all(b"test-only-password").ok()?;
        // Ready once the default keyring answers.
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < until {
            if Session::open().and_then(|session| session.collection()).is_ok() {
                return Some(daemon);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = daemon.kill();
        None
    }

    #[test]
    fn a_secret_round_trips_through_the_keyring_and_is_deleted() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipped: no session bus (run under tools/linux-session.sh)");
            return;
        }
        let Some(mut daemon) = keyring() else {
            eprintln!("skipped: gnome-keyring-daemon is not installed");
            return;
        };
        let store = LinuxSecureStorage::new("com.example.Tests");
        let other = LinuxSecureStorage::new("com.example.Other");
        assert_eq!(store.get("token").unwrap(), None);
        store.put("token", b"first").unwrap();
        store.put("token", b"second").unwrap();
        other.put("token", b"theirs").unwrap();
        assert_eq!(
            store.get("token").unwrap().as_deref(),
            Some(&b"second"[..]),
            "replaced, and per application"
        );
        store.delete("token").unwrap();
        assert_eq!(store.get("token").unwrap(), None);
        assert_eq!(other.get("token").unwrap().as_deref(), Some(&b"theirs"[..]));
        store.delete("never stored").unwrap();
        assert!(!store.traits().hardware_backed);
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
}
