//! The portable service contracts (`rustnative_core::Services`) on Android.
//!
//! | Service | Android |
//! |---|---|
//! | [`AndroidClipboard`] | `ClipboardManager` (Android lets only the focused application read it) |
//! | [`AndroidFileDialogs`] | the Storage Access Framework: `ACTION_OPEN_DOCUMENT`, `ACTION_CREATE_DOCUMENT`, `ACTION_OPEN_DOCUMENT_TREE`; the answer is a `content://` URI, read and written with [`read_document`] and [`write_document`] |
//! | [`AndroidSystem`] | `ACTION_VIEW` for URLs; notifications on a channel, with actions returning as `Event::SurfaceAction`; the share sheet ([`AndroidSystem::share`]) |
//! | [`AndroidHttp`] | `HttpsURLConnection` with the system trust store and the application's network security config; pins checked after the handshake, before a byte is written |
//! | [`AndroidPrinting`] | a PDF (`PdfDocument`) written to a file, or handed to `PrintManager`'s print UI |
//! | [`AndroidSerial`] | unavailable, saying why |
//! | [`AndroidLocale`] | ICU (`android.icu`): `NumberFormat`, `DateFormat`, `Collator`, `UCharacter` |
//! | [`AndroidSecureStorage`] | an AES-GCM key in the Android Keystore (`StrongBox` where the device has one) sealing values in the application's private files |
//! | [`AndroidPermissions`] | runtime permissions in the five portable states |
//! | [`AndroidPush`] | Firebase Cloud Messaging when the application ships it, else unavailable |
//! | [`AndroidStore`] | unavailable, saying why (Play Billing's bridge is owed) |
//! | [`AndroidConditions`] | `ConnectivityManager` and `BatteryManager` |
//! | [`BitmapDecoder`] | `BitmapFactory`, subsampled then scaled to the requested size |
//! | [`AndroidWork`] | `JobScheduler`: constrained jobs that run even when the application does not |
//! | [`FileStateStore`] | crash-safe files under `getFilesDir()/rustnative/state` |
//!
//! A service is `Send + Sync` and may be called from any thread: JNI
//! attaches the calling thread, and the Java half posts to the main thread
//! what must run there. Blocking work (HTTP, the Keystore, Play services)
//! runs on a thread of its own, awaited on a channel. An answer that comes
//! from an activity — a picked document, a permission prompt — arrives as
//! an activity or permissions result with a request code this module gave
//! out, and resolves the waiting future ([`pending`]).

mod state_store;

pub use state_store::FileStateStore;

#[cfg(target_os = "android")]
mod data;
#[cfg(target_os = "android")]
mod http;
#[cfg(target_os = "android")]
mod industrial;
#[cfg(target_os = "android")]
mod locale;
#[cfg(target_os = "android")]
mod permissions;
#[cfg(target_os = "android")]
mod product;
#[cfg(target_os = "android")]
mod secure_storage;
#[cfg(target_os = "android")]
mod system;
#[cfg(target_os = "android")]
mod work;

#[cfg(target_os = "android")]
pub use data::{AndroidConditions, BitmapDecoder};
#[cfg(target_os = "android")]
pub use http::AndroidHttp;
#[cfg(target_os = "android")]
pub use industrial::{AndroidPrinting, AndroidSerial};
#[cfg(target_os = "android")]
pub use locale::AndroidLocale;
#[cfg(target_os = "android")]
pub use permissions::AndroidPermissions;
#[cfg(target_os = "android")]
pub use product::{AndroidPush, AndroidStore};
#[cfg(target_os = "android")]
pub use secure_storage::AndroidSecureStorage;
#[cfg(target_os = "android")]
pub use system::{
    AndroidClipboard, AndroidFileDialogs, AndroidSystem, read_document, write_document,
};
#[cfg(all(target_os = "android", feature = "device-tests"))]
pub(crate) use work::job_id;
#[cfg(target_os = "android")]
pub(crate) use work::run as run_job;
#[cfg(target_os = "android")]
pub use work::{AndroidWork, JobHandler};

#[cfg(target_os = "android")]
pub(crate) use plumbing::*;

#[cfg(target_os = "android")]
mod plumbing {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI32, Ordering};

    use rustnative_core::ServiceError;

    use crate::jni_host::{Arg, Class, JavaRef, Ret, call_static};

    /// `RnServices.REQUEST_BASE`: request codes are this plus a token.
    const REQUEST_BASE: i32 = 0x5200;
    /// Tokens wrap below this, so a request code stays a 16-bit one.
    const TOKENS: i32 = 0x1000;

    /// What an activity answered.
    #[derive(Debug)]
    pub(crate) enum Answer {
        /// An activity result: whether it was `RESULT_OK`, and the URI it chose.
        Chosen(Option<String>),
        /// A permission prompt was answered.
        Answered,
    }

    static PENDING: Mutex<Option<HashMap<i32, tokio::sync::oneshot::Sender<Answer>>>> =
        Mutex::new(None);
    static NEXT: AtomicI32 = AtomicI32::new(0);

    /// A token for an answer to come, and where it arrives.
    pub(crate) fn pending() -> (i32, tokio::sync::oneshot::Receiver<Answer>) {
        let token = NEXT.fetch_add(1, Ordering::Relaxed).rem_euclid(TOKENS);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        with_pending(|pending| pending.insert(token, sender));
        (token, receiver)
    }

    /// Forgets a token whose request never started.
    pub(crate) fn abandon(token: i32) {
        with_pending(|pending| pending.remove(&token));
    }

    fn with_pending<R>(
        f: impl FnOnce(&mut HashMap<i32, tokio::sync::oneshot::Sender<Answer>>) -> R,
    ) -> R {
        let mut pending = PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(pending.get_or_insert_with(HashMap::new))
    }

    fn resolve(request: i32, answer: Answer) {
        let token = request - REQUEST_BASE;
        if !(0..TOKENS).contains(&token) {
            return;
        }
        if let Some(sender) = with_pending(|pending| pending.remove(&token)) {
            let _ = sender.send(answer);
        }
    }

    /// An activity result arrived (a document chosen).
    pub(crate) fn activity_result(request: i32, result: i32, data: Option<JavaRef>) {
        // RESULT_OK is -1.
        let uri = if result == -1 {
            data.and_then(|data| {
                call_static(
                    Class::Services,
                    "resultUri",
                    "(Landroid/content/Intent;)Ljava/lang/String;",
                    &[Arg::Obj(&data)],
                )
                .ok()
                .and_then(Ret::string)
            })
        } else {
            None
        };
        resolve(request, Answer::Chosen(uri));
    }

    /// A permission request was answered.
    pub(crate) fn permissions_answered(request: i32, _permissions: &[String], _results: &[i32]) {
        resolve(request, Answer::Answered);
    }

    /// Java answered work a service started (unused: services await their
    /// own threads).
    pub(crate) fn reply(_token: i64, _status: i32, _text: Option<String>, _bytes: Vec<u8>) {}

    /// A service's timer fired (none schedules one).
    #[allow(clippy::unnecessary_wraps, reason = "the timer contract can fail")]
    pub(crate) const fn timer_fired(
        _registry: &mut crate::registry::WindowRegistry,
        _token: i64,
    ) -> Result<(), crate::Error> {
        Ok(())
    }

    /// The application context is recorded on the Java side
    /// (`RnServices.init`).
    pub(crate) fn set_context(_context: JavaRef) {}

    /// The application's private files directory.
    pub(crate) fn files_dir() -> Result<PathBuf, ServiceError> {
        java(Class::Services, "filesDir", "()Ljava/lang/String;", &[])?
            .string()
            .map(PathBuf::from)
            .ok_or_else(|| ServiceError::new("the application has no files directory yet"))
    }

    /// The application's cache directory.
    pub(crate) fn cache_dir() -> Result<PathBuf, ServiceError> {
        java(Class::Services, "cacheDir", "()Ljava/lang/String;", &[])?
            .string()
            .map(PathBuf::from)
            .ok_or_else(|| ServiceError::new("the application has no cache directory yet"))
    }

    /// Calls a static Java method, its failure as a service error.
    pub(crate) fn java(
        class: Class,
        name: &str,
        signature: &str,
        args: &[Arg<'_>],
    ) -> Result<Ret, ServiceError> {
        call_static(class, name, signature, args)
            .map_err(|error| ServiceError::new(error.to_string()))
    }

    /// A Java method's error message (a non-null `String` result) as an error.
    pub(crate) fn checked(ret: Ret) -> Result<(), ServiceError> {
        ret.string().map_or(Ok(()), |message| Err(ServiceError::new(message)))
    }

    /// Runs blocking `work` on a thread of its own and awaits it.
    pub(crate) async fn off_thread<R: Send + 'static>(
        work: impl FnOnce() -> R + Send + 'static,
    ) -> Result<R, ServiceError> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("rustnative-service".into())
            .spawn(move || {
                let _ = reply.send(work());
            })
            .map_err(|error| {
                ServiceError::new(format!("could not start a service thread: {error}"))
            })?;
        answer.await.map_err(|_| ServiceError::new("the service thread ended without answering"))
    }
}
