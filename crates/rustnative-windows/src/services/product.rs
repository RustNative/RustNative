//! Remote push and store billing on Windows (`PLAN.md` Milestone 57),
//! answered honestly: both need what a plain executable does not have.
//!
//! - **Push** is WNS: `PushNotificationChannelManager` hands out a channel
//!   only to an application with Store-associated package identity. The
//!   server side (sending to a channel) is Milestone 49's `wns_request`.
//! - **Billing** is the Microsoft Store's `StoreContext`, which answers
//!   only an application the Store published.
//!
//! Each returns [`Unavailable`] with that reason, so an application can
//! show why rather than fail silently; `FakeStore` implements billing for
//! development and tests.

use rustnative_core::product::{CommerceService, Product, PushService, Receipt, Unavailable};

const NEEDS_IDENTITY: &str =
    "Windows push (WNS) needs Store-associated package identity; this build has none";
const NEEDS_STORE: &str = "Microsoft Store billing answers only an application the Store published";

/// Remote push on Windows: unavailable without package identity.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsPush;

#[async_trait::async_trait]
impl PushService for WindowsPush {
    async fn register(&self, _topics: &[String]) -> Result<String, Unavailable> {
        Err(Unavailable { reason: NEEDS_IDENTITY.to_owned() })
    }
}

/// Store billing on Windows: unavailable outside a Store-published build.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsStore;

#[async_trait::async_trait]
impl CommerceService for WindowsStore {
    async fn products(&self) -> Result<Vec<Product>, Unavailable> {
        Err(Unavailable { reason: NEEDS_STORE.to_owned() })
    }
    async fn purchase(&self, _product: &str) -> Result<Receipt, Unavailable> {
        Err(Unavailable { reason: NEEDS_STORE.to_owned() })
    }
    async fn entitlements(&self) -> Result<Vec<Receipt>, Unavailable> {
        Err(Unavailable { reason: NEEDS_STORE.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_services_say_why() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let push = runtime.block_on(WindowsPush.register(&["news".into()])).unwrap_err();
        assert!(push.reason.contains("package identity"), "{}", push.reason);
        assert!(runtime.block_on(WindowsStore.products()).unwrap_err().reason.contains("Store"));
    }
}
