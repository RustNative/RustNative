//! The terminal's services (`PLAN.md` Milestone 38: "`Clipboard` only where
//! OSC 52 is available, `Notifications` only where the terminal implements
//! them").
//!
//! A terminal application's services are the terminal's, plus what the
//! operating system provides without a GUI toolkit:
//!
//! | Service | Realized by |
//! |---|---|
//! | clipboard | OSC 52 (write; read where the terminal answers), and always a copy kept in the process so copying and pasting inside the application works anywhere |
//! | notifications | OSC 9, 99, or 777 — whichever the terminal implements — and nothing elsewhere |
//! | URLs | the operating system's opener, in a local session only (over SSH it would open on the far machine) |
//! | persisted state | crash-safe files (`rustnative_core::services::FileStateStore`) under `$XDG_STATE_HOME/<id>/state` or `%LOCALAPPDATA%\<id>\state` |
//! | documents | files (`FileDocuments`) |
//!
//! Every write to the terminal goes through the loop ([`Channel`]): a
//! service queues its bytes and wakes the loop, which writes them between
//! frames, so nothing interleaves with a frame being written.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker as TaskWaker};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustnative_core::{ClipboardService, ServiceError, Services, SystemService, UrlPolicy};

use crate::decode::base64_encode;
use crate::probe::{ClipboardSupport, NotificationProtocol, TerminalProfile};
use crate::term::Waker;

/// How long a clipboard read waits for the terminal to answer.
pub const CLIPBOARD_READ_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
#[allow(
    clippy::option_option,
    reason = "outer: whether the read has ended; inner: what the clipboard held"
)]
struct ReadSlot {
    result: Option<Option<String>>,
    waker: Option<TaskWaker>,
    deadline: Option<Instant>,
}

/// What the loop and the services share.
#[derive(Default)]
pub struct Channel {
    outbox: Mutex<Vec<u8>>,
    profile: Mutex<Option<TerminalProfile>>,
    waker: Mutex<Option<Waker>>,
    local: Mutex<Option<String>>,
    reads: Mutex<Vec<Arc<Mutex<ReadSlot>>>>,
    notifications: Mutex<Vec<(String, String)>>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel").field("queued", &self.outbox.lock().len()).finish_non_exhaustive()
    }
}

impl Channel {
    /// A channel with no terminal yet.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn set_waker(&self, waker: Waker) {
        *self.waker.lock() = Some(waker);
    }

    pub(crate) fn set_profile(&self, profile: TerminalProfile) {
        *self.profile.lock() = Some(profile);
    }

    fn profile(&self) -> Option<TerminalProfile> {
        self.profile.lock().clone()
    }

    fn send(&self, bytes: &[u8]) {
        self.outbox.lock().extend_from_slice(bytes);
        let waker = self.waker.lock().clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    /// Takes the bytes the services queued.
    pub(crate) fn take_outbox(&self) -> Vec<u8> {
        std::mem::take(&mut *self.outbox.lock())
    }

    /// The text copied last, inside the process.
    #[must_use]
    pub fn local_clipboard(&self) -> Option<String> {
        self.local.lock().clone()
    }

    pub(crate) fn set_local_clipboard(&self, text: &str) {
        *self.local.lock() = Some(text.to_owned());
    }

    /// Copies `text`: kept in the process, and sent with OSC 52 where the
    /// terminal takes it. Returns whether it reached the terminal.
    pub fn copy(&self, text: &str) -> bool {
        self.set_local_clipboard(text);
        let supported =
            self.profile().is_some_and(|profile| profile.clipboard != ClipboardSupport::None);
        if supported {
            self.send(format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes())).as_bytes());
        }
        supported
    }

    /// A clipboard reply arrived: every waiting read gets it.
    pub(crate) fn clipboard_reply(&self, text: &str) {
        self.set_local_clipboard(text);
        for slot in self.reads.lock().drain(..) {
            let mut slot = slot.lock();
            slot.result = Some(Some(text.to_owned()));
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }

    /// Ends waiting reads whose deadline passed, with the in-process copy.
    pub(crate) fn expire_reads(&self, now: Instant) {
        let local = self.local_clipboard();
        self.reads.lock().retain(|slot| {
            let mut slot = slot.lock();
            if slot.deadline.is_some_and(|deadline| now >= deadline) {
                slot.result = Some(local.clone());
                if let Some(waker) = slot.waker.take() {
                    waker.wake();
                }
                false
            } else {
                true
            }
        });
    }

    /// Whether a clipboard read is waiting (the loop then wakes at its
    /// deadline).
    pub(crate) fn next_read_deadline(&self) -> Option<Instant> {
        self.reads.lock().iter().filter_map(|slot| slot.lock().deadline).min()
    }

    /// Notifications posted, as (title, body) — what a test reads.
    #[must_use]
    pub fn notifications(&self) -> Vec<(String, String)> {
        self.notifications.lock().clone()
    }
}

struct ClipboardRead(Arc<Mutex<ReadSlot>>);

impl Future for ClipboardRead {
    type Output = Option<String>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut slot = self.0.lock();
        if let Some(result) = slot.result.take() {
            Poll::Ready(result)
        } else {
            slot.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

/// The clipboard through OSC 52.
#[derive(Debug, Clone)]
pub struct Osc52Clipboard {
    channel: Arc<Channel>,
}

impl Osc52Clipboard {
    /// A clipboard over `channel`.
    #[must_use]
    pub const fn new(channel: Arc<Channel>) -> Self {
        Self { channel }
    }
}

#[async_trait::async_trait]
impl ClipboardService for Osc52Clipboard {
    async fn read_text(&self) -> Result<Option<String>, ServiceError> {
        let support =
            self.channel.profile().map_or(ClipboardSupport::None, |profile| profile.clipboard);
        if support == ClipboardSupport::None {
            return Ok(self.channel.local_clipboard());
        }
        let slot = Arc::new(Mutex::new(ReadSlot {
            deadline: Some(Instant::now() + CLIPBOARD_READ_TIMEOUT),
            ..ReadSlot::default()
        }));
        self.channel.reads.lock().push(Arc::clone(&slot));
        self.channel.send(b"\x1b]52;c;?\x07");
        Ok(ClipboardRead(slot).await)
    }

    async fn write_text(&self, value: String) -> Result<(), ServiceError> {
        if self.channel.copy(&value) {
            Ok(())
        } else {
            Err(ServiceError::new(
                "this terminal is not known to take OSC 52, so the text was kept only inside the application",
            ))
        }
    }
}

/// Notifications and URL launch for a terminal application.
#[derive(Debug, Clone)]
pub struct TerminalSystem {
    channel: Arc<Channel>,
    name: String,
    url_policy: UrlPolicy,
}

impl TerminalSystem {
    /// The system service over `channel`, posting notifications as `name`
    /// and opening the URLs [`UrlPolicy::default`] allows.
    #[must_use]
    pub fn new(channel: Arc<Channel>, name: impl Into<String>) -> Self {
        Self { channel, name: name.into(), url_policy: UrlPolicy::default() }
    }

    /// This service, opening the URLs `policy` allows.
    #[must_use]
    pub fn with_url_policy(mut self, policy: UrlPolicy) -> Self {
        self.url_policy = policy;
        self
    }
}

fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() && *c != ';').collect()
}

/// How URLs are opened from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlOpener {
    /// `ShellExecuteW` (Windows): the URL's registered handler, with no
    /// command interpreter in between.
    ShellExecute,
    /// A program given the URL as its one argument (`open`, `xdg-open`).
    Program(&'static str),
}

/// Whether URLs can be opened from here, and how. Never over SSH (it would
/// open on the far machine), and on Linux only with a desktop session.
#[must_use]
pub fn url_opener(remote: bool) -> Option<UrlOpener> {
    if remote {
        return None;
    }
    if cfg!(windows) {
        Some(UrlOpener::ShellExecute)
    } else if cfg!(target_os = "macos") {
        Some(UrlOpener::Program("open"))
    } else if std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        Some(UrlOpener::Program("xdg-open"))
    } else {
        None
    }
}

#[cfg(windows)]
fn shell_execute(url: &str) -> Result<(), ServiceError> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (operation, file) = (wide("open"), wide(url));
    // SAFETY: NUL-terminated wide strings; no window, parameters, or
    // directory.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 are success (the documented convention).
    if result as isize > 32 {
        Ok(())
    } else {
        Err(ServiceError::new(format!(
            "ShellExecute could not open the URL (code {})",
            result as isize
        )))
    }
}

#[cfg(not(windows))]
fn shell_execute(_url: &str) -> Result<(), ServiceError> {
    Err(ServiceError::new("ShellExecute is a Windows facility"))
}

#[async_trait::async_trait]
impl SystemService for TerminalSystem {
    async fn open_url(&self, url: String) -> Result<(), ServiceError> {
        let url = self.url_policy.check(&url)?;
        let remote = self.channel.profile().is_some_and(|profile| profile.remote);
        let Some(opener) = url_opener(remote) else {
            return Err(ServiceError::new(if remote {
                "the session runs over SSH: a URL opened here would open on the far machine"
            } else {
                "there is no desktop session to open a URL in"
            }));
        };
        match opener {
            UrlOpener::ShellExecute => shell_execute(url),
            UrlOpener::Program(program) => std::process::Command::new(program)
                .arg(url)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map(drop)
                .map_err(|error| {
                    ServiceError::new(format!("{program} could not be started: {error}"))
                }),
        }
    }

    async fn notify(&self, title: String, body: String) -> Result<(), ServiceError> {
        let protocol = self.channel.profile().and_then(|profile| profile.notifications);
        let (title, body) = (clean(&title), clean(&body));
        let sequence = match protocol {
            Some(NotificationProtocol::Osc9) => format!("\x1b]9;{title}: {body}\x07"),
            Some(NotificationProtocol::Osc777) => format!("\x1b]777;notify;{title};{body}\x07"),
            Some(NotificationProtocol::Osc99) => {
                format!("\x1b]99;i=1:d=0;{title}\x1b\\\x1b]99;i=1:p=body;{body}\x1b\\")
            }
            None => {
                return Err(ServiceError::new(format!(
                    "this terminal implements no notification sequence, so `{}` cannot notify here",
                    self.name
                )));
            }
        };
        self.channel.notifications.lock().push((title, body));
        self.channel.send(sequence.as_bytes());
        Ok(())
    }
}

/// Where application `app_id` keeps its state: `$XDG_STATE_HOME/<id>/state`
/// (`~/.local/state/<id>/state`) on Unix, `%LOCALAPPDATA%\<id>\state` on
/// Windows — the same folders the desktop backends use, so a binary that is
/// both a GUI and a terminal application keeps one state.
///
/// # Errors
///
/// `app_id` is not a usable folder name, or no base folder is known.
pub fn state_directory(app_id: &str) -> Result<PathBuf, ServiceError> {
    let valid = !app_id.is_empty()
        && app_id != "."
        && app_id != ".."
        && !app_id.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        });
    if !valid {
        return Err(ServiceError::new(format!("`{app_id}` is not a usable application id")));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
    };
    base.map(|base| base.join(app_id).join("state")).ok_or_else(|| {
        ServiceError::new("no folder for state: neither the state home nor the home folder is set")
    })
}

/// Every service a terminal application has, for `app_id`, over `channel`
/// (pass the same channel to `TuiPlatform::with_channel`).
///
/// # Errors
///
/// The state folder could not be created.
pub fn services(
    app_id: &str,
    display_name: &str,
    channel: &Arc<Channel>,
) -> Result<Services, ServiceError> {
    let store = rustnative_core::services::FileStateStore::in_directory(state_directory(app_id)?)?;
    Ok(Services::default()
        .with_clipboard(Arc::new(Osc52Clipboard::new(Arc::clone(channel))))
        .with_system(Arc::new(TerminalSystem::new(Arc::clone(channel), display_name)))
        .with_documents(Arc::new(rustnative_core::services::FileDocuments))
        .with_state_store(Arc::new(store)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::DetectInput;

    fn channel(
        clipboard: ClipboardSupport,
        notifications: Option<NotificationProtocol>,
    ) -> Arc<Channel> {
        let channel = Channel::new();
        let mut profile = TerminalProfile::from_environment(&DetectInput::default(), "test".into());
        profile.clipboard = clipboard;
        profile.notifications = notifications;
        channel.set_profile(profile);
        channel
    }

    #[test]
    fn copying_sends_osc_52_only_where_it_is_taken() {
        let supported = channel(ClipboardSupport::Write, None);
        assert!(supported.copy("hi"));
        assert_eq!(supported.take_outbox(), b"\x1b]52;c;aGk=\x07");
        let unsupported = channel(ClipboardSupport::None, None);
        assert!(!unsupported.copy("hi"));
        assert_eq!(unsupported.take_outbox(), b"");
        assert_eq!(unsupported.local_clipboard().as_deref(), Some("hi"), "still pasteable inside");
    }

    #[test]
    fn notifications_use_the_terminal_s_own_sequence() {
        let kitty = channel(ClipboardSupport::None, Some(NotificationProtocol::Osc777));
        let system = TerminalSystem::new(Arc::clone(&kitty), "App");
        let posted = futures_lite_block_on(system.notify("Done".into(), "it; worked".into()));
        assert!(posted.is_ok());
        assert_eq!(kitty.take_outbox(), b"\x1b]777;notify;Done;it worked\x07");
        let none = channel(ClipboardSupport::None, None);
        assert!(
            futures_lite_block_on(TerminalSystem::new(none, "App").notify("a".into(), "b".into()))
                .is_err()
        );
    }

    #[test]
    fn a_read_ends_with_the_reply_or_the_local_copy() {
        let terminal = channel(ClipboardSupport::Write, None);
        terminal.set_local_clipboard("local");
        let clipboard = Osc52Clipboard::new(Arc::clone(&terminal));
        let mut read = Box::pin(clipboard.read_text());
        assert!(poll_once(read.as_mut()).is_none());
        assert_eq!(terminal.take_outbox(), b"\x1b]52;c;?\x07");
        terminal.clipboard_reply("from the terminal");
        assert_eq!(poll_once(read.as_mut()), Some(Ok(Some("from the terminal".to_owned()))));
        let mut read = Box::pin(clipboard.read_text());
        assert!(poll_once(read.as_mut()).is_none());
        terminal.expire_reads(Instant::now() + CLIPBOARD_READ_TIMEOUT * 2);
        assert_eq!(poll_once(read.as_mut()), Some(Ok(Some("from the terminal".to_owned()))));
    }

    #[test]
    fn a_url_the_policy_refuses_never_reaches_an_opener() {
        // Over SSH nothing is ever launched, so the error says which check
        // answered: the policy's, or the opener's.
        let remote = channel(ClipboardSupport::None, None);
        let mut profile = remote.profile().unwrap();
        profile.remote = true;
        remote.set_profile(profile);
        for refused in ["file:///etc/passwd", r"\\host\share\x", "mailt€x", "http😀"] {
            let system = TerminalSystem::new(Arc::clone(&remote), "App");
            let error = futures_lite_block_on(system.open_url(refused.into())).unwrap_err();
            assert!(!error.to_string().contains("SSH"), "{refused:?}: {error}");
        }
        let system = TerminalSystem::new(remote, "App")
            .with_url_policy(UrlPolicy::default().allow_scheme("notes").unwrap());
        let error = futures_lite_block_on(system.open_url("notes://items/7".into())).unwrap_err();
        assert!(error.to_string().contains("SSH"), "{error}");
    }

    #[test]
    fn state_lives_where_the_desktop_backends_keep_it() {
        let path = state_directory("com.example.tui").unwrap();
        assert!(path.ends_with(std::path::Path::new("com.example.tui").join("state")));
        assert!(state_directory("../escape").is_err());
    }

    fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Option<F::Output> {
        let waker = TaskWaker::noop();
        match future.poll(&mut Context::from_waker(waker)) {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
    }

    fn futures_lite_block_on<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        loop {
            if let Some(value) = poll_once(future.as_mut()) {
                return value;
            }
        }
    }
}
