//! Actors on an edge host (Web milestone K, `C46`): the [`Actor`] contract
//! in the WAGI shape, as an edge platform runs a stateful object.
//!
//! - **Routing**: the host routes every request for one actor id to one
//!   instance at a time (`rustnative serve wagi --actors /actors/`; on a
//!   provider, its per-object routing). The adapter answers
//!   `POST {prefix}{id}` with the message as JSON, and replies with the
//!   actor's reply as JSON.
//! - **Instances**: each request starts an instance from storage, handles
//!   its one message, and ends — the instance lives as long as the request,
//!   which is what an edge host guarantees.
//! - **Storage**: the host's key-value store (`KvBackend`, on `wasm32-wasip1`, through the
//!   `rn_kv` import): `actor/{id}/{key}` for values, `alarm/{id}` for the
//!   alarm. On a provider, its per-object storage takes the same three
//!   operations.
//! - **Alarms**: the host's scheduler calls `POST {prefix}{id}/alarm`; the
//!   alarm runs if it is due, once.
//!
//! [`answer`] is the adapter over any [`Backend`], so the same actor code
//! is exercised natively and on the edge.

use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::actor::{Actor, ActorContext, Backend, Storage, now_ms};

/// Answers one request for an actor: its status and JSON body. `backend`
/// must also implement alarm reads, through [`AlarmClock`].
pub fn answer<A>(
    backend: &Arc<dyn AlarmClock>,
    prefix: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> (u16, String)
where
    A: Actor,
    A::Message: DeserializeOwned,
    A::Reply: Serialize,
{
    let error =
        |status: u16, message: &str| (status, serde_json::json!({ "error": message }).to_string());
    let Some(rest) = path.strip_prefix(prefix) else {
        return error(404, "not an actor path");
    };
    let mut segments = rest.split('/').filter(|segment| !segment.is_empty());
    let Some(id) = segments.next() else {
        return error(404, "no actor id");
    };
    if method != "POST" {
        return error(405, "actors take POST");
    }
    let storage_backend: Arc<dyn Backend> = backend.clone().as_backend();
    let storage = Storage::new(storage_backend, id);
    let context = ActorContext::new(id.to_owned(), storage.clone());
    match segments.next() {
        None => {
            let Ok(message) = serde_json::from_slice::<A::Message>(body) else {
                return error(400, "not a message this actor takes");
            };
            let mut actor = A::start(id, &storage);
            let reply = rustnative_server::serverless::block_on(actor.handle(message, &context));
            (200, serde_json::to_string(&reply).unwrap_or_else(|_| "null".into()))
        }
        Some("alarm") => {
            let fired = backend.take_alarm(id, now_ms());
            if fired {
                let mut actor = A::start(id, &storage);
                rustnative_server::serverless::block_on(actor.alarm(&context));
            }
            (200, serde_json::json!({ "fired": fired }).to_string())
        }
        Some(_) => error(404, "no such actor action"),
    }
}

/// A [`Backend`] whose alarms the adapter can read: an alarm that is due
/// is taken (cleared) and reported once.
pub trait AlarmClock: Backend {
    /// Whether `actor`'s alarm is due at `now`; clears it if so.
    fn take_alarm(&self, actor: &str, now: i64) -> bool;

    /// This, as a plain backend.
    fn as_backend(self: Arc<Self>) -> Arc<dyn Backend>;
}

/// Actors' storage in the edge host's key-value store.
#[cfg(target_os = "wasi")]
pub struct KvBackend;

#[cfg(target_os = "wasi")]
impl Backend for KvBackend {
    fn get(&self, actor: &str, key: &str) -> Option<String> {
        let bytes = rustnative_server::serverless::kv::get(&format!("actor/{actor}/{key}"))?;
        String::from_utf8(bytes).ok()
    }

    fn put(&self, actor: &str, key: &str, value: &str) -> Result<(), crate::StorageError> {
        if rustnative_server::serverless::kv::set(&format!("actor/{actor}/{key}"), value.as_bytes())
        {
            Ok(())
        } else {
            Err(crate::StorageError("the host refused the write".into()))
        }
    }

    fn set_alarm(&self, actor: &str, at: i64) -> Result<(), crate::StorageError> {
        if rustnative_server::serverless::kv::set(
            &format!("alarm/{actor}"),
            at.to_string().as_bytes(),
        ) {
            Ok(())
        } else {
            Err(crate::StorageError("the host refused the write".into()))
        }
    }
}

#[cfg(target_os = "wasi")]
impl AlarmClock for KvBackend {
    fn take_alarm(&self, actor: &str, now: i64) -> bool {
        let key = format!("alarm/{actor}");
        let at = rustnative_server::serverless::kv::get(&key)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.parse::<i64>().ok());
        if at.is_some_and(|at| at <= now) {
            rustnative_server::serverless::kv::delete(&key);
            return true;
        }
        false
    }

    fn as_backend(self: Arc<Self>) -> Arc<dyn Backend> {
        self
    }
}

/// Serves actor type `A` for this run's one request (WAGI): the request
/// from the CGI variables and standard input, the reply on standard
/// output.
#[cfg(target_os = "wasi")]
#[must_use]
pub fn serve<A>(prefix: &str) -> std::process::ExitCode
where
    A: Actor,
    A::Message: DeserializeOwned,
    A::Reply: Serialize,
{
    use std::io::{Read, Write};
    let method = std::env::var("REQUEST_METHOD").unwrap_or_default();
    let path = std::env::var("PATH_INFO").unwrap_or_else(|_| "/".into());
    let mut body = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut body);
    let backend: Arc<dyn AlarmClock> = Arc::new(KvBackend);
    let (status, reply) = answer::<A>(&backend, prefix, &method, &path, &body);
    let mut out = std::io::stdout().lock();
    let _ = write!(
        out,
        "Status: {status}\r\ncontent-type: application/json\r\ncache-control: no-store\r\n\r\n{reply}"
    );
    let _ = out.flush();
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::StorageError;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);

    impl Backend for Memory {
        fn get(&self, actor: &str, key: &str) -> Option<String> {
            self.0.lock().unwrap().get(&format!("{actor}/{key}")).cloned()
        }
        fn put(&self, actor: &str, key: &str, value: &str) -> Result<(), StorageError> {
            self.0.lock().unwrap().insert(format!("{actor}/{key}"), value.to_owned());
            Ok(())
        }
        fn set_alarm(&self, actor: &str, at: i64) -> Result<(), StorageError> {
            self.0.lock().unwrap().insert(format!("alarm/{actor}"), at.to_string());
            Ok(())
        }
    }

    impl AlarmClock for Memory {
        fn take_alarm(&self, actor: &str, now: i64) -> bool {
            let mut map = self.0.lock().unwrap();
            let due = map
                .get(&format!("alarm/{actor}"))
                .and_then(|at| at.parse::<i64>().ok())
                .is_some_and(|at| at <= now);
            if due {
                map.remove(&format!("alarm/{actor}"));
            }
            due
        }
        fn as_backend(self: Arc<Self>) -> Arc<dyn Backend> {
            self
        }
    }

    struct Counter(u32);

    #[async_trait::async_trait]
    impl Actor for Counter {
        type Message = u32;
        type Reply = u32;
        fn start(_: &str, storage: &Storage) -> Self {
            Self(storage.get("count").unwrap_or_default())
        }
        async fn handle(&mut self, add: u32, context: &ActorContext) -> u32 {
            self.0 += add;
            context.storage().put("count", &self.0).unwrap();
            context.set_alarm(std::time::Duration::ZERO).unwrap();
            self.0
        }
        async fn alarm(&mut self, context: &ActorContext) {
            context.storage().put("count", &0).unwrap();
        }
    }

    #[test]
    fn each_request_is_an_instance_that_finds_its_storage() {
        let backend: Arc<dyn AlarmClock> = Arc::new(Memory::default());
        assert_eq!(
            answer::<Counter>(&backend, "/actors/", "POST", "/actors/a", b"2"),
            (200, "2".into())
        );
        assert_eq!(
            answer::<Counter>(&backend, "/actors/", "POST", "/actors/a", b"3"),
            (200, "5".into())
        );
        assert_eq!(
            answer::<Counter>(&backend, "/actors/", "POST", "/actors/b", b"1"),
            (200, "1".into())
        );
        assert_eq!(
            answer::<Counter>(&backend, "/actors/", "POST", "/actors/a/alarm", b"").1,
            r#"{"fired":true}"#
        );
        assert_eq!(
            answer::<Counter>(&backend, "/actors/", "POST", "/actors/a/alarm", b"").1,
            r#"{"fired":false}"#
        );
        assert_eq!(
            answer::<Counter>(&backend, "/actors/", "POST", "/actors/a", b"1"),
            (200, "1".into())
        );
        assert_eq!(answer::<Counter>(&backend, "/actors/", "GET", "/actors/a", b"").0, 405);
        assert_eq!(answer::<Counter>(&backend, "/actors/", "POST", "/actors/a", b"\"x\"").0, 400);
    }
}
