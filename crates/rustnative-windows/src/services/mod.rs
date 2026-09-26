//! Native OS-service integrations: clipboard, shell/system, file dialogs,
//! and tray notifications. Each lives in its own module but shares the two
//! async execution helpers below.

pub mod clipboard;
#[cfg(windows)]
pub mod data;
pub mod dialogs;
#[cfg(windows)]
pub mod http;
#[cfg(windows)]
pub mod industrial;
#[cfg(windows)]
pub mod locale;
pub mod notifications;
#[cfg(windows)]
pub mod permissions;
#[cfg(windows)]
pub mod product;
#[cfg(windows)]
pub mod secure_storage;
#[cfg(windows)]
pub mod state_store;
pub mod system;

/// Runs `f` on tokio's dedicated blocking-task pool rather than a scheduler
/// worker thread. Every `f` here wraps a synchronous Win32 call (there is no
/// async I/O on this side of the FFI boundary at all) — running it inline in
/// an `async fn` body would still block whichever worker thread executes it
/// to completion, and with only a couple of shared workers backing the whole
/// framework (see `rustnative_core`'s scheduler), one open file-picker or
/// blocked `ShellExecuteW` call could stall every other component's pending
/// task in the process. `spawn_blocking` moves the call to a pool sized for
/// exactly this.
#[cfg(windows)]
pub(crate) async fn run_blocking<T, F>(f: F) -> Result<T, rustnative_core::ServiceError>
where
    F: FnOnce() -> Result<T, rustnative_core::ServiceError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|_| Err(rustnative_core::ServiceError::new("blocking task panicked")))
}

/// Runs a shell-dialog request on a newly-created STA thread. Tokio's
/// blocking pool deliberately reuses worker threads and therefore cannot
/// establish an apartment-model invariant for COM UI APIs.
#[cfg(windows)]
pub(crate) async fn run_sta<T, F>(f: F) -> Result<T, rustnative_core::ServiceError>
where
    F: FnOnce() -> Result<T, rustnative_core::ServiceError> + Send + 'static,
    T: Send + 'static,
{
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
    };

    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("framework-dialog-sta".to_owned())
        .spawn(move || {
            // SAFETY: `std::ptr::null()` is a documented-valid
            // `pvReserved` argument (it must always be null);
            // `COINIT_APARTMENTTHREADED` is a documented, valid
            // initialization-mode constant. This call happens on a
            // freshly spawned thread that performs no other COM
            // activity before it, satisfying `CoInitializeEx`'s
            // per-thread, call-before-any-other-COM-use contract.
            let initialized =
                unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
            let result = if initialized >= 0 {
                let result = f();
                // SAFETY: pairs the successful `CoInitializeEx` call
                // immediately above, on the same thread, after `f`
                // (the only COM/dialog activity this thread performs)
                // has returned.
                unsafe { CoUninitialize() };
                result
            } else {
                Err(rustnative_core::ServiceError::new(format!(
                    "failed to initialize the dialog STA (HRESULT 0x{initialized:08X})"
                )))
            };
            let _ = sender.send(result);
        })
        .map_err(|error| {
            rustnative_core::ServiceError::new(format!("failed to start dialog STA: {error}"))
        })?;
    receiver.await.map_err(|_| {
        rustnative_core::ServiceError::new("dialog STA terminated before returning a result")
    })?
}
