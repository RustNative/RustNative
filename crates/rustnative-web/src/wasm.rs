//! WebAssembly subtrees (Web milestone A's opt-in, and F): a component —
//! any component, with full Rust — built for `wasm32-unknown-unknown` and
//! run in the browser, in the page or in a Web Worker.
//!
//! A crate built for the browser names its components once:
//!
//! ```ignore
//! rustnative_web::wasm_subtree! {
//!     "counter" => rustnative_web::Client<counter::Counter>,
//!     "notes" => notes::Notes,
//! }
//! ```
//!
//! which, on `wasm32`, exports the module's interface to the runtime
//! (`rn_alloc`, `rn_init`, `rn_dispatch`, `rn_pump`, `rn_view`,
//! `rn_deadline`, `rn_complete`), and on every other target is nothing. The
//! glue is the framework's own: there is no `wasm-bindgen`, and the module
//! imports only the runtime's `rn` functions — the host clock, a wake-up
//! request, and one request channel through which the component's services
//! (HTTP, storage, the clipboard) reach the browser.
//!
//! Inside, a `ComponentTree` on a [`HostExecutor`] over the browser's clock
//! renders the component; its view leaves as the same elements the rest of
//! the page is made of, and the runtime patches them in.
//!
//! On the server, [`WasmSubtree`] renders the same component natively in
//! the page — so the subtree's first markup is there before the module
//! loads — and registers it as an island, so the module is loaded only by
//! pages that have one.
//!
//! [`HostExecutor`]: rustnative_core::HostExecutor

use rustnative_core::{Component, ComponentContext, Event, Node};
use serde::Serialize;

use crate::client::{Island, IslandKind, ServerRender};

/// A WebAssembly subtree: the module and the component in it, the
/// component's props (also the state the browser starts it from), and
/// whether it runs in a Web Worker.
pub struct WasmProps<C: Component> {
    /// The module's name (served at `/_rn/w/{module}.wasm`).
    pub module: &'static str,
    /// The component's name in the module (`wasm_subtree!`).
    pub component: &'static str,
    /// The component's props.
    pub props: C::Props,
    /// Run it in a Web Worker, off the page's main thread.
    pub worker: bool,
}

impl<C: Component> Clone for WasmProps<C> {
    fn clone(&self) -> Self {
        Self {
            module: self.module,
            component: self.component,
            props: self.props.clone(),
            worker: self.worker,
        }
    }
}

impl<C: Component> PartialEq for WasmProps<C> {
    fn eq(&self, other: &Self) -> bool {
        self.module == other.module
            && self.component == other.component
            && self.props == other.props
            && self.worker == other.worker
    }
}

impl<C: Component> WasmProps<C> {
    /// Component `component` of module `module`, starting from `props`, on
    /// the page's main thread.
    pub const fn new(module: &'static str, component: &'static str, props: C::Props) -> Self {
        Self { module, component, props, worker: false }
    }

    /// In a Web Worker.
    #[must_use]
    pub fn in_worker(mut self) -> Self {
        self.worker = true;
        self
    }
}

/// See the [module documentation](self).
pub struct WasmSubtree<C: Component> {
    props: WasmProps<C>,
}

impl<C: Component> Component for WasmSubtree<C>
where
    C::Props: Serialize,
{
    type Props = WasmProps<C>;
    type Message = ();

    fn new(props: WasmProps<C>) -> Self {
        Self { props }
    }

    fn props(&self) -> &WasmProps<C> {
        &self.props
    }

    fn set_props(&mut self, props: WasmProps<C>) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column("wasm", [])
    }

    fn update(&mut self, _: Event) {}

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let node = context.child_with_props::<C, _>("wasm", self.props.props.clone(), C::new);
        if let Some(render) = context.services().extension::<ServerRender>() {
            if let (Some(owner), Ok(state)) =
                (node.id().owner(), crate::client::state_json(&self.props.props))
            {
                render.register(Island {
                    owner,
                    kind: IslandKind::Wasm {
                        module: self.props.module,
                        component: self.props.component,
                        worker: self.props.worker,
                    },
                    state,
                    persist: false,
                });
            }
        }
        node
    }
}

/// Names a crate's components for the browser; see the [module
/// documentation](crate::wasm).
#[macro_export]
macro_rules! wasm_subtree {
    ($($name:literal => $component:ty),+ $(,)?) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            fn __rn_components() -> ::std::vec::Vec<(&'static str, $crate::wasm::host::Start)> {
                ::std::vec![$(($name, $crate::wasm::host::start::<$component> as $crate::wasm::host::Start)),+]
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_alloc(length: usize) -> usize {
                $crate::wasm::host::alloc(length)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_init(length: usize) -> u32 {
                $crate::wasm::host::init(length, &__rn_components())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_dispatch(length: usize) {
                $crate::wasm::host::dispatch(length);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_pump() -> u32 {
                $crate::wasm::host::pump()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_view() -> usize {
                $crate::wasm::host::view()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_out() -> usize {
                $crate::wasm::host::out()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_deadline() -> f64 {
                $crate::wasm::host::deadline()
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn rn_complete(id: u32, length: usize) {
                $crate::wasm::host::complete(id, length);
            }
        };
    };
}

/// The browser host a `wasm_subtree!` module runs its component in.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod host {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    use rustnative_core::services::ServiceError;
    use rustnative_core::{
        ClipboardService, Clock, Component, ComponentTree, HostExecutor, HttpRequest, HttpResponse,
        HttpService, Scheduler, Services, StorageService, Theme,
    };
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};

    #[link(wasm_import_module = "rn")]
    unsafe extern "C" {
        /// The page's clock, in milliseconds.
        fn rn_now() -> f64;
        /// Asks the runtime to call `rn_pump` after `delay` milliseconds.
        fn rn_wake(delay: f64);
        /// A request of the component's services: `id`, and the request as
        /// JSON at `pointer` (`length` bytes). The answer arrives through
        /// `rn_complete`.
        fn rn_request(id: u32, pointer: usize, length: usize);
    }

    /// Builds a component tree from its props.
    pub type Start = fn(Value, Services, Scheduler) -> Option<ComponentTree>;

    /// A component started from its props' JSON.
    #[must_use]
    pub fn start<C: Component>(
        props: Value,
        services: Services,
        scheduler: Scheduler,
    ) -> Option<ComponentTree>
    where
        C::Props: DeserializeOwned,
    {
        let props: C::Props = serde_json::from_value(props).ok()?;
        Some(ComponentTree::with_scheduler(C::new(props), services, Theme::default(), scheduler))
    }

    struct BrowserClock;

    impl Clock for BrowserClock {
        fn now(&self) -> Duration {
            // SAFETY: an import the runtime provides, taking and returning
            // plain numbers.
            let milliseconds = unsafe { rn_now() };
            Duration::from_secs_f64(milliseconds.max(0.0) / 1000.0)
        }
    }

    struct Host {
        tree: ComponentTree,
        executor: HostExecutor,
        options: Value,
    }

    #[derive(Default)]
    struct Pending {
        answer: Option<Value>,
        waker: Option<Waker>,
    }

    thread_local! {
        static INBOX: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static OUTBOX: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    }

    /// Requests in flight, by id; shared with the services, which must be
    /// `Send + Sync` (a module has one thread, so the lock never waits).
    static REQUESTS: Mutex<Option<HashMap<u32, Pending>>> = Mutex::new(None);
    static NEXT: Mutex<u32> = Mutex::new(1);

    fn inbox() -> Vec<u8> {
        INBOX.with(|inbox| std::mem::take(&mut *inbox.borrow_mut()))
    }

    /// A buffer of `length` bytes the runtime writes its input into.
    #[must_use]
    pub fn alloc(length: usize) -> usize {
        INBOX.with(|inbox| {
            let mut inbox = inbox.borrow_mut();
            inbox.clear();
            inbox.resize(length, 0);
            inbox.as_mut_ptr() as usize
        })
    }

    /// Starts the component named in the input (`{component, props, scope,
    /// flow}`); `1` if it started.
    #[must_use]
    pub fn init(length: usize, components: &[(&'static str, Start)]) -> u32 {
        let input: Value = serde_json::from_slice(&inbox()[..length]).unwrap_or_default();
        let name = input.get("component").and_then(Value::as_str).unwrap_or_default();
        let Some((_, start)) = components.iter().find(|(candidate, _)| *candidate == name) else {
            return 0;
        };
        let executor = HostExecutor::new(Arc::new(BrowserClock));
        executor.set_wake(Arc::new(|| {
            // SAFETY: an import the runtime provides, taking a number.
            unsafe { rn_wake(0.0) };
        }));
        let scheduler = Scheduler::with_executor(Arc::new(executor.clone()));
        let services = Services::default()
            .with_http(Arc::new(Bridge))
            .with_storage(Arc::new(Bridge))
            .with_clipboard(Arc::new(Bridge));
        let props = input.get("props").cloned().unwrap_or(Value::Null);
        let Some(mut tree) = start(props, services, scheduler) else { return 0 };
        let _ = tree.render();
        let options = json!({ "scope": input.get("scope"), "flow": input.get("flow") });
        HOST.with(|host| *host.borrow_mut() = Some(Host { tree, executor, options }));
        1
    }

    /// Delivers the event in the input.
    pub fn dispatch(length: usize) {
        let input: Value = serde_json::from_slice(&inbox()[..length]).unwrap_or_default();
        if let Some(event) = crate::client::event_from_json(&input) {
            HOST.with(|host| {
                if let Some(host) = host.borrow_mut().as_mut() {
                    host.tree.dispatch(event);
                }
            });
        }
    }

    /// Runs the tasks that are ready; `1` if the view may have changed.
    #[must_use]
    pub fn pump() -> u32 {
        HOST.with(|host| {
            host.borrow_mut().as_mut().map_or(0, |host| u32::from(host.tree.pump_tasks()))
        })
    }

    /// Milliseconds until the next delay is due (`-1`: none).
    #[must_use]
    pub fn deadline() -> f64 {
        HOST.with(|host| {
            let host = host.borrow();
            let Some(host) = host.as_ref() else { return -1.0 };
            host.executor.next_deadline().map_or(-1.0, |deadline| {
                let now = BrowserClock.now();
                deadline.saturating_sub(now).as_secs_f64() * 1000.0
            })
        })
    }

    /// The view as elements, written to the output buffer; its length.
    #[must_use]
    pub fn view() -> usize {
        let encoded = HOST.with(|host| {
            let host = host.borrow();
            host.as_ref()
                .map(|host| crate::live::encode(&host.tree.unresolved_view(), &host.options))
        });
        let bytes = serde_json::to_vec(&encoded.unwrap_or(Value::Null)).unwrap_or_default();
        let length = bytes.len();
        OUTBOX.with(|outbox| *outbox.borrow_mut() = bytes);
        length
    }

    /// Where the output buffer is.
    #[must_use]
    pub fn out() -> usize {
        OUTBOX.with(|outbox| outbox.borrow().as_ptr() as usize)
    }

    /// The answer to request `id`, in the input.
    pub fn complete(id: u32, length: usize) {
        let answer: Value = serde_json::from_slice(&inbox()[..length]).unwrap_or(Value::Null);
        let waker = {
            let mut requests = REQUESTS.lock().unwrap_or_else(PoisonError::into_inner);
            let pending = requests.get_or_insert_with(HashMap::new).entry(id).or_default();
            pending.answer = Some(answer);
            pending.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// One request to the runtime, answered through `complete`.
    struct Request {
        id: u32,
    }

    impl Future for Request {
        type Output = Value;
        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Value> {
            let mut requests = REQUESTS.lock().unwrap_or_else(PoisonError::into_inner);
            let pending = requests.get_or_insert_with(HashMap::new).entry(self.id).or_default();
            if let Some(answer) = pending.answer.take() {
                requests.get_or_insert_with(HashMap::new).remove(&self.id);
                return Poll::Ready(answer);
            }
            pending.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }

    fn request(value: &Value) -> Request {
        let id = {
            let mut next = NEXT.lock().unwrap_or_else(PoisonError::into_inner);
            *next += 1;
            *next
        };
        let bytes = serde_json::to_vec(value).unwrap_or_default();
        // SAFETY: the pointer and length describe `bytes`, alive for the
        // call; the runtime copies them out before returning.
        unsafe { rn_request(id, bytes.as_ptr() as usize, bytes.len()) };
        Request { id }
    }

    fn error(answer: &Value) -> Option<ServiceError> {
        answer.get("error").and_then(Value::as_str).map(ServiceError::new)
    }

    /// The component's services, carried out by the runtime.
    struct Bridge;

    #[async_trait::async_trait]
    impl HttpService for Bridge {
        async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
            let headers: Vec<(String, String)> = request.headers().to_vec();
            let answer = self::request(&json!({
                "kind": "fetch",
                "url": request.url(),
                "method": format!("{:?}", request.method()).to_uppercase(),
                "headers": headers,
                "body": String::from_utf8_lossy(request.body_bytes()),
            }))
            .await;
            if let Some(error) = error(&answer) {
                return Err(error);
            }
            let status = answer
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .unwrap_or(0);
            let body =
                answer.get("body").and_then(Value::as_str).unwrap_or_default().as_bytes().to_vec();
            let headers = answer
                .get("headers")
                .and_then(|headers| serde_json::from_value(headers.clone()).ok())
                .unwrap_or_default();
            Ok(HttpResponse::new(status, headers, body))
        }
    }

    #[async_trait::async_trait]
    impl StorageService for Bridge {
        async fn get(&self, key: String) -> Result<Option<Vec<u8>>, ServiceError> {
            let answer = request(&json!({ "kind": "storage_get", "key": key })).await;
            if let Some(error) = error(&answer) {
                return Err(error);
            }
            Ok(answer.get("value").and_then(Value::as_str).map(|value| value.as_bytes().to_vec()))
        }

        async fn set(&self, key: String, value: Vec<u8>) -> Result<(), ServiceError> {
            let value = String::from_utf8_lossy(&value).into_owned();
            let answer =
                request(&json!({ "kind": "storage_set", "key": key, "value": value })).await;
            error(&answer).map_or(Ok(()), Err)
        }

        async fn remove(&self, key: String) -> Result<(), ServiceError> {
            let answer = request(&json!({ "kind": "storage_remove", "key": key })).await;
            error(&answer).map_or(Ok(()), Err)
        }
    }

    #[async_trait::async_trait]
    impl ClipboardService for Bridge {
        async fn read_text(&self) -> Result<Option<String>, ServiceError> {
            let answer = request(&json!({ "kind": "clipboard_read" })).await;
            if let Some(error) = error(&answer) {
                return Err(error);
            }
            Ok(answer.get("text").and_then(Value::as_str).map(str::to_owned))
        }

        async fn write_text(&self, value: String) -> Result<(), ServiceError> {
            let answer = request(&json!({ "kind": "clipboard_write", "text": value })).await;
            error(&answer).map_or(Ok(()), Err)
        }
    }
}
