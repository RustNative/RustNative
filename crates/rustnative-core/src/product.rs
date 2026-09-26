//! Product services (`PLAN.md` Milestone 57): the contracts every backend
//! answers — secure storage, feature flags, remote push, and commerce —
//! with what the host cannot do stated rather than faked.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::services::ServiceError;

// --- Secure storage -------------------------------------------------------

/// What a secure store can promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SecureStorageTraits {
    /// Keys are held by hardware (a TPM, a secure enclave).
    pub hardware_backed: bool,
    /// Reading can require biometric confirmation.
    pub biometric_gating: bool,
}

/// Secrets — tokens, keys — in the host's protected store.
pub trait SecureStorage: Send + Sync {
    /// What this store can promise.
    fn traits(&self) -> SecureStorageTraits;

    /// Stores `secret` under `name`, replacing any previous one.
    ///
    /// # Errors
    ///
    /// The store refused.
    fn put(&self, name: &str, secret: &[u8]) -> Result<(), ServiceError>;

    /// The secret stored under `name`.
    ///
    /// # Errors
    ///
    /// The store refused (a missing secret is `Ok(None)`).
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError>;

    /// Removes `name`.
    ///
    /// # Errors
    ///
    /// The store refused.
    fn delete(&self, name: &str) -> Result<(), ServiceError>;
}

/// A secure store in memory, for tests.
#[derive(Debug, Clone, Default)]
pub struct MemorySecureStorage(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);

impl SecureStorage for MemorySecureStorage {
    fn traits(&self) -> SecureStorageTraits {
        SecureStorageTraits::default()
    }
    fn put(&self, name: &str, secret: &[u8]) -> Result<(), ServiceError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_owned(), secret.to_vec());
        Ok(())
    }
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, ServiceError> {
        Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner).get(name).cloned())
    }
    fn delete(&self, name: &str) -> Result<(), ServiceError> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(name);
        Ok(())
    }
}

// --- Feature flags --------------------------------------------------------

/// A typed flag with its compiled default.
///
/// ```
/// use rustnative_core::product::{Flag, Flags};
///
/// const NEW_EDITOR: Flag<bool> = Flag::new("new-editor", false);
/// let flags = Flags::new();
/// assert!(!flags.get(&NEW_EDITOR), "the compiled default, offline");
/// flags.apply_remote(&serde_json::json!({ "new-editor": true }));
/// assert!(flags.get(&NEW_EDITOR));
/// flags.set_override("new-editor", serde_json::json!(false));
/// assert!(!flags.get(&NEW_EDITOR), "a local override wins, for development and tests");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Flag<T: 'static> {
    key: &'static str,
    default: T,
}

impl<T: 'static> Flag<T> {
    /// A flag named `key`, `default` until configured otherwise.
    pub const fn new(key: &'static str, default: T) -> Self {
        Self { key, default }
    }

    /// Its name.
    pub const fn key(&self) -> &'static str {
        self.key
    }
}

/// The flags' values: remote configuration over compiled defaults, local
/// overrides over both. Readers see a changed value at their next read;
/// `version` changes whenever a value may have.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    remote: Arc<Mutex<BTreeMap<String, Value>>>,
    overrides: Arc<Mutex<BTreeMap<String, Value>>>,
    version: Arc<std::sync::atomic::AtomicU64>,
}

impl Flags {
    /// No configuration yet: every flag is its default.
    #[must_use]
    pub fn new() -> Self {
        let flags = Self::default();
        // `RUSTNATIVE_FLAGS=key=value,key=value` overrides locally.
        if let Ok(text) = std::env::var("RUSTNATIVE_FLAGS") {
            for pair in text.split(',') {
                if let Some((key, value)) = pair.split_once('=') {
                    let value = serde_json::from_str(value)
                        .unwrap_or_else(|_| Value::String(value.to_owned()));
                    flags.set_override(key.trim(), value);
                }
            }
        }
        flags
    }

    /// A flag's value.
    pub fn get<T: Clone + DeserializeOwned>(&self, flag: &Flag<T>) -> T {
        let pick = |map: &Arc<Mutex<BTreeMap<String, Value>>>| {
            map.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(flag.key)
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
        };
        pick(&self.overrides).or_else(|| pick(&self.remote)).unwrap_or_else(|| flag.default.clone())
    }

    /// Applies fetched remote configuration (an object of flag values).
    pub fn apply_remote(&self, configuration: &Value) {
        if let Some(object) = configuration.as_object() {
            let mut remote = self.remote.lock().unwrap_or_else(PoisonError::into_inner);
            for (key, value) in object {
                remote.insert(key.clone(), value.clone());
            }
            self.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Overrides a flag locally.
    pub fn set_override(&self, key: &str, value: Value) {
        self.overrides.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_owned(), value);
        self.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// The remote configuration, to cache (offline, the cached copy is
    /// applied at start).
    #[must_use]
    pub fn to_cache(&self) -> Value {
        Value::Object(
            self.remote
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .into_iter()
                .collect(),
        )
    }

    /// Bumps whenever a value may have changed.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Fetches the remote configuration from `url` (a JSON object) and
    /// applies it.
    ///
    /// # Errors
    ///
    /// The fetch failed; the current values stay.
    pub async fn refresh(
        &self,
        http: &dyn crate::services::HttpService,
        url: &str,
    ) -> Result<(), ServiceError> {
        let response = http.execute(crate::services::HttpRequest::get(url)).await?;
        let value: Value = serde_json::from_slice(response.body_bytes())
            .map_err(|error| ServiceError::new(error.to_string()))?;
        self.apply_remote(&value);
        Ok(())
    }
}

// --- Remote push -----------------------------------------------------------

/// A push notification with its actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushMessage {
    /// Its title.
    pub title: String,
    /// Its body.
    pub body: String,
    /// Its action buttons: an id and a label each. Choosing one arrives as
    /// `Event::NotificationAction`.
    pub actions: Vec<(String, String)>,
    /// Data for the application.
    pub data: Value,
}

/// Why a host service cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unavailable {
    /// Why, as the host states it.
    pub reason: String,
}

/// Remote push: registration and its rotating token.
#[async_trait::async_trait]
pub trait PushService: Send + Sync {
    /// Registers for push; the token to give the application's server.
    ///
    /// # Errors
    ///
    /// Push is unavailable here, with why.
    async fn register(&self, topics: &[String]) -> Result<String, Unavailable>;
}

// --- Commerce --------------------------------------------------------------

/// Something for sale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    /// The store's id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its price, formatted by the store.
    pub price: String,
    /// Whether it renews.
    pub subscription: bool,
}

/// What a purchase proves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// The product.
    pub product: String,
    /// The store's transaction id.
    pub transaction: String,
    /// The store's signed receipt, for server-side validation.
    pub signed: String,
}

/// A store's billing.
#[async_trait::async_trait]
pub trait CommerceService: Send + Sync {
    /// The catalogue.
    async fn products(&self) -> Result<Vec<Product>, Unavailable>;
    /// Buys `product`.
    async fn purchase(&self, product: &str) -> Result<Receipt, Unavailable>;
    /// What the person owns (restoration).
    async fn entitlements(&self) -> Result<Vec<Receipt>, Unavailable>;
}

/// A store for tests and development, validating its own receipts.
#[derive(Debug, Clone, Default)]
pub struct FakeStore {
    catalogue: Vec<Product>,
    owned: Arc<Mutex<Vec<Receipt>>>,
}

impl FakeStore {
    /// A store selling `catalogue`.
    #[must_use]
    pub fn new(catalogue: Vec<Product>) -> Self {
        Self { catalogue, owned: Arc::default() }
    }

    /// Whether a receipt is one this store issued (what a server-side
    /// validator checks against the real store).
    #[must_use]
    pub fn validate(&self, receipt: &Receipt) -> bool {
        receipt.signed == format!("fake-signature:{}:{}", receipt.product, receipt.transaction)
    }
}

#[async_trait::async_trait]
impl CommerceService for FakeStore {
    async fn products(&self) -> Result<Vec<Product>, Unavailable> {
        Ok(self.catalogue.clone())
    }

    async fn purchase(&self, product: &str) -> Result<Receipt, Unavailable> {
        if !self.catalogue.iter().any(|candidate| candidate.id == product) {
            return Err(Unavailable { reason: format!("no product {product}") });
        }
        let mut owned = self.owned.lock().unwrap_or_else(PoisonError::into_inner);
        let transaction = format!("t{}", owned.len() + 1);
        let receipt = Receipt {
            product: product.to_owned(),
            signed: format!("fake-signature:{product}:{transaction}"),
            transaction,
        };
        owned.push(receipt.clone());
        Ok(receipt)
    }

    async fn entitlements(&self) -> Result<Vec<Receipt>, Unavailable> {
        Ok(self.owned.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
}
