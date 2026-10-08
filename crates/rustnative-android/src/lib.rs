//! Android platform backend (`PLAN.md` Milestone 35).
//!
//! The backend realizes the portable tree as Android's own platform views —
//! `TextView`, `Button`, `EditText`, `CheckBox`, `Switch`, `SeekBar`,
//! `Spinner`, … — inside framework `ViewGroup`s that place each view where
//! the portable layout engine put it, measures on prototypes of those views
//! and with `StaticLayout`, translates `MotionEvent`s and `KeyEvent`s into
//! the portable input model, carries the accessibility model on
//! `AccessibilityNodeInfo`, and maps the activity lifecycle onto the
//! portable lifecycle and restoration contracts. It ships a Java host
//! library (`java/`), which `rustnative build android` puts in the
//! generated Gradle project.
//!
//! Module map (private modules; the public API is re-exported flat):
//! - `jni_host`: the JNI boundary — the one module that names `jni::`.
//! - `entry`: where Android starts the application (`export_main!`).
//! - `backend`, `registry`: the work queue and every window's state.
//! - `rendering`, `measure`, `styling`, `units`: realization.
//! - `input`, `accessibility`, `animation`, `lifecycle`, `environment`,
//!   `host_traits`, `menus`, `intents`, `surfaces`, `services`: the rest.
//!
//! # Getting started
//!
//! An application's `main` is the same on every host. For Android, the
//! crate is built as a library and exports its `main` for the host to
//! start:
//!
//! ```no_run
//! use rustnative_core::{Application, Component, Event, Node, Platform, Size, Window};
//! use rustnative_android::AndroidPlatform;
//!
//! # struct Greeter;
//! # impl Component for Greeter {
//! #     type Props = ();
//! #     type Message = ();
//! #     fn new((): Self::Props) -> Self { Self }
//! #     fn props(&self) -> &Self::Props { &() }
//! #     fn set_props(&mut self, (): Self::Props) {}
//! #     fn view(&self) -> Node { Node::label("greeting", "Hello") }
//! #     fn update(&mut self, _event: Event) {}
//! # }
//! pub fn main() {
//!     let mut application =
//!         Application::new(Greeter::new(()), Window::new("Greeter", Size::new(320, 200)));
//!     // On Android, `run` adopts the application and returns at once; the
//!     // host's own loop drives it from then on.
//!     AndroidPlatform::new().run(&mut application).expect("the application started");
//! }
//!
//! #[cfg(target_os = "android")]
//! rustnative_android::export_main!(main);
//! ```
#![deny(missing_docs)]
// The portable half (mapping tables, unit and style computation) is what
// the Android build calls; on another host only its tests do.
#![cfg_attr(
    target_os = "android",
    allow(
        clippy::missing_const_for_thread_local,
        reason = "every `thread_local!` here is `const` already; the lint misfires on Android's emulated TLS"
    )
)]
#![cfg_attr(
    not(target_os = "android"),
    allow(dead_code, unused_imports, reason = "the portable half is called by the Android build")
)]

mod accessibility;
mod canvas;
mod capabilities;
mod environment;
mod error;
mod host_traits;
mod input;
mod intents;
mod lifecycle;
mod log;
mod menus;
mod pixels;
mod platform;
mod posture;
mod protocol;
mod styling;
mod surface;
mod units;

#[cfg(target_os = "android")]
mod animation;
#[cfg(target_os = "android")]
mod backend;
#[cfg(all(target_os = "android", feature = "device-tests"))]
mod device_tests;
#[cfg(target_os = "android")]
mod entry;
#[cfg(target_os = "android")]
mod foreign;
#[cfg(target_os = "android")]
mod inspect;
#[cfg(target_os = "android")]
mod jni_host;
#[cfg(target_os = "android")]
mod looper;
#[cfg(target_os = "android")]
mod mappers;
#[cfg(target_os = "android")]
mod measure;
#[cfg(target_os = "android")]
mod registry;
#[cfg(target_os = "android")]
mod rendering;
mod services;
#[cfg(target_os = "android")]
mod surfaces;
#[cfg(target_os = "android")]
mod timers;

pub use error::{Error, NativeContext};
#[cfg(target_os = "android")]
pub use foreign::{ForeignView, Ownership, register_foreign, register_foreign_class};
#[cfg(target_os = "android")]
pub use mappers::{
    MappedProperty, MapperContext, MapperInfo, MapperMode, MapperTarget, NativeView,
    active_mappers, clear_mappers, register_mapper,
};
pub use platform::AndroidPlatform;
pub use services::FileStateStore;
#[cfg(target_os = "android")]
pub use services::{
    AndroidClipboard, AndroidConditions, AndroidFileDialogs, AndroidHttp, AndroidLocale,
    AndroidPermissions, AndroidPrinting, AndroidPush, AndroidSecureStorage, AndroidSerial,
    AndroidStore, AndroidSystem, AndroidWork, BitmapDecoder, JobHandler, read_document,
    write_document,
};
pub use surface::{SurfaceHandle, native_surface};

/// The process's Java VM as a JNI `JavaVM*`, for an application's own JNI
/// calls (a mapper's, a foreign view factory's). Null before the library is
/// loaded, and on every host but Android.
#[must_use]
pub fn java_vm() -> *mut std::ffi::c_void {
    #[cfg(target_os = "android")]
    {
        jni_host::raw_vm()
    }
    #[cfg(not(target_os = "android"))]
    {
        std::ptr::null_mut()
    }
}

/// Exports the application's `main` for Android to start: expands to the
/// library's `JNI_OnLoad`, which records `main` and connects the host
/// library. The launcher activity runs `main` on the main thread when it is
/// created; its `AndroidPlatform::run` adopts the application.
///
/// `export_main!()` with no `main` is library-only mode (Milestone 40): the
/// application model and services are linked into an existing Android
/// application, which embeds views with `RustNativeView` or uses no
/// framework UI at all.
#[macro_export]
macro_rules! export_main {
    ($main:path) => {
        /// The JNI entry Android calls when it loads this library.
        #[unsafe(no_mangle)]
        pub extern "system" fn JNI_OnLoad(
            vm: *mut ::std::ffi::c_void,
            _reserved: *mut ::std::ffi::c_void,
        ) -> i32 {
            // SAFETY: `vm` is the runtime's own VM pointer, passed to
            // `JNI_OnLoad` for exactly this.
            unsafe { $crate::__private::on_load(vm, ::core::option::Option::Some($main)) }
        }
    };
    ($main:path, work = $work:path) => {
        /// The JNI entry Android calls when it loads this library.
        #[unsafe(no_mangle)]
        pub extern "system" fn JNI_OnLoad(
            vm: *mut ::std::ffi::c_void,
            _reserved: *mut ::std::ffi::c_void,
        ) -> i32 {
            // Every process start registers its background jobs, including
            // one `JobScheduler` starts with no activity.
            $work();
            // SAFETY: as above.
            unsafe { $crate::__private::on_load(vm, ::core::option::Option::Some($main)) }
        }
    };
    () => {
        /// The JNI entry Android calls when it loads this library.
        #[unsafe(no_mangle)]
        pub extern "system" fn JNI_OnLoad(
            vm: *mut ::std::ffi::c_void,
            _reserved: *mut ::std::ffi::c_void,
        ) -> i32 {
            // SAFETY: as above.
            unsafe { $crate::__private::on_load(vm, ::core::option::Option::None) }
        }
    };
}

/// What `export_main!` expands to; not public API.
#[doc(hidden)]
pub mod __private {
    /// `JNI_OnLoad`'s body.
    ///
    /// # Safety
    ///
    /// `vm` is the pointer the runtime passed to `JNI_OnLoad`.
    #[must_use]
    pub unsafe fn on_load(vm: *mut std::ffi::c_void, main: Option<fn()>) -> i32 {
        #[cfg(target_os = "android")]
        {
            // SAFETY: forwarded from this function's contract.
            unsafe { crate::jni_host::on_load(vm.cast(), main) }
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (vm, main);
            -1
        }
    }
}

/// Runs the preview catalogue (`rustnative_core::preview::Catalogue`) on
/// this backend, opened at the preview named `first` — what an
/// application's `main` does when `rustnative preview` runs it.
///
/// # Errors
///
/// As [`AndroidPlatform`]'s `run`.
pub fn run_catalogue(
    previews: Vec<rustnative_core::preview::Preview>,
    first: &str,
) -> Result<(), Error> {
    use rustnative_core::Platform as _;
    let mut application = rustnative_core::Application::new(
        rustnative_core::preview::Catalogue::open(previews, first),
        rustnative_core::Window::new("Previews", rustnative_core::Size::new(400, 800)),
    );
    AndroidPlatform::new().run(&mut application)
}
