//! Who owns what: this backend's host-object conventions in one place
//! (Milestone 39: one ownership module per backend, each rule asserted).
//!
//! | Object | Owner | Created | Destroyed | Asserted by |
//! |---|---|---|---|---|
//! | Top-level `HWND` | `WindowRegistry` (one `Runtime` each) | `create_window_once` | `WM_CLOSE` → `DestroyWindow`, or the registry's drop | `integration::native_window_lifecycle`, `integration::native_dynamic_window_close` |
//! | Control and container `HWND`s | the window's `NativeObjectRegistry` | a `TreeOp::Insert` | a `TreeOp::Remove`, the registry's drop, or a parked row drained from the pool | `registry::tests`, `integration::native_child_reconciliation`, `integration::native_gdi_resource_lifecycle` |
//! | `HFONT`, `HBRUSH` | the renderer's `StyleCache` | first realization of a style | replacement, node removal, or the cache's drop | `integration::native_gdi_resource_lifecycle` (GDI object count returns to baseline) |
//! | `HMENU` | `BuiltMenu` until attached, then the window | `build_native_menu` | by the window (attached) or `BuiltMenu`'s drop (not) | `menu::tests::*` |
//! | COM objects (UIA providers, drop targets, Direct2D, WIC) | `windows` crate smart pointers held by their `Runtime` | on demand | when the last reference drops, before `CoUninitialize` (`OleApartment` outlives the registry) | `uia_integration`, `graphics_integration`, `input_integration` |
//! | The low-memory watcher thread and its kernel objects | `MemoryWatcher` | `run_application` | its drop: stop event set, thread joined, handles closed | `memory_watch::tests::the_watcher_starts_and_stops_cleanly` |
//! | Escape-hatch handle table entries | `crate::handle`, written only by the registry | object insertion | object removal or registry drop | `integration::native_handle_validates_and_goes_stale` |
//!
//! Conventions, which every new module follows:
//!
//! 1. **A registry owns every `HWND` it created and nothing else.** A
//!    window the framework did not create (a host window in embedding, a
//!    foreign control) is borrowed and never destroyed by it.
//! 2. **Destruction is explicit and single.** `NativeObject` has no `Drop`;
//!    exactly one path (`NativeObject::destroy`) calls `DestroyWindow`, so
//!    a double destroy is structurally impossible.
//! 3. **GDI objects outlive every control that references them.** A font
//!    is released only after no control can still be showing it.
//! 4. **COM is balanced per thread.** The apartment is entered before any
//!    COM object exists and left after the last one is released.
//! 5. **Handles cross threads only as integers**, and only to be handed
//!    back to a Win32 call (`PostMessageW`, `SetEvent`), never dereferenced.
