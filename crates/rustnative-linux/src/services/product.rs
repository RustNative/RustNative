//! Remote push and store billing on Linux (`PLAN.md` Milestone 57),
//! answered honestly: a Linux desktop has neither.
//!
//! - **Push**: no Linux desktop runs a system push service an application
//!   registers with (UnifiedPush distributors exist, but none ships with a
//!   desktop); an application keeps its own connection to its server.
//! - **Billing**: Flathub, Snapcraft, and the distributions' repositories
//!   sell nothing through an API an application calls.
//!
//! Each returns [`Unavailable`] with that reason, so an application can
//! show why rather than fail silently.

use rustnative_core::product::{CommerceService, Product, PushService, Receipt, Unavailable};

const NO_PUSH: &str =
    "Linux desktops have no system push service; keep a connection to your server instead";
const NO_STORE: &str =
    "Linux app stores (Flathub, Snapcraft, distribution repositories) offer no in-app billing";

/// Remote push on Linux: unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxPush;

#[async_trait::async_trait]
impl PushService for LinuxPush {
    async fn register(&self, _topics: &[String]) -> Result<String, Unavailable> {
        Err(Unavailable { reason: NO_PUSH.to_owned() })
    }
}

/// Store billing on Linux: unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxStore;

#[async_trait::async_trait]
impl CommerceService for LinuxStore {
    async fn products(&self) -> Result<Vec<Product>, Unavailable> {
        Err(Unavailable { reason: NO_STORE.to_owned() })
    }
    async fn purchase(&self, _product: &str) -> Result<Receipt, Unavailable> {
        Err(Unavailable { reason: NO_STORE.to_owned() })
    }
    async fn entitlements(&self) -> Result<Vec<Receipt>, Unavailable> {
        Err(Unavailable { reason: NO_STORE.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_services_say_why() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().expect("a runtime");
        let push = runtime.block_on(LinuxPush.register(&["news".into()])).expect_err("unavailable");
        assert!(push.reason.contains("push"), "{}", push.reason);
        let store = runtime.block_on(LinuxStore.products()).expect_err("unavailable");
        assert!(store.reason.contains("billing"), "{}", store.reason);
    }
}
