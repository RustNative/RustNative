//! Push and in-app billing.
//!
//! Push goes through Firebase Cloud Messaging when the application ships
//! it (`firebase-messaging` and its `google-services.json`), reached by
//! reflection so applications that do not push carry no Firebase
//! dependency. Without it, `register` answers `Unavailable` with the
//! reason. Messages arrive through the application's own
//! `FirebaseMessagingService`; a notification's action returns as
//! `Event::SurfaceAction`.
//!
//! Billing answers `Unavailable`: Play Billing needs an application
//! published to a Play Console track to test against, so its bridge is
//! owed (`BUILD_STATUS.md`). Use `FakeStore` in development.

use rustnative_core::product::{CommerceService, Product, PushService, Receipt, Unavailable};

use super::{java, off_thread};
use crate::jni_host::{Arg, Class, Ret};

/// Firebase Cloud Messaging, when present.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidPush;

#[async_trait::async_trait]
impl PushService for AndroidPush {
    async fn register(&self, topics: &[String]) -> Result<String, Unavailable> {
        let topics = topics.to_vec();
        let token = off_thread(move || {
            java(
                Class::Services,
                "pushToken",
                "([Ljava/lang/String;)Ljava/lang/String;",
                &[Arg::Strs(&topics)],
            )
            .map(Ret::string)
        })
        .await
        .and_then(|token| token);
        match token {
            Ok(Some(token)) => Ok(token),
            Ok(None) => {
                Err(Unavailable { reason: "Firebase Cloud Messaging gave no token".to_owned() })
            }
            Err(error) => Err(Unavailable { reason: error.to_string() }),
        }
    }
}

/// In-app billing: unavailable, saying why.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidStore;

fn no_store() -> Unavailable {
    let present = java(Class::Services, "billingPresent", "()Z", &[]).is_ok_and(Ret::bool);
    Unavailable {
        reason: if present {
            "Play Billing is in this application, but the backend's billing bridge is owed: it needs a Play Console \
             track to be verified against"
                .to_owned()
        } else {
            "Play Billing is not in this application, and the backend's billing bridge is owed"
                .to_owned()
        },
    }
}

#[async_trait::async_trait]
impl CommerceService for AndroidStore {
    async fn products(&self) -> Result<Vec<Product>, Unavailable> {
        Err(no_store())
    }

    async fn purchase(&self, _product: &str) -> Result<Receipt, Unavailable> {
        Err(no_store())
    }

    async fn entitlements(&self) -> Result<Vec<Receipt>, Unavailable> {
        Err(no_store())
    }
}
