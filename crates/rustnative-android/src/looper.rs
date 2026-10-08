//! Work handed to the main thread from any thread, through the main
//! `Looper` (`PLAN.md` Milestone 35: "scheduler wake-ups through the main
//! `Looper`").
//!
//! A pipe's read end is registered with the main thread's `ALooper`; any
//! thread queues a closure and writes one byte, and the looper runs every
//! queued closure on the main thread between its other messages. No JNI is
//! involved, so a Tokio worker waking a window's scheduler never attaches to
//! the JVM.

use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread::ThreadId;

type Task = Box<dyn FnOnce() + Send>;

static QUEUE: Mutex<VecDeque<Task>> = Mutex::new(VecDeque::new());
static WRITE_END: AtomicI32 = AtomicI32::new(-1);
static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

#[repr(C)]
struct ALooper {
    _private: [u8; 0],
}

type Callback = unsafe extern "C" fn(fd: c_int, events: c_int, data: *mut c_void) -> c_int;

#[link(name = "android")]
unsafe extern "C" {
    fn ALooper_forThread() -> *mut ALooper;
    fn ALooper_acquire(looper: *mut ALooper);
    fn ALooper_addFd(
        looper: *mut ALooper,
        fd: c_int,
        ident: c_int,
        events: c_int,
        callback: Option<Callback>,
        data: *mut c_void,
    ) -> c_int;
}

const ALOOPER_POLL_CALLBACK: c_int = -2;
const ALOOPER_EVENT_INPUT: c_int = 1;

/// Registers the wake pipe with this thread's looper. Called once, on the
/// main thread, from the first entry Java makes into Rust.
pub(crate) fn install() {
    if MAIN_THREAD.get().is_some() {
        return;
    }
    let _ = MAIN_THREAD.set(std::thread::current().id());
    let mut ends = [0 as c_int; 2];
    // SAFETY: `ends` is two writable ints, as `pipe2` requires.
    if unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
        crate::log::error("the main-thread wake pipe could not be created");
        return;
    }
    // SAFETY: called on a thread with a looper (the main thread); the
    // looper is acquired so it outlives the registration, and the callback
    // is a `'static` function with no data.
    let added = unsafe {
        let looper = ALooper_forThread();
        if looper.is_null() {
            crate::log::error("the main thread has no looper");
            return;
        }
        ALooper_acquire(looper);
        ALooper_addFd(
            looper,
            ends[0],
            ALOOPER_POLL_CALLBACK,
            ALOOPER_EVENT_INPUT,
            Some(woken),
            std::ptr::null_mut(),
        )
    };
    if added != 1 {
        crate::log::error("the wake pipe could not be registered with the main looper");
        return;
    }
    WRITE_END.store(ends[1], Ordering::Release);
}

#[cfg(feature = "device-tests")]
/// Whether this is the main thread (the one the looper runs on).
pub(crate) fn is_main_thread() -> bool {
    MAIN_THREAD.get().is_some_and(|main| *main == std::thread::current().id())
}

/// Runs `task` on the main thread, soon: from the main thread itself, after
/// the work running now.
pub(crate) fn post(task: impl FnOnce() + Send + 'static) {
    QUEUE.lock().unwrap_or_else(PoisonError::into_inner).push_back(Box::new(task));
    let fd = WRITE_END.load(Ordering::Acquire);
    if fd >= 0 {
        let byte = 1u8;
        // SAFETY: `fd` is the pipe's write end, open for the process; a
        // full pipe (EAGAIN) means a wake is already pending, which is
        // enough.
        unsafe {
            libc::write(fd, std::ptr::from_ref(&byte).cast(), 1);
        }
    }
}

#[cfg(feature = "device-tests")]
/// Runs `task` on the main thread and waits for its result — directly when
/// this is the main thread.
///
/// # Panics
///
/// Re-raises a panic `task` raised.
pub(crate) fn on_main<R: Send + 'static>(task: impl FnOnce() -> R + Send + 'static) -> R {
    if is_main_thread() {
        return task();
    }
    let (reply, answer) = std::sync::mpsc::channel();
    post(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
        let _ = reply.send(outcome);
    });
    match answer.recv() {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        Err(std::sync::mpsc::RecvError) => {
            panic!("the main thread's looper stopped before running the task")
        }
    }
}

/// Runs every queued task now, on this (the main) thread — what the device
/// suite's harness does while the test holds the main thread.
#[cfg(feature = "device-tests")]
pub(crate) fn run_pending() {
    loop {
        let task = QUEUE.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
        let Some(task) = task else { break };
        crate::jni_host::guard("looper", task);
    }
}

/// The looper's callback: drains the pipe and runs what was queued.
unsafe extern "C" fn woken(fd: c_int, _events: c_int, _data: *mut c_void) -> c_int {
    let mut sink = [0u8; 64];
    // SAFETY: `fd` is the pipe's read end and `sink` is writable for its
    // length; the pipe is non-blocking.
    while unsafe { libc::read(fd, sink.as_mut_ptr().cast(), sink.len()) } > 0 {}
    loop {
        let task = QUEUE.lock().unwrap_or_else(PoisonError::into_inner).pop_front();
        let Some(task) = task else { break };
        // Each task in its own boundary: one that panics does not strand
        // the ones queued behind it.
        crate::jni_host::guard("looper", task);
    }
    1
}
