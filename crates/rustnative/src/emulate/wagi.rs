//! `rustnative serve wagi <module.wasm>`: an edge host for a
//! `wasm32-wasip1` module, one instance per request, in the WAGI shape —
//! the request in CGI variables and standard input, the response on
//! standard output.
//!
//! What an edge host limits, the emulator limits, so a route over its
//! budget fails here before it fails there:
//!
//! - **CPU**, in fuel (one unit per instruction, roughly): a default budget
//!   and a budget per route prefix. A request out of fuel is `503` with
//!   `x-rn-limit: fuel`. Every response reports what it used in
//!   `x-rn-fuel`.
//! - **Memory**: a ceiling on the module's linear memory. A request that
//!   grows past it is `503` with `x-rn-limit: memory`.
//! - **Response size**: a body over the cap is `502` with
//!   `x-rn-limit: response`.
//! - **Time**: the deadline is stated to the module
//!   (`RUSTNATIVE_LIMIT_DEADLINE_MS`) and bounds its sleeps.
//!
//! The host provides WASI preview 1 as an edge sandbox does — standard
//! streams, clocks, randomness, environment, sleeping; no file system, no
//! sockets (every other call answers `ENOSYS`) — plus a key-value store,
//! `rn_kv`, the storage an edge actor keeps its state in (see
//! `rustnative_durable::edge`), and outbound HTTP, `rn_http`, to the hosts
//! allowed (`--allow-http`), as an edge host lets a module reach a data
//! service. With an actor prefix, requests for the same
//! actor (the path segment after the prefix) run one at a time, as an edge
//! host routes an actor id to its one instance.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wasmi::core::{LimiterError, ResourceLimiter, TrapCode};
use wasmi::{Caller, Config, Engine, Extern, ExternType, Linker, Memory, Module, Store};

use super::{HttpRequest, io, read_request, write_error, write_response};
use crate::error::{Error, Result};

/// What the host allows each request.
#[derive(Debug, Clone)]
pub struct WagiOptions {
    /// The CPU budget, in fuel, for a route with no budget of its own.
    pub fuel: u64,
    /// Budgets by path prefix; the longest matching prefix wins.
    pub route_fuel: Vec<(String, u64)>,
    /// The ceiling on the module's memory, in bytes.
    pub memory_bytes: u64,
    /// The largest response body, in bytes.
    pub response_bytes: u64,
    /// How long a request may take.
    pub deadline: Duration,
    /// Requests whose path starts with this prefix are for the actor named
    /// by the next segment, and run one at a time per actor.
    pub actor_prefix: Option<String>,
    /// Configuration and secrets, as the host's environment gives them.
    pub env: Vec<(String, String)>,
    /// The `host:port`s the module may send HTTP requests to.
    pub allow_http: Vec<String>,
    /// Stop after this many requests (for tests).
    pub requests: Option<usize>,
}

impl Default for WagiOptions {
    fn default() -> Self {
        Self {
            fuel: 2_000_000_000,
            route_fuel: Vec::new(),
            memory_bytes: 128 * 1024 * 1024,
            response_bytes: 6 * 1024 * 1024,
            deadline: Duration::from_secs(30),
            actor_prefix: None,
            env: Vec::new(),
            allow_http: Vec::new(),
            requests: None,
        }
    }
}

impl WagiOptions {
    /// The fuel `path` may use.
    #[must_use]
    pub fn fuel_for(&self, path: &str) -> u64 {
        self.route_fuel
            .iter()
            .filter(|(prefix, _)| path.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map_or(self.fuel, |(_, fuel)| *fuel)
    }
}

/// The key-value store the host keeps across requests.
pub type Kv = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// Denies memory growth past the ceiling, and remembers it did.
struct Ceiling {
    bytes: usize,
    denied: bool,
}

impl ResourceLimiter for Ceiling {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> std::result::Result<bool, LimiterError> {
        if desired > self.bytes {
            self.denied = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> std::result::Result<bool, LimiterError> {
        Ok(desired <= 1_000_000)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        4
    }

    fn memories(&self) -> usize {
        1
    }
}

/// One request's instance state.
struct State {
    stdin: Vec<u8>,
    stdin_at: usize,
    stdout: Vec<u8>,
    stdout_over: bool,
    response_bytes: usize,
    env: Vec<String>,
    args: Vec<String>,
    kv: Kv,
    allow_http: Arc<Vec<String>>,
    /// The last outbound response, until the module reads it.
    fetched: Vec<u8>,
    ceiling: Ceiling,
    started: Instant,
    deadline: Duration,
}

/// Sends one plain-HTTP request for the module: `{"method", "url",
/// "headers": [[name, value]], "body"}` in, `{"status", "headers", "body"}`
/// out (bodies as text; status 0 when it could not be sent).
fn fetch(request: &[u8], allowed: &[String], timeout: Duration) -> serde_json::Value {
    use std::io::{Read as _, Write as _};
    let failed =
        |message: String| serde_json::json!({ "status": 0, "headers": [], "body": message });
    let Ok(request) = serde_json::from_slice::<serde_json::Value>(request) else {
        return failed("not a request".into());
    };
    let url = request["url"].as_str().unwrap_or_default();
    let Some(rest) = url.strip_prefix("http://") else {
        return failed(format!("only http:// is emulated: {url}"));
    };
    let (authority, path) =
        rest.split_once('/').map_or((rest, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
    if !allowed.iter().any(|host| host == authority) {
        return failed(format!("{authority} is not an allowed host"));
    }
    let Ok(mut stream) = TcpStream::connect(authority) else {
        return failed(format!("could not connect to {authority}"));
    };
    let _ = stream.set_read_timeout(Some(timeout.max(Duration::from_millis(1))));
    let method = request["method"].as_str().unwrap_or("GET");
    let body = request["body"].as_str().unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nhost: {authority}\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for pair in request["headers"].as_array().into_iter().flatten() {
        if let (Some(name), Some(value)) = (pair[0].as_str(), pair[1].as_str()) {
            let _ = write!(head, "{name}: {value}\r\n");
        }
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body.as_bytes())).is_err() {
        return failed("could not send".into());
    }
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(response.len(), |at| at + 4);
    let head = String::from_utf8_lossy(&response[..split]).into_owned();
    let status =
        head.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    let headers: Vec<[String; 2]> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| [name.trim().to_ascii_lowercase(), value.trim().to_owned()])
        .collect();
    let mut body = response[split..].to_vec();
    if headers.iter().any(|[name, value]| name == "transfer-encoding" && value.contains("chunked"))
    {
        body = unchunk(&body);
    }
    serde_json::json!({ "status": status, "headers": headers, "body": String::from_utf8_lossy(&body) })
}

/// A chunked body's bytes.
fn unchunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(line_end) = body.windows(2).position(|window| window == b"\r\n") {
        let size = std::str::from_utf8(&body[..line_end])
            .ok()
            .and_then(|line| {
                usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).ok()
            })
            .unwrap_or(0);
        let start = line_end + 2;
        if size == 0 || start + size > body.len() {
            break;
        }
        out.extend_from_slice(&body[start..start + size]);
        body = &body[(start + size + 2).min(body.len())..];
    }
    out
}

// WASI preview 1 errno values.
const SUCCESS: i32 = 0;
const EBADF: i32 = 8;
const EFAULT: i32 = 21;
const EINVAL: i32 = 28;
const ENOSYS: i32 = 52;
const ESPIPE: i32 = 70;

type Call<'a> = Caller<'a, State>;

fn memory(caller: &Call<'_>) -> Option<Memory> {
    caller.get_export("memory").and_then(Extern::into_memory)
}

fn read_bytes(caller: &Call<'_>, at: i32, len: i32) -> Option<Vec<u8>> {
    let memory = memory(caller)?;
    let mut bytes = vec![0; usize::try_from(len).ok()?];
    memory.read(caller, usize::try_from(at).ok()?, &mut bytes).ok()?;
    Some(bytes)
}

fn write_bytes(caller: &mut Call<'_>, at: i32, bytes: &[u8]) -> bool {
    let Some(memory) = memory(caller) else { return false };
    let Ok(at) = usize::try_from(at) else { return false };
    memory.write(caller, at, bytes).is_ok()
}

fn u32_at(caller: &Call<'_>, at: i32) -> Option<u32> {
    let bytes = read_bytes(caller, at, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// The `(buffer, length)` pairs of an `iovec` array.
fn iovecs(caller: &Call<'_>, at: i32, count: i32) -> Option<Vec<(i32, i32)>> {
    (0..count)
        .map(|index| {
            let entry = at.checked_add(index.checked_mul(8)?)?;
            let buffer = i32::try_from(u32_at(caller, entry)?).ok()?;
            let len = i32::try_from(u32_at(caller, entry + 4)?).ok()?;
            Some((buffer, len))
        })
        .collect()
}

/// Writes strings as WASI passes them: pointers at `pointers`, the
/// NUL-terminated bytes at `buffer`.
fn write_strings(caller: &mut Call<'_>, strings: &[String], pointers: i32, buffer: i32) -> i32 {
    let mut at = buffer;
    for (index, string) in strings.iter().enumerate() {
        let Ok(index) = i32::try_from(index) else { return EFAULT };
        let mut bytes = string.as_bytes().to_vec();
        bytes.push(0);
        if !write_bytes(caller, pointers + index * 4, &at.to_le_bytes())
            || !write_bytes(caller, at, &bytes)
        {
            return EFAULT;
        }
        at += i32::try_from(bytes.len()).unwrap_or(i32::MAX);
    }
    SUCCESS
}

fn write_sizes(caller: &mut Call<'_>, strings: &[String], count: i32, size: i32) -> i32 {
    let total: usize = strings.iter().map(|string| string.len() + 1).sum();
    let count_bytes = u32::try_from(strings.len()).unwrap_or(u32::MAX).to_le_bytes();
    let size_bytes = u32::try_from(total).unwrap_or(u32::MAX).to_le_bytes();
    if write_bytes(caller, count, &count_bytes) && write_bytes(caller, size, &size_bytes) {
        SUCCESS
    } else {
        EFAULT
    }
}

fn now_ns(id: i32, started: Instant) -> u64 {
    if id == 0 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_nanos()).unwrap_or(u64::MAX))
    } else {
        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// `poll_oneoff`: clock subscriptions sleep (bounded by the deadline);
/// standard streams are always ready.
fn poll_oneoff(
    caller: &mut Call<'_>,
    subscriptions: i32,
    events: i32,
    count: i32,
    written: i32,
) -> i32 {
    let started = caller.data().started;
    let mut sleep: Option<Duration> = None;
    let mut ready = Vec::new();
    let mut clocks = Vec::new();
    for index in 0..count {
        let Some(subscription) = read_bytes(caller, subscriptions + index * 48, 48) else {
            return EFAULT;
        };
        let userdata = &subscription[0..8];
        match subscription[8] {
            0 => {
                let id = i32::from_le_bytes(subscription[16..20].try_into().unwrap_or_default());
                let timeout =
                    u64::from_le_bytes(subscription[24..32].try_into().unwrap_or_default());
                let absolute = subscription[40] & 1 == 1;
                let wait =
                    if absolute { timeout.saturating_sub(now_ns(id, started)) } else { timeout };
                let wait = Duration::from_nanos(wait);
                sleep = Some(sleep.map_or(wait, |sleep| sleep.min(wait)));
                clocks.push(userdata.to_vec());
            }
            kind => ready.push((userdata.to_vec(), kind)),
        }
    }
    let mut out = Vec::new();
    if ready.is_empty() {
        let remaining = caller.data().deadline.saturating_sub(started.elapsed());
        std::thread::sleep(sleep.unwrap_or_default().min(remaining));
        for userdata in clocks {
            let mut event = [0u8; 32];
            event[0..8].copy_from_slice(&userdata);
            event[10] = 0;
            out.extend_from_slice(&event);
        }
    } else {
        for (userdata, kind) in ready {
            let mut event = [0u8; 32];
            event[0..8].copy_from_slice(&userdata);
            event[10] = kind;
            event[16..24].copy_from_slice(&1u64.to_le_bytes());
            out.extend_from_slice(&event);
        }
    }
    let events_count = u32::try_from(out.len() / 32).unwrap_or(0);
    if write_bytes(caller, events, &out)
        && write_bytes(caller, written, &events_count.to_le_bytes())
    {
        SUCCESS
    } else {
        EFAULT
    }
}

#[allow(clippy::too_many_lines, reason = "one registration per host function")]
fn linker(engine: &Engine, module: &Module) -> Result<Linker<State>> {
    const WASI: &str = "wasi_snapshot_preview1";
    let mut linker = Linker::<State>::new(engine);
    let defined = |result: std::result::Result<&mut Linker<State>, wasmi::errors::LinkerError>| {
        result.map(|_| ()).map_err(|error| Error::Io {
            what: "define the host's functions".into(),
            cause: std::io::Error::other(error.to_string()),
        })
    };
    defined(linker.func_wrap(
        WASI,
        "args_sizes_get",
        |mut caller: Call<'_>, count: i32, size: i32| {
            let args = caller.data().args.clone();
            write_sizes(&mut caller, &args, count, size)
        },
    ))?;
    defined(linker.func_wrap(
        WASI,
        "args_get",
        |mut caller: Call<'_>, pointers: i32, buffer: i32| {
            let args = caller.data().args.clone();
            write_strings(&mut caller, &args, pointers, buffer)
        },
    ))?;
    defined(linker.func_wrap(
        WASI,
        "environ_sizes_get",
        |mut caller: Call<'_>, count: i32, size: i32| {
            let env = caller.data().env.clone();
            write_sizes(&mut caller, &env, count, size)
        },
    ))?;
    defined(linker.func_wrap(
        WASI,
        "environ_get",
        |mut caller: Call<'_>, pointers: i32, buffer: i32| {
            let env = caller.data().env.clone();
            write_strings(&mut caller, &env, pointers, buffer)
        },
    ))?;
    defined(linker.func_wrap(
        WASI,
        "clock_time_get",
        |mut caller: Call<'_>, id: i32, _precision: i64, at: i32| {
            let now = now_ns(id, caller.data().started);
            if write_bytes(&mut caller, at, &now.to_le_bytes()) { SUCCESS } else { EFAULT }
        },
    ))?;
    defined(linker.func_wrap(WASI, "clock_res_get", |mut caller: Call<'_>, _id: i32, at: i32| {
        if write_bytes(&mut caller, at, &1_000u64.to_le_bytes()) { SUCCESS } else { EFAULT }
    }))?;
    defined(linker.func_wrap(WASI, "random_get", |mut caller: Call<'_>, at: i32, len: i32| {
        let mut bytes = vec![0; usize::try_from(len).unwrap_or(0)];
        if getrandom::getrandom(&mut bytes).is_err() {
            return EINVAL;
        }
        if write_bytes(&mut caller, at, &bytes) { SUCCESS } else { EFAULT }
    }))?;
    defined(linker.func_wrap(
        WASI,
        "fd_write",
        |mut caller: Call<'_>, fd: i32, iovs: i32, count: i32, written: i32| {
            let Some(iovs) = iovecs(&caller, iovs, count) else { return EFAULT };
            let mut total = 0u32;
            for (buffer, len) in iovs {
                let Some(bytes) = read_bytes(&caller, buffer, len) else { return EFAULT };
                total += u32::try_from(bytes.len()).unwrap_or(0);
                match fd {
                    1 => {
                        let state = caller.data_mut();
                        if state.stdout.len() + bytes.len() > state.response_bytes {
                            state.stdout_over = true;
                        } else {
                            state.stdout.extend_from_slice(&bytes);
                        }
                    }
                    2 => eprint!("{}", String::from_utf8_lossy(&bytes)),
                    _ => return EBADF,
                }
            }
            if write_bytes(&mut caller, written, &total.to_le_bytes()) { SUCCESS } else { EFAULT }
        },
    ))?;
    defined(linker.func_wrap(
        WASI,
        "fd_read",
        |mut caller: Call<'_>, fd: i32, iovs: i32, count: i32, read: i32| {
            if fd != 0 {
                return EBADF;
            }
            let Some(iovs) = iovecs(&caller, iovs, count) else { return EFAULT };
            let mut total = 0u32;
            for (buffer, len) in iovs {
                let state = caller.data();
                let end =
                    (state.stdin_at + usize::try_from(len).unwrap_or(0)).min(state.stdin.len());
                let bytes = state.stdin[state.stdin_at..end].to_vec();
                caller.data_mut().stdin_at = end;
                if !write_bytes(&mut caller, buffer, &bytes) {
                    return EFAULT;
                }
                total += u32::try_from(bytes.len()).unwrap_or(0);
                if bytes.is_empty() {
                    break;
                }
            }
            if write_bytes(&mut caller, read, &total.to_le_bytes()) { SUCCESS } else { EFAULT }
        },
    ))?;
    defined(linker.func_wrap(WASI, "fd_fdstat_get", |mut caller: Call<'_>, fd: i32, at: i32| {
        if !(0..=2).contains(&fd) {
            return EBADF;
        }
        let mut stat = [0u8; 24];
        // A character device, with every right.
        stat[0] = 2;
        stat[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        stat[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        if write_bytes(&mut caller, at, &stat) { SUCCESS } else { EFAULT }
    }))?;
    defined(linker.func_wrap(WASI, "fd_fdstat_set_flags", |_: Call<'_>, fd: i32, _flags: i32| {
        if (0..=2).contains(&fd) { SUCCESS } else { EBADF }
    }))?;
    defined(linker.func_wrap(WASI, "fd_prestat_get", |_: Call<'_>, _fd: i32, _at: i32| EBADF))?;
    defined(linker.func_wrap(
        WASI,
        "fd_prestat_dir_name",
        |_: Call<'_>, _: i32, _: i32, _: i32| EBADF,
    ))?;
    defined(linker.func_wrap(WASI, "fd_close", |_: Call<'_>, fd: i32| {
        if (0..=2).contains(&fd) { SUCCESS } else { EBADF }
    }))?;
    defined(
        linker.func_wrap(WASI, "fd_seek", |_: Call<'_>, _: i32, _: i64, _: i32, _: i32| ESPIPE),
    )?;
    defined(linker.func_wrap(WASI, "sched_yield", |_: Call<'_>| SUCCESS))?;
    defined(linker.func_wrap(
        WASI,
        "poll_oneoff",
        |mut caller: Call<'_>, subscriptions: i32, events: i32, count: i32, written: i32| {
            poll_oneoff(&mut caller, subscriptions, events, count, written)
        },
    ))?;
    defined(linker.func_wrap(
        WASI,
        "proc_exit",
        |_: Call<'_>, status: i32| -> std::result::Result<(), wasmi::Error> {
            Err(wasmi::Error::i32_exit(status))
        },
    ))?;

    // The key-value store (`rn_kv`).
    defined(linker.func_wrap(
        "rn_kv",
        "get",
        |mut caller: Call<'_>, key: i32, key_len: i32, buffer: i32, capacity: i32| -> i64 {
            let Some(key) = read_bytes(&caller, key, key_len) else { return -2 };
            let key = String::from_utf8_lossy(&key).into_owned();
            let value =
                caller.data().kv.lock().unwrap_or_else(PoisonError::into_inner).get(&key).cloned();
            let Some(value) = value else { return -1 };
            let fits = usize::try_from(capacity).unwrap_or(0) >= value.len();
            if fits && !write_bytes(&mut caller, buffer, &value) {
                return -2;
            }
            i64::try_from(value.len()).unwrap_or(i64::MAX)
        },
    ))?;
    defined(linker.func_wrap(
        "rn_kv",
        "set",
        |caller: Call<'_>, key: i32, key_len: i32, value: i32, value_len: i32| -> i32 {
            let (Some(key), Some(value)) =
                (read_bytes(&caller, key, key_len), read_bytes(&caller, value, value_len))
            else {
                return EFAULT;
            };
            let key = String::from_utf8_lossy(&key).into_owned();
            caller.data().kv.lock().unwrap_or_else(PoisonError::into_inner).insert(key, value);
            SUCCESS
        },
    ))?;
    defined(linker.func_wrap(
        "rn_kv",
        "delete",
        |caller: Call<'_>, key: i32, key_len: i32| -> i32 {
            let Some(key) = read_bytes(&caller, key, key_len) else { return EFAULT };
            let key = String::from_utf8_lossy(&key).into_owned();
            caller.data().kv.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            SUCCESS
        },
    ))?;
    defined(linker.func_wrap(
        "rn_kv",
        "list",
        |mut caller: Call<'_>, prefix: i32, prefix_len: i32, buffer: i32, capacity: i32| -> i64 {
            let Some(prefix) = read_bytes(&caller, prefix, prefix_len) else { return -2 };
            let prefix = String::from_utf8_lossy(&prefix).into_owned();
            let keys: Vec<String> = caller
                .data()
                .kv
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .range(prefix.clone()..)
                .take_while(|(key, _)| key.starts_with(&prefix))
                .map(|(key, _)| key.clone())
                .collect();
            let listed = serde_json::to_vec(&keys).unwrap_or_default();
            let fits = usize::try_from(capacity).unwrap_or(0) >= listed.len();
            if fits && !write_bytes(&mut caller, buffer, &listed) {
                return -2;
            }
            i64::try_from(listed.len()).unwrap_or(i64::MAX)
        },
    ))?;

    // Outbound HTTP (`rn_http`): `send` performs a request and returns the
    // response's length; `read` copies it out.
    defined(linker.func_wrap(
        "rn_http",
        "send",
        |mut caller: Call<'_>, request: i32, len: i32| -> i64 {
            let Some(request) = read_bytes(&caller, request, len) else { return -2 };
            let state = caller.data();
            let remaining = state.deadline.saturating_sub(state.started.elapsed());
            let response = fetch(&request, &state.allow_http, remaining);
            let bytes = serde_json::to_vec(&response).unwrap_or_default();
            let length = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            caller.data_mut().fetched = bytes;
            length
        },
    ))?;
    defined(linker.func_wrap(
        "rn_http",
        "read",
        |mut caller: Call<'_>, buffer: i32, capacity: i32| -> i64 {
            let fetched = std::mem::take(&mut caller.data_mut().fetched);
            if usize::try_from(capacity).unwrap_or(0) < fetched.len()
                || !write_bytes(&mut caller, buffer, &fetched)
            {
                return -2;
            }
            i64::try_from(fetched.len()).unwrap_or(i64::MAX)
        },
    ))?;

    // Everything else a sandbox does not offer: `ENOSYS`.
    for import in module.imports() {
        if import.module() != WASI {
            continue;
        }
        if let ExternType::Func(ty) = import.ty() {
            let returns_errno = ty.results().len() == 1;
            // Refused (as a duplicate) for the calls defined above.
            let _ = linker.func_new(WASI, import.name(), ty.clone(), move |_, _, results| {
                if returns_errno {
                    results[0] = wasmi::Val::I32(ENOSYS);
                }
                Ok(())
            });
        }
    }
    Ok(linker)
}

/// A request's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The status.
    pub status: u16,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

/// The CGI environment for `request`.
fn cgi_env(
    request: &HttpRequest,
    options: &WagiOptions,
    address: Option<SocketAddr>,
) -> Vec<String> {
    let mut env = vec![
        format!("REQUEST_METHOD={}", request.method),
        format!("PATH_INFO={}", request.path()),
        "SCRIPT_NAME=".to_owned(),
        format!("QUERY_STRING={}", request.query()),
        format!("CONTENT_LENGTH={}", request.body.len()),
        format!("CONTENT_TYPE={}", request.header("content-type").unwrap_or_default()),
        "SERVER_PROTOCOL=HTTP/1.1".to_owned(),
        "GATEWAY_INTERFACE=CGI/1.1".to_owned(),
        format!(
            "SERVER_NAME={}",
            address.map_or_else(|| "localhost".to_owned(), |a| a.ip().to_string())
        ),
        format!("SERVER_PORT={}", address.map_or(0, |a| a.port())),
        format!("RUSTNATIVE_LIMIT_DEADLINE_MS={}", options.deadline.as_millis()),
        format!("RUSTNATIVE_LIMIT_MEMORY_BYTES={}", options.memory_bytes),
        format!("RUSTNATIVE_LIMIT_RESPONSE_BYTES={}", options.response_bytes),
        format!("RUSTNATIVE_LIMIT_FUEL={}", options.fuel_for(request.path())),
    ];
    for (name, value) in &request.headers {
        if name == "content-type" || name == "content-length" {
            continue;
        }
        env.push(format!("HTTP_{}={value}", name.to_ascii_uppercase().replace('-', "_")));
    }
    env.extend(options.env.iter().map(|(name, value)| format!("{name}={value}")));
    env
}

/// The response a CGI program wrote.
fn parse_cgi(output: &[u8]) -> Outcome {
    let split = output
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| (at, 4))
        .or_else(|| output.windows(2).position(|window| window == b"\n\n").map(|at| (at, 2)));
    let Some((at, gap)) = split else {
        return Outcome {
            status: 502,
            headers: Vec::new(),
            body: b"the module wrote no CGI headers".to_vec(),
        };
    };
    let head = String::from_utf8_lossy(&output[..at]);
    let mut status = None;
    let mut headers = Vec::new();
    for line in head.lines() {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_owned());
        if name == "status" {
            status = value.split_whitespace().next().and_then(|code| code.parse().ok());
        } else {
            headers.push((name, value));
        }
    }
    let located = headers.iter().any(|(name, _)| name == "location");
    let status = status.unwrap_or(if located { 302 } else { 200 });
    Outcome { status, headers, body: output[at + gap..].to_vec() }
}

fn limit(status: u16, which: &str, message: &str, fuel: u64) -> Outcome {
    Outcome {
        status,
        headers: vec![
            ("content-type".into(), "text/plain; charset=utf-8".into()),
            ("x-rn-limit".into(), which.into()),
            ("x-rn-fuel".into(), fuel.to_string()),
        ],
        body: message.as_bytes().to_vec(),
    }
}

/// A module, compiled once, answering requests.
pub struct Host {
    engine: Engine,
    module: Module,
    linker: Linker<State>,
    options: WagiOptions,
    kv: Kv,
    actors: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Host {
    /// Compiles the module at `path`.
    ///
    /// # Errors
    ///
    /// It could not be read, or is not a module the host can run.
    pub fn new(path: &Path, options: WagiOptions) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(io(format!("read {}", path.display())))?;
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, &bytes).map_err(|error| Error::Io {
            what: format!("compile {}", path.display()),
            cause: std::io::Error::other(error.to_string()),
        })?;
        let linker = linker(&engine, &module)?;
        Ok(Self { engine, module, linker, options, kv: Kv::default(), actors: Mutex::default() })
    }

    /// The actor a request is for, when it is for one.
    fn actor(&self, path: &str) -> Option<String> {
        let rest = path.strip_prefix(self.options.actor_prefix.as_deref()?)?;
        rest.split('/').find(|segment| !segment.is_empty()).map(str::to_owned)
    }

    /// Answers `request` with a fresh instance of the module.
    #[must_use]
    pub fn answer(&self, request: &HttpRequest, address: Option<SocketAddr>) -> Outcome {
        let lock = self.actor(request.path()).map(|actor| {
            Arc::clone(
                self.actors
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .entry(actor)
                    .or_default(),
            )
        });
        let _turn = lock.as_ref().map(|lock| lock.lock().unwrap_or_else(PoisonError::into_inner));
        let fuel = self.options.fuel_for(request.path());
        let state = State {
            stdin: request.body.clone(),
            stdin_at: 0,
            stdout: Vec::new(),
            stdout_over: false,
            response_bytes: usize::try_from(self.options.response_bytes).unwrap_or(usize::MAX),
            env: cgi_env(request, &self.options, address),
            args: vec!["module.wasm".into()],
            kv: Arc::clone(&self.kv),
            allow_http: Arc::new(self.options.allow_http.clone()),
            fetched: Vec::new(),
            ceiling: Ceiling {
                bytes: usize::try_from(self.options.memory_bytes).unwrap_or(usize::MAX),
                denied: false,
            },
            started: Instant::now(),
            deadline: self.options.deadline,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.ceiling);
        let _ = store.set_fuel(fuel);
        let run = self
            .linker
            .instantiate(&mut store, &self.module)
            .and_then(|pre| pre.start(&mut store))
            .and_then(|instance| {
                instance.get_typed_func::<(), ()>(&store, "_start")?.call(&mut store, ())
            });
        let used = fuel.saturating_sub(store.get_fuel().unwrap_or(0));
        let state = store.data();
        if state.ceiling.denied {
            return limit(503, "memory", "the request grew past the host's memory ceiling", used);
        }
        match run {
            Ok(()) => {}
            Err(error) if error.i32_exit_status() == Some(0) => {}
            Err(error) if error.as_trap_code() == Some(TrapCode::OutOfFuel) => {
                return limit(503, "fuel", "the request ran past its route's CPU budget", used);
            }
            Err(error) => {
                eprintln!("wagi: {} {}: {error}", request.method, request.target);
                return limit(500, "trap", "the module failed", used);
            }
        }
        if state.stdout_over {
            return limit(502, "response", "the response is larger than the host allows", used);
        }
        let mut outcome = parse_cgi(&state.stdout);
        outcome.headers.push(("x-rn-fuel".into(), used.to_string()));
        outcome
    }
}

/// `rustnative serve wagi`: serves `module` at `address`.
///
/// # Errors
///
/// The module could not be compiled, or the address bound.
pub fn serve(module: &Path, address: SocketAddr, options: WagiOptions) -> Result<()> {
    let listener = TcpListener::bind(address).map_err(io(format!("listen on {address}")))?;
    println!("serve wagi: http://{}", listener.local_addr().unwrap_or(address));
    serve_on(&listener, module, options)
}

/// [`serve`] on an existing listener.
///
/// # Errors
///
/// The module could not be compiled.
pub fn serve_on(listener: &TcpListener, module: &Path, options: WagiOptions) -> Result<()> {
    let requests = options.requests;
    let host = Arc::new(Host::new(module, options)?);
    let address = listener.local_addr().ok();
    let mut served = 0;
    let mut workers = Vec::new();
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let host = Arc::clone(&host);
        workers.push(std::thread::spawn(move || answer_stream(stream, &host, address)));
        served += 1;
        if requests.is_some_and(|limit| served >= limit) {
            break;
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

fn answer_stream(mut stream: TcpStream, host: &Host, address: Option<SocketAddr>) {
    let Some(request) = read_request(&mut stream) else { return };
    if u64::try_from(request.body.len()).unwrap_or(u64::MAX) > host.options.response_bytes {
        write_error(&mut stream, 413, "the request is larger than the host allows");
        return;
    }
    let outcome = host.answer(&request, address);
    write_response(&mut stream, outcome.status, &outcome.headers, &outcome.body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgi_output_becomes_a_response() {
        let outcome = parse_cgi(b"Status: 404 Not Found\r\ncontent-type: text/plain\r\n\r\nnope");
        assert_eq!(outcome.status, 404);
        assert_eq!(outcome.body, b"nope");
        assert_eq!(parse_cgi(b"Location: /x\n\n").status, 302);
    }

    #[test]
    fn the_longest_route_prefix_sets_the_budget() {
        let options = WagiOptions {
            fuel: 10,
            route_fuel: vec![("/".into(), 20), ("/reports".into(), 30)],
            ..WagiOptions::default()
        };
        assert_eq!(options.fuel_for("/reports/q3"), 30);
        assert_eq!(options.fuel_for("/notes"), 20);
    }
}
