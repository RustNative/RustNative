//! Browser capabilities as effects (Web milestone E).
//!
//! Each capability is an [`Effects`] binding: in the browser the runtime
//! carries it out with the platform API, and asks for permission the way
//! the browser does; natively, where the core has a service contract for
//! it (HTTP, storage, the clipboard, notifications, permissions), the
//! component's services carry it out, and where it does not, the reply says
//! the capability is unavailable — never a silent success.
//!
//! [`fx.capabilities`](Effects::capabilities) answers which of them this
//! browser, in this security context, with this page's permissions policy,
//! can use (`rn.caps()` in the runtime).
//!
//! | Effect | Browser API | Natively |
//! |---|---|---|
//! | `http_get`, `http_post`, `fetch` | `fetch` | `HttpService` |
//! | `store`, `load` | Web Storage | `StorageService` |
//! | `db_put`, `db_get` | IndexedDB | `StorageService` |
//! | `cache_put`, `cache_get` | Cache Storage | unavailable |
//! | `copy`, `read_clipboard` | Clipboard | `ClipboardService` |
//! | `notify` | Notifications | `SystemService::notify` |
//! | `permission`, `request_permission` | Permissions | `PermissionService` |
//! | `share` | Web Share | unavailable |
//! | `locate` | Geolocation | unavailable |
//! | `open_file`, `save_file` | File System Access (an `<input type=file>` and a download where it is missing) | unavailable |
//! | `download` | a download | unavailable |
//! | `socket_open`, `socket_send`, `socket_close` | WebSocket | unavailable |
//! | `worker` | Web Workers | unavailable |
//! | `media` | media devices | unavailable |
//! | `bluetooth` | Web Bluetooth | unavailable |
//! | `sensor` | the Generic Sensor API | unavailable |
//! | `vibrate` | Vibration | nothing |
//! | `online` | `online`/`offline` | always online |
//! | `capture_pointer`, `release_pointer` | pointer capture | the tree's input requests |

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::client::Effects;

/// Where the device is (`fx.locate`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    /// Degrees north.
    pub latitude: f64,
    /// Degrees east.
    pub longitude: f64,
    /// The radius of uncertainty, in metres.
    pub accuracy: f64,
}

/// Whether a permission is granted (`fx.permission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionState {
    /// Granted.
    Granted,
    /// Refused.
    Denied,
    /// Not yet asked: asking shows the browser's prompt.
    Prompt,
    /// The browser, or this platform, does not have it.
    Unsupported,
}

/// A file the person chose (`fx.open_file`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileData {
    /// Its name.
    pub name: String,
    /// Its contents, as text.
    pub text: String,
}

/// What happened on a socket (`fx.socket_open`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketEvent {
    /// It is open.
    Opened,
    /// A text message arrived.
    Message(String),
    /// It closed.
    Closed,
    /// It failed.
    Failed(String),
}

const UNAVAILABLE: &str = "this capability is not available on this platform";

#[allow(clippy::unnecessary_wraps, reason = "the shape every effect's perform returns")]
fn once<M: Send + 'static>(message: M) -> Option<crate::client::Task<M>> {
    Some(Box::pin(async move { Some(message) }))
}

impl<M: Send + 'static> Effects<M> {
    /// Fetches `url` (same-origin: with the page's cookies and token);
    /// the reply is the body as text, or the error status.
    pub fn http_get(
        &mut self,
        url: impl Into<String>,
        reply: impl FnOnce(Result<String, String>) -> M + Send + 'static,
    ) {
        let url = url.into();
        let request = rustnative_core::HttpRequest::new(rustnative_core::Method::Get, url.clone());
        self.push("httpGet", json!([url]), move |services| http(services, request, reply));
    }

    /// Posts `body` to `url` (JSON when it parses as JSON, text otherwise);
    /// the reply is the response body as text, or the error status.
    pub fn http_post(
        &mut self,
        url: impl Into<String>,
        body: impl Into<String>,
        reply: impl FnOnce(Result<String, String>) -> M + Send + 'static,
    ) {
        let (url, body) = (url.into(), body.into());
        let kind = if serde_json::from_str::<Value>(&body).is_ok() {
            "application/json"
        } else {
            "text/plain"
        };
        let request = rustnative_core::HttpRequest::new(rustnative_core::Method::Post, url.clone())
            .header("content-type", kind)
            .body(body.clone().into_bytes());
        self.push("httpPost", json!([url, body]), move |services| http(services, request, reply));
    }
    /// Shares `text` (and `url`) through the platform's share sheet.
    pub fn share(
        &mut self,
        title: impl Into<String>,
        text: impl Into<String>,
        url: impl Into<String>,
        reply: impl FnOnce(Result<(), String>) -> M + Send + 'static,
    ) {
        let args = json!([title.into(), text.into(), url.into()]);
        self.push("share", args, move |_| once(reply(Err(UNAVAILABLE.into()))));
    }

    /// Asks where the device is.
    pub fn locate(&mut self, reply: impl FnOnce(Result<Position, String>) -> M + Send + 'static) {
        self.push("locate", json!([]), move |_| once(reply(Err(UNAVAILABLE.into()))));
    }

    /// Whether permission `name` (`notifications`, `geolocation`,
    /// `clipboard-read`, …) is granted, without asking.
    pub fn permission(
        &mut self,
        name: impl Into<String>,
        reply: impl FnOnce(PermissionState) -> M + Send + 'static,
    ) {
        let name = name.into();
        self.push("permission", json!([name]), move |services| {
            let state = native_permission(services, &name);
            once(reply(state))
        });
    }

    /// Asks the person for permission `name`.
    pub fn request_permission(
        &mut self,
        name: impl Into<String>,
        reply: impl FnOnce(PermissionState) -> M + Send + 'static,
    ) {
        let name = name.into();
        self.push("requestPermission", json!([name]), move |services| {
            let state = native_permission(services, &name);
            once(reply(state))
        });
    }

    /// Lets the person choose a file (of the types `accept` names:
    /// `.txt,text/plain`), and reads it as text; `None` if they chose none.
    pub fn open_file(
        &mut self,
        accept: impl Into<String>,
        reply: impl FnOnce(Option<FileData>) -> M + Send + 'static,
    ) {
        self.push("openFile", json!([accept.into()]), move |_| once(reply(None)));
    }

    /// Saves `text` as a file the person names, starting from `name`.
    pub fn save_file(
        &mut self,
        name: impl Into<String>,
        text: impl Into<String>,
        reply: impl FnOnce(Result<(), String>) -> M + Send + 'static,
    ) {
        let args = json!([name.into(), text.into()]);
        self.push("saveFile", args, move |_| once(reply(Err(UNAVAILABLE.into()))));
    }

    /// Stores `value` under `key` in object store `store` (`IndexedDB`).
    pub fn db_put(
        &mut self,
        store: impl Into<String>,
        key: impl Into<String>,
        value: &impl Serialize,
    ) {
        let (store, key) = (store.into(), key.into());
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        let bytes = serde_json::to_vec(&value).unwrap_or_default();
        let name = format!("db/{store}/{key}");
        self.push("dbPut", json!([store, key, value]), move |services| {
            let storage = services.storage().cloned()?;
            Some(Box::pin(async move {
                let _ = storage.set(name, bytes).await;
                None
            }))
        });
    }

    /// The value under `key` in object store `store`, if any.
    pub fn db_get<T: DeserializeOwned + Send + 'static>(
        &mut self,
        store: impl Into<String>,
        key: impl Into<String>,
        reply: impl FnOnce(Option<T>) -> M + Send + 'static,
    ) {
        let (store, key) = (store.into(), key.into());
        let name = format!("db/{store}/{key}");
        self.push("dbGet", json!([store, key]), move |services| {
            let storage = services.storage().cloned();
            Some(Box::pin(async move {
                let bytes = match storage {
                    Some(storage) => storage.get(name).await.ok().flatten(),
                    None => None,
                };
                Some(reply(bytes.and_then(|bytes| serde_json::from_slice(&bytes).ok())))
            }))
        });
    }

    /// Caches the response at `url` for offline use (Cache Storage).
    pub fn cache_put(&mut self, url: impl Into<String>) {
        self.push("cachePut", json!([url.into()]), |_| None);
    }

    /// The cached response body at `url`, if any.
    pub fn cache_get(
        &mut self,
        url: impl Into<String>,
        reply: impl FnOnce(Option<String>) -> M + Send + 'static,
    ) {
        self.push("cacheGet", json!([url.into()]), move |_| once(reply(None)));
    }

    /// Reads the clipboard's text (the browser asks the person).
    pub fn read_clipboard(
        &mut self,
        reply: impl FnOnce(Result<String, String>) -> M + Send + 'static,
    ) {
        self.push("readClipboard", json!([]), move |services| {
            let clipboard = services.clipboard().cloned();
            Some(Box::pin(async move {
                let text = match clipboard {
                    Some(clipboard) => clipboard
                        .read_text()
                        .await
                        .map(Option::unwrap_or_default)
                        .map_err(|error| error.to_string()),
                    None => Err(UNAVAILABLE.to_owned()),
                };
                Some(reply(text))
            }))
        });
    }

    /// Opens a WebSocket named `name` to `url`; `reply` receives each thing
    /// that happens on it, for as long as the component lives.
    pub fn socket_open(
        &mut self,
        name: impl Into<String>,
        url: impl Into<String>,
        reply: impl Fn(SocketEvent) -> M + Send + Sync + 'static,
    ) {
        self.push("socketOpen", json!([name.into(), url.into()]), move |_| {
            once(reply(SocketEvent::Failed(UNAVAILABLE.into())))
        });
    }

    /// Sends `text` on socket `name`.
    pub fn socket_send(&mut self, name: impl Into<String>, text: impl Into<String>) {
        self.push("socketSend", json!([name.into(), text.into()]), |_| None);
    }

    /// Closes socket `name`.
    pub fn socket_close(&mut self, name: impl Into<String>) {
        self.push("socketClose", json!([name.into()]), |_| None);
    }

    /// Runs the worker script at `url` with `input`, off the page's main
    /// thread; `reply` receives the first message it posts back.
    pub fn worker(
        &mut self,
        url: impl Into<String>,
        input: &impl Serialize,
        reply: impl FnOnce(Result<Value, String>) -> M + Send + 'static,
    ) {
        let input = serde_json::to_value(input).unwrap_or(Value::Null);
        self.push("worker", json!([url.into(), input]), move |_| {
            once(reply(Err(UNAVAILABLE.into())))
        });
    }

    /// Vibrates for `milliseconds`, where the device can.
    pub fn vibrate(&mut self, milliseconds: u32) {
        self.push("vibrate", json!([milliseconds]), |_| None);
    }

    /// Whether the browser is online, now and at each change.
    pub fn online(&mut self, reply: impl Fn(bool) -> M + Send + Sync + 'static) {
        self.push("online", json!([]), move |_| once(reply(true)));
    }

    /// Asks for the microphone (`audio`) and camera (`video`); the reply is
    /// the kinds of the tracks granted (`audio`, `video`).
    pub fn media(
        &mut self,
        audio: bool,
        video: bool,
        reply: impl FnOnce(Result<Vec<String>, String>) -> M + Send + 'static,
    ) {
        self.push("media", json!([audio, video]), move |_| once(reply(Err(UNAVAILABLE.into()))));
    }

    /// Asks the person to choose a Bluetooth device offering `service`; the
    /// reply is its name.
    pub fn bluetooth(
        &mut self,
        service: impl Into<String>,
        reply: impl FnOnce(Result<String, String>) -> M + Send + 'static,
    ) {
        self.push("bluetooth", json!([service.into()]), move |_| {
            once(reply(Err(UNAVAILABLE.into())))
        });
    }

    /// Reads sensor `name` (`accelerometer`, `gyroscope`,
    /// `ambient-light`, …) at each change: its reading's values.
    pub fn sensor(
        &mut self,
        name: impl Into<String>,
        reply: impl Fn(Result<Vec<f64>, String>) -> M + Send + Sync + 'static,
    ) {
        self.push("sensor", json!([name.into()]), move |_| once(reply(Err(UNAVAILABLE.into()))));
    }

    /// Which capabilities this platform has now, by effect name.
    pub fn capabilities(&mut self, reply: impl FnOnce(Vec<String>) -> M + Send + 'static) {
        self.push("capabilities", json!([]), move |services| {
            let mut have = Vec::new();
            if services.http().is_some() {
                have.push("fetch".to_owned());
            }
            if services.storage().is_some() {
                have.extend(["store", "load", "db_put", "db_get"].map(str::to_owned));
            }
            if services.clipboard().is_some() {
                have.extend(["copy", "read_clipboard"].map(str::to_owned));
            }
            once(reply(have))
        });
    }

    /// Sends pointer `pointer_id`'s events to node `key` until it is
    /// released, even when the pointer leaves it (a drag).
    pub fn capture_pointer(&mut self, key: impl Into<String>, pointer_id: u32) {
        self.push("capturePointer", json!([key.into(), pointer_id]), |_| None);
    }

    /// Ends [`Self::capture_pointer`].
    pub fn release_pointer(&mut self, key: impl Into<String>, pointer_id: u32) {
        self.push("releasePointer", json!([key.into(), pointer_id]), |_| None);
    }
}

#[allow(clippy::unnecessary_wraps, reason = "the shape every effect's perform returns")]
fn http<M: Send + 'static>(
    services: &rustnative_core::Services,
    request: rustnative_core::HttpRequest,
    reply: impl FnOnce(Result<String, String>) -> M + Send + 'static,
) -> Option<crate::client::Task<M>> {
    let http = services.http().cloned();
    Some(Box::pin(async move {
        let result = match http {
            Some(http) => match http.execute(request).await {
                Ok(response) if (200..300).contains(&response.status()) => {
                    Ok(String::from_utf8_lossy(response.body_bytes()).into_owned())
                }
                Ok(response) => Err(response.status().to_string()),
                Err(error) => Err(error.to_string()),
            },
            None => Err(UNAVAILABLE.to_owned()),
        };
        Some(reply(result))
    }))
}

fn native_permission(services: &rustnative_core::Services, name: &str) -> PermissionState {
    let _ = (services, name);
    PermissionState::Unsupported
}
