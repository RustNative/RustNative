//! Registers the top-level window class, runs the Win32 message loop, and
//! implements the top-level `WNDPROC`.

use rustnative_core::{Event, PanicAction, PanicReport};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_WINDOW, GetSysColorBrush, HDC, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::Controls::{
    DTN_DATETIMECHANGE, NM_CLICK, NM_RETURN, NMHDR, TCN_SELCHANGE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CBN_SELCHANGE, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, EN_CHANGE, GetMessageW, KillTimer, LBN_SELCHANGE, MSG,
    PM_NOREMOVE, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SWP_NOACTIVATE,
    SWP_NOZORDER, SetWindowPos, TranslateMessage, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_ENDSESSION,
    WM_HSCROLL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_MOVE, WM_NCCREATE, WM_NOTIFY, WM_PAINT, WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE,
    WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_QUIT, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDBLCLK,
    WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
};

use super::container::container_proc;
use super::context::{root_window, with_runtime};
use super::input::pointer::{
    LONG_PRESS_TIMER_ID, MOUSE_POINTER_ID, WM_FRAMEWORK_CAPTURE_LOST, WM_POINTERCAPTURECHANGED,
};
use super::input::{
    self, clipboard, focus_next, focused_node, key_code, modifiers, set_hovered, set_pressed,
    sync_focus,
};
use super::registry::NativeObject;
use super::runtime::Runtime;
use super::user_data::RuntimeSlot;
use super::win32::{best_effort, ignored_by_contract, informational};
use super::{
    CONTAINER_CLASS_NAME, EnableWindow, WINDOW_CLASS_NAME, WM_FRAMEWORK_SCHEDULE, WM_MOUSELEAVE,
};
use crate::Error;
use crate::error::{NativeContext, Win32Category};
use crate::native::util::window_text;

/// Whether the loop should keep going after a message, or stop because
/// `WM_QUIT` was posted.
///
/// `run_message_loop` learns this from `GetMessageW`'s return value, while
/// the test harness (`native::harness`) learns it from `PeekMessageW`
/// returning `WM_QUIT` as an ordinary message — which is exactly why
/// [`handle_message`] reports it rather than deciding for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopStep {
    /// Keep pumping.
    Continue,
    /// `WM_QUIT` was seen; unwind the loop.
    Quit,
}

pub(crate) fn run_message_loop() -> Result<(), Error> {
    let mut message = MSG::default();
    let exit_when_interactive =
        std::env::var("RUSTNATIVE_EXIT_AT").is_ok_and(|value| value == "interactive");

    loop {
        mark_interactive_when_idle(exit_when_interactive);
        // SAFETY: `message` is a valid, exclusively borrowed `MSG` for
        // `GetMessageW` to write into; a null `hWnd` filter is the
        // documented way to retrieve messages for every window owned by
        // this thread. A `-1` return (checked below) is the documented
        // failure signal; `0` (also checked below) signals `WM_QUIT`.
        let result = unsafe { GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) };

        if result == -1 {
            return Err(Error::windows_api("GetMessageW"));
        }

        if result == 0 {
            break;
        }

        if handle_message(&message)? == LoopStep::Quit {
            break;
        }
    }

    Ok(())
}

/// Marks the startup model's "interactive" the first time the queue is
/// empty after the first window painted — the application has nothing left
/// to do and will answer the next input at once. With
/// `RUSTNATIVE_EXIT_AT=interactive` it then quits: the scripted startup a
/// profile-guided build and the startup budget run (`PLAN.md` Milestone 42).
fn mark_interactive_when_idle(exit: bool) {
    use rustnative_core::perf::{self, StartupPhase};
    if perf::reached(StartupPhase::Interactive) || !perf::reached(StartupPhase::FirstContent) {
        return;
    }
    let mut peeked = MSG::default();
    // SAFETY: `peeked` is writable; `PM_NOREMOVE` leaves the queue as it
    // was.
    let pending =
        unsafe { PeekMessageW(&raw mut peeked, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) } != 0;
    if !pending && perf::mark(StartupPhase::Interactive) && exit {
        // SAFETY: plain call on this thread.
        unsafe { PostQuitMessage(0) };
    }
}

/// Everything the message loop does with one message: framework-level
/// pre-processing, native dispatch, and the post-dispatch focus
/// reconciliation.
///
/// This is a function rather than the body of `run_message_loop` so that
/// the native integration harness pumps messages through the *same* code
/// the application does. A harness with its own parallel dispatch would
/// test itself rather than the backend — which is the trap the standards
/// audit's P1.17 finding warns about ("rather than pretending compile-time
/// tests cover native behavior").
pub(crate) fn handle_message(message: &MSG) -> Result<LoopStep, Error> {
    if message.message == WM_QUIT {
        return Ok(LoopStep::Quit);
    }

    if message.message == WM_FRAMEWORK_SCHEDULE {
        // The only thing that ever posts this custom message is the waker
        // `create_window_once` installs, targeting that same window's own
        // top-level hwnd — so this resolves the handle directly rather than
        // walking to an ancestor. A `None` result means the window has
        // since been destroyed and the wake is moot.
        with_runtime(message.hwnd, Runtime::pump_tasks).transpose()?;
        return Ok(LoopStep::Continue);
    }

    // Win32 addresses each message to whichever window it concerns —
    // frequently a native control or a nested container, not the top-level
    // window that owns the `Runtime`. Resolve the root once and reuse it
    // for both the pre-dispatch pass and the focus sync after dispatch.
    let root = root_window(message.hwnd);
    let flow = with_runtime(root, |runtime| pre_dispatch(runtime, message))
        .unwrap_or(MessageFlow::Dispatch);
    if flow == MessageFlow::Consumed {
        return Ok(LoopStep::Continue);
    }

    // SAFETY: `message` points at a `MSG` the caller just obtained from
    // `GetMessageW`/`PeekMessageW`; both calls only read it for the
    // duration of this statement.
    //
    // `TranslateMessage` reports whether the message *was* translated
    // (into a `WM_CHAR`), not whether it succeeded, and `DispatchMessageW`
    // returns the target `WNDPROC`'s own `LRESULT` — neither is a status
    // code this loop can act on.
    unsafe {
        ignored_by_contract(TranslateMessage(message));
        informational(DispatchMessageW(message));
    }

    // Native focus can have moved during dispatch (a click on a control, a
    // `SetFocus` from application code) without the framework hearing about
    // it, so reconcile after every message rather than only after the ones
    // that obviously change focus.
    with_runtime(root, sync_focus);
    Ok(LoopStep::Continue)
}

/// [`handle_message`] for a host-owned loop (`native::embed`): handles a
/// message for one of the framework's windows and returns `true`, or
/// returns `false` for a message that is the host's.
pub(crate) fn handle_for_host(message: &MSG) -> Result<bool, Error> {
    let ours = message.message == WM_FRAMEWORK_SCHEDULE
        || with_runtime(root_window(message.hwnd), |_| ()).is_some();
    if !ours {
        return Ok(false);
    }
    handle_message(message)?;
    Ok(true)
}

/// Whether a message this loop pre-processed still needs Win32's ordinary
/// `TranslateMessage`/`DispatchMessageW` treatment afterwards.
///
/// A few messages are fully consumed before dispatch — Tab is turned into
/// focus traversal rather than reaching a control, and a wheel event over a
/// scrollable container becomes a viewport transform — and forwarding those
/// on would let the native control act on them a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageFlow {
    /// Hand the message to Win32 as usual.
    Dispatch,
    /// The pre-dispatch step fully handled it; skip native dispatch.
    Consumed,
}

/// Framework-level handling that must happen *before* Win32 dispatches a
/// message to the target window's own procedure.
///
/// This exists as a separate pass, rather than as more arms inside
/// `window_proc`, because everything here concerns messages Win32 delivers
/// to a **child** window — a native control or a container — whose window
/// procedure is either the system's own (for `STATIC`/`BUTTON`/`EDIT`) or
/// `container_proc`. Neither can see the owning `Runtime` at the moment the
/// message arrives; the loop can, because it has just resolved the root.
fn pre_dispatch(runtime: &mut Runtime, message: &MSG) -> MessageFlow {
    match message.message {
        WM_MOUSEMOVE => {
            let hovered = runtime.renderer.registry.id_for_hwnd(message.hwnd);
            set_hovered(runtime, hovered);
            if hovered.is_some() {
                track_mouse_leave(message.hwnd);
            }
        }
        WM_MOUSELEAVE => set_hovered(runtime, None),
        WM_LBUTTONDOWN => {
            let pressed = runtime.renderer.registry.id_for_hwnd(message.hwnd);
            set_pressed(runtime, pressed);
        }
        WM_LBUTTONUP => set_pressed(runtime, None),
        _ => {}
    }

    match message.message {
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN
        | WM_RBUTTONUP | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK
        | WM_XBUTTONDOWN | WM_XBUTTONUP | WM_XBUTTONDBLCLK => {
            input::pointer::mouse_message(runtime, message);
        }
        WM_MOUSELEAVE => input::pointer::mouse_left(runtime),
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
            input::pointer::pointer_message(runtime, message);
        }
        WM_POINTERCAPTURECHANGED => input::pointer::pointer_capture_changed(runtime, message),
        _ => {}
    }

    match message.message {
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => wheel(runtime, message),
        WM_KEYDOWN | WM_SYSKEYDOWN => key_down(runtime, message),
        WM_KEYUP | WM_SYSKEYUP => {
            key_up(runtime, message);
            MessageFlow::Dispatch
        }
        WM_CHAR => {
            character(runtime, message);
            MessageFlow::Dispatch
        }
        _ => MessageFlow::Dispatch,
    }
}

/// Asks Win32 to send one `WM_MOUSELEAVE` when the pointer next leaves
/// `hwnd`, which is the only way to learn that a control stopped being
/// hovered — there is no "mouse exited" message otherwise.
fn track_mouse_leave(hwnd: HWND) {
    // `TRACKMOUSEEVENT` is a small, fixed-layout Win32 struct; its size
    // can never approach `u32::MAX`, so this cannot actually truncate.
    #[allow(clippy::cast_possible_truncation)]
    let cb_size = std::mem::size_of::<TRACKMOUSEEVENT>() as u32;
    let mut tracking =
        TRACKMOUSEEVENT { cbSize: cb_size, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
    // SAFETY: `tracking` is a fully initialized, exclusively borrowed
    // `TRACKMOUSEEVENT`; `hwndTrack` is the live HWND Win32 just delivered
    // `WM_MOUSEMOVE` for.
    let tracked = unsafe { TrackMouseEvent(&raw mut tracking) } != 0;
    // Best effort: without the leave notification the control simply keeps
    // its hover styling until the pointer enters a different tracked
    // control, which is a cosmetic degradation, not an incorrect UI.
    best_effort(tracked, "TrackMouseEvent(TME_LEAVE)", "hover styling is cosmetic");
}

/// Delivers a wheel event to the innermost wheel-interested node under the
/// pointer, or — if a scrollable container is nearer, or no node asked —
/// scrolls that container, as before Milestone 25.
fn wheel(runtime: &mut Runtime, message: &MSG) -> MessageFlow {
    // The wheel message carries the pointer's screen position, and Windows
    // has already decided it belongs to this window; the only question left
    // is which of this window's own descendants is under that point. Asking
    // `WindowFromPoint` instead would answer with whatever window is topmost
    // on the desktop there, which is not this window's business.
    let hovered = input::pointer::deepest_child_at(
        runtime.window,
        input::pointer::wheel_screen_point(message),
    );
    if input::pointer::wheel(runtime, message, hovered) {
        // The component owns this wheel; the native control must not also
        // act on it.
        return MessageFlow::Consumed;
    }
    if message.message == WM_MOUSEHWHEEL {
        // Containers scroll vertically only; a horizontal wheel no node
        // asked for is left to the native control under the pointer.
        return MessageFlow::Dispatch;
    }
    let Some(id) = runtime.renderer.scrollable_ancestor(hovered) else {
        return MessageFlow::Dispatch;
    };

    // The high word of `wParam` carries the wheel delta as a signed 16-bit
    // value (`WHEEL_DELTA` units) per `WM_MOUSEWHEEL`'s documented layout;
    // reinterpreting that exact `u16` as `i16` is the intentional,
    // documented bit pattern this message defines, not a truncating or
    // wrapping value change.
    #[allow(clippy::cast_possible_wrap)]
    let delta = super::util::hiword(message.wParam) as i16;
    runtime.renderer.scroll_container(id, 0, -(i32::from(delta) / 3).clamp(-120, 120));
    // Scrolling past the edge of a virtual list's realized items is the
    // one thing a scroll *does* reach a component for.
    super::virtual_list::after_render(runtime);
    // A scroll is a viewport transform this framework owns; letting the
    // native control also act on the same wheel event would double-scroll.
    MessageFlow::Consumed
}

/// Turns a key press into either framework focus traversal (Tab) or a
/// `KeyDown` event for the focused component.
fn key_down(runtime: &mut Runtime, message: &MSG) -> MessageFlow {
    // `wParam` for `WM_KEYDOWN` documents its virtual-key code as fitting
    // in the low byte; the truncation this cast could in principle perform
    // never actually happens for any value Win32 delivers here.
    #[allow(clippy::cast_possible_truncation)]
    let key = key_code(message.wParam as u32);
    if key == rustnative_core::KeyCode::Tab {
        focus_next(runtime, modifiers().shift);
        sync_focus(runtime);
        // Tab is framework-level focus traversal; forwarding it would let
        // Win32's own dialog-manager-style handling move focus a second
        // time.
        return MessageFlow::Consumed;
    }
    let held = modifiers();
    let target = focused_node(runtime);
    // A command's shortcut wins over the focused control's own handling of
    // the key, as an accelerator does.
    match super::commands::shortcut(runtime, key, held, target) {
        Ok(true) => return MessageFlow::Consumed,
        Ok(false) => {}
        Err(error) => {
            runtime.error = Some(error);
            super::embed::request_quit(1);
            return MessageFlow::Consumed;
        }
    }
    if runtime.dispatch_or_quit(Event::KeyDown { target, key, modifiers: held }) {
        clipboard::shortcut(runtime, key, held);
    }
    MessageFlow::Dispatch
}

/// Delivers a key release to the focused component.
fn key_up(runtime: &mut Runtime, message: &MSG) {
    // As in `key_down`: the virtual-key code occupies the low byte.
    #[allow(clippy::cast_possible_truncation)]
    let key = key_code(message.wParam as u32);
    let target = focused_node(runtime);
    runtime.dispatch_or_quit(Event::KeyUp { target, key, modifiers: modifiers() });
}

/// Delivers committed text to the focused component, for every focus target
/// except a native `EDIT`, which produces its own `EN_CHANGE` notification
/// and would otherwise report the same keystroke twice.
fn character(runtime: &mut Runtime, message: &MSG) {
    let target = focused_node(runtime);
    let native_text_input = target
        .and_then(|id| runtime.renderer.registry.get(id))
        .is_some_and(|object| matches!(object, NativeObject::TextInput(_)));
    if native_text_input {
        return;
    }
    // `wParam` for `WM_CHAR` documents its UTF-16 code unit as occupying
    // the low word; this narrowing cannot lose information for any value
    // Win32 delivers here.
    #[allow(clippy::cast_possible_truncation)]
    let wparam_char = message.wParam as u32;
    let Some(character) = char::from_u32(wparam_char) else {
        return;
    };
    if character.is_control() {
        return;
    }
    runtime.dispatch_or_quit(Event::TextInput { target, text: character.to_string() });
}

pub(crate) fn register_window_classes(instance: HINSTANCE) -> Result<(), Error> {
    let top_level = register_window_class(instance, WINDOW_CLASS_NAME, window_proc)?;
    super::user_data::set_top_level_class_atom(top_level);
    register_window_class(instance, CONTAINER_CLASS_NAME, container_proc)?;
    register_window_class(
        instance,
        super::graphics::canvas::CANVAS_CLASS_NAME,
        super::graphics::canvas::canvas_proc,
    )?;
    register_window_class(
        instance,
        super::graphics::surface::SURFACE_CLASS_NAME,
        super::graphics::surface::surface_proc,
    )
    .map(|_| ())
}

/// Registers one class, returning its atom — also when an earlier call in
/// this process already registered it.
fn register_window_class(
    instance: HINSTANCE,
    class_name: &str,
    window_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> Result<u16, Error> {
    const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
    let class_name = super::util::wide(class_name);

    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        // SAFETY: `COLOR_WINDOW` is a documented standard system-color
        // index; `GetSysColorBrush` takes no pointer arguments and
        // returns a static system brush that must not be deleted.
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    // SAFETY: `class` is a fully initialized `WNDCLASSW`, exclusively
    // borrowed for the duration of this call; `class_name` (borrowed by
    // `class.lpszClassName`) is a NUL-terminated wide buffer that
    // outlives this call.
    let atom = unsafe { RegisterClassW(&raw const class) };
    if atom != 0 {
        return Ok(atom);
    }
    // SAFETY: `GetLastError` takes no arguments and is called immediately
    // after `RegisterClassW` reported failure, on the same thread, before
    // any other call could overwrite the thread-local error code.
    let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    if error != ERROR_CLASS_ALREADY_EXISTS {
        return Err(Error::WindowsApi {
            operation: "RegisterClassW",
            code: error,
            category: Win32Category::of(error),
            // Class registration happens once, before any window exists,
            // so there is no window or node to name.
            context: NativeContext::none(),
        });
    }

    // Already registered (an earlier `run_application`, or a test harness,
    // in this process): `GetClassInfoExW` returns the existing class's atom.
    // `WNDCLASSEXW` is a fixed-size Win32 struct, far below `u32::MAX`.
    #[allow(clippy::cast_possible_truncation)]
    let mut existing = windows_sys::Win32::UI::WindowsAndMessaging::WNDCLASSEXW {
        cbSize: std::mem::size_of::<windows_sys::Win32::UI::WindowsAndMessaging::WNDCLASSEXW>()
            as u32,
        ..Default::default()
    };
    // SAFETY: `class_name` is the NUL-terminated name registered above;
    // `existing` is a valid, exclusively borrowed `WNDCLASSEXW` with
    // `cbSize` set as the call requires.
    let found = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetClassInfoExW(
            instance,
            class_name.as_ptr(),
            &raw mut existing,
        )
    };
    // The documented return value of `GetClassInfoExW` on success *is* the
    // class atom (typed `BOOL` in the header), so it fits a `u16`.
    u16::try_from(found)
        .ok()
        .filter(|atom| *atom != 0)
        .ok_or_else(|| Error::windows_api_in("GetClassInfoExW", NativeContext::none()))
}

/// Wraps the body of a Win32 `WNDPROC` callback so a panic inside user
/// `Component` code cannot unwind across this `extern "system"` FFI
/// boundary — undefined behavior on stable Rust (standards audit P0.1).
/// A caught panic poisons the owning `Runtime` with a typed error and
/// posts `WM_QUIT`, so the message loop still exits, just cleanly and
/// through `run_application`'s ordinary error path instead of
/// potentially corrupting Win32's own call stack.
pub(crate) fn wndproc_boundary<F>(hwnd: HWND, f: F) -> LRESULT
where
    F: FnOnce() -> LRESULT + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let message = panic_payload_message(payload.as_ref());
            // `RuntimeSlot::get` on every top-level window this crate
            // creates returns either null (WM_NCCREATE has not run yet)
            // or a live `*mut Runtime` set exactly once in WM_NCCREATE
            // and never reassigned to a dangling value for the window's
            // lifetime — see WM_NCCREATE below.
            let runtime_ptr = RuntimeSlot::get(hwnd);
            poison_runtime_and_quit(runtime_ptr, message);
            0
        }
    }
}

/// Shared tail of both `wndproc_boundary` variants: records the caught
/// panic and applies the application's configured [`PanicPolicy`].
///
/// Factored out once so the two callback-specific boundaries — which
/// necessarily resolve `runtime_ptr` differently (see
/// `container::container_wndproc_boundary`) — do not each re-derive this
/// SAFETY-critical step independently.
///
/// A panic with no resolvable `Runtime` (a callback that panicked before
/// `WM_NCCREATE` published one) has no application to consult and nowhere
/// to record the error, so it always quits: continuing would mean running
/// on after a panic nobody can observe.
pub(crate) fn poison_runtime_and_quit(runtime_ptr: *mut Runtime, message: String) {
    if runtime_ptr.is_null() {
        quit_message_loop();
        return;
    }
    // SAFETY: the caller guarantees `runtime_ptr` is either null (returned
    // above) or a live `*mut Runtime` per `RuntimeSlot`'s invariant (see
    // `user_data`'s module docs), and the Win32 message loop is
    // single-threaded and non-reentrant, so nothing else can be
    // concurrently mutating this `Runtime` right now.
    let runtime = unsafe { &mut *runtime_ptr };
    let window = runtime.window_id;
    let report = PanicReport { message: message.clone(), window };
    let action =
        runtime.with_application(|application| application.handle_component_panic(&report));

    // A development run shows where it happened — in the `.rsx` file when
    // the panicking code was lowered from markup — before the policy acts
    // (`rustnative_core::dev`, `PLAN.md` Milestone 43).
    if let Some(site) = rustnative_core::dev::take_panic() {
        show_dev_error(runtime.window, &site.to_string());
    }
    match action {
        PanicAction::Terminate => {
            // The host is put back before the loop unwinds: whatever the
            // panicking component held (a capture, a clipped cursor, an
            // open composition) is released now, not when the process ends.
            super::teardown::restore(&super::teardown::policy());
            let context =
                NativeContext::none().with_window(window).with_handle(runtime.window as usize);
            runtime.error = Some(Error::ComponentPanicked { message, context });
            quit_message_loop();
        }
        PanicAction::CloseWindow(target) => {
            // Deliberately routed through the same deferred path a normal
            // close uses (`Application::close_window`, picked up by
            // `WindowRegistry::sync`) rather than destroying the window
            // here. This code runs *inside* the panicking callback, with
            // that window's `Runtime` borrowed further up the stack;
            // tearing it down synchronously is the use-after-free
            // `WindowRegistry`'s own documentation describes.
            //
            // The error is not recorded: under this policy the panic is
            // handled, not fatal, and `run_application` returns
            // `Runtime::error` as the failure of `Platform::run`.
            runtime.with_application(|application| application.close_window(target));
            // A panic unwinds past the `sync_windows` at the tail of
            // `Runtime::dispatch`, so the close request just queued would
            // otherwise sit unapplied forever. Syncing here is what turns it
            // into a posted `WM_CLOSE`.
            if let Err(error) = runtime.sync_windows() {
                // The window could not be torn down, which leaves the
                // application in exactly the state `CloseWindow` was chosen
                // to avoid; fall back to the stronger policy rather than
                // carrying on with a window whose component just panicked.
                runtime.error = Some(error);
                quit_message_loop();
            }
        }
        // `PanicAction::Continue`, and any action a future version of
        // `rustnative-core` adds that this backend has not caught up with.
        // Continuing is the right default for an unrecognized action: the
        // panic was already caught before it could unwind across the FFI
        // boundary, which is the part that had to happen, and inventing a
        // termination this backend was not asked for would be worse than
        // doing nothing.
        _ => {}
    }
}

/// `WM_DPICHANGED`: the window moved to a monitor of another DPI (or the
/// DPI changed under it). It takes the size Windows suggests, and every
/// surface is told its new scale — the surface hand-off contract
/// (`docs/interop/surface-handoff.md`).
fn dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let dpi = u32::try_from(wparam & 0xFFFF).unwrap_or(96);
    let suggested = lparam as *const RECT;
    if !suggested.is_null() {
        // SAFETY: for `WM_DPICHANGED`, `lParam` points at the
        // suggested window rectangle, valid for this message.
        let rect = unsafe { *suggested };
        with_runtime(hwnd, |runtime| runtime.renderer.set_dpi(dpi));
        // SAFETY: `hwnd` is this procedure's live window; no
        // pointers.
        let moved = unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } != 0;
        best_effort(moved, "SetWindowPos(WM_DPICHANGED)", "the window keeps its old size");
    }
    with_runtime(hwnd, |runtime| {
        runtime.renderer.set_dpi(dpi);
        runtime.renderer.collect_surface_changes();
        runtime.report_surface_changes();
    });
}

/// Asks the message loop to exit through its ordinary path.
fn quit_message_loop() {
    super::embed::request_quit(1);
}

/// Extracts a human-readable message from a caught panic's payload.
///
/// The parameter is the *unboxed* payload, and both callers must pass
/// `payload.as_ref()` rather than `&payload`. That is not a style
/// preference: `Box<dyn Any + Send>` is itself `Sized`, `Send`, and
/// `'static`, so it satisfies `Any` too — `&payload` coerces the **box** to
/// `&dyn Any` instead of dereferencing to the value inside it, and every
/// `downcast_ref` then misses. This code had exactly that bug, and it was
/// invisible until the native integration tests provoked a real component
/// panic and read the message back: every caught panic reported
/// "component panicked with a non-string payload", discarding the one piece
/// of diagnostic information a caught panic carries.
/// The development error overlay: the panic and its source position, in a
/// window owned by the application's.
fn show_dev_error(owner: HWND, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let text = super::util::wide(format!("A component panicked.\n\n{text}"));
    let title = super::util::wide("RustNative — development error");
    // SAFETY: both strings are NUL-terminated wide buffers alive for the
    // call; a stale or null owner is accepted.
    unsafe { MessageBoxW(owner, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
}

pub(crate) fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "component panicked with a non-string payload".to_string()
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    wndproc_boundary(hwnd, move || window_proc_impl(hwnd, message, wparam, lparam))
}

#[allow(
    clippy::too_many_lines,
    reason = "the window procedure is one dispatch table: an arm per message, each delegating"
)]
fn window_proc_impl(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Delegating to the default window procedure, which every arm below
    // that declines to handle a message falls back to.
    //
    // SAFETY: `hwnd`/`message`/`wparam`/`lparam` are exactly what Win32
    // just delivered this callback with; `DefWindowProcW`'s documented
    // default handling is valid for any window message.
    let default = || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };

    match message {
        // A framework window's first paint is the startup model's "first
        // content" (`rustnative_core::perf`, `PLAN.md` Milestone 42).
        WM_PAINT => {
            rustnative_core::perf::mark(rustnative_core::perf::StartupPhase::FirstContent);
            default()
        }
        // The window moved to a monitor of another DPI (or the DPI changed
        // under it): take the size Windows suggests, and tell every
        // surface its new scale — the surface hand-off contract
        // (`docs/interop/surface-handoff.md`).
        WM_DPICHANGED => {
            dpi_changed(hwnd, wparam, lparam);
            0
        }
        // Under this backend's own loop the wake is handled before dispatch
        // (`handle_message`); under a host's loop (`ExternalLoop`,
        // `EmbeddedRoot`) it arrives here.
        WM_FRAMEWORK_SCHEDULE => {
            with_runtime(hwnd, |runtime| {
                if let Err(error) = runtime.pump_tasks() {
                    runtime.error = Some(error);
                    super::embed::request_quit(1);
                }
            });
            0
        }
        WM_NCCREATE => {
            let create = lparam as *const CREATESTRUCTW;
            if create.is_null() {
                return 0;
            }

            // SAFETY: `create` was just checked non-null and, for a
            // window created through `create_window_once`, points at a
            // `CREATESTRUCTW` whose `lpCreateParams` Win32 forwards
            // unchanged from the `lpParam` given to `CreateWindowExW` —
            // here, `runtime_ptr.cast()`, a pointer into the
            // `Box<Runtime>` owned by `WindowRegistry::runtimes` for as
            // long as this window exists (see `WindowRegistry`'s type
            // doc comment: a `Runtime` outlives its own `WM_DESTROY`
            // and is only ever dropped after `run_application`'s
            // message loop returns).
            let runtime_ptr = unsafe { (*create).lpCreateParams }.cast::<Runtime>();
            // SAFETY: `hwnd` was just created by Win32 and is being
            // delivered its first message (`WM_NCCREATE` is always
            // first); storing this pointer here is exactly what
            // establishes the invariant every `native::context`
            // resolution in this crate relies on.
            unsafe {
                RuntimeSlot::set(hwnd, runtime_ptr);
            }
            1
        }
        WM_COMMAND => {
            with_runtime(hwnd, |runtime| {
                let notification_code = u32::from(super::util::hiword(wparam));
                let control = lparam as HWND;
                if control.is_null() {
                    // A native menu command: no control window is
                    // associated with it (`lParam` is 0), unlike a control
                    // notification, and its notification code is 0.
                    if notification_code == 0 {
                        menu_command(runtime, super::util::loword(wparam));
                    }
                } else if let Some(id) = runtime.renderer.registry.id_for_hwnd(control) {
                    control_notification(runtime, id, notification_code, control);
                }
            });
            0
        }
        super::single_instance::WM_FRAMEWORK_DEEP_LINK
        | super::memory_watch::WM_FRAMEWORK_LOW_MEMORY
        | WM_QUERYENDSESSION
        | WM_ENDSESSION
        | WM_POWERBROADCAST => session_message(hwnd, message, wparam),
        super::surfaces::WM_FRAMEWORK_TRAY => {
            // Version 4 callbacks: the event is the low word of `lParam`.
            #[allow(
                clippy::cast_sign_loss,
                reason = "the low word is read as the bits Windows packed"
            )]
            let event = u32::from(super::util::loword(lparam as usize));
            with_runtime(hwnd, |runtime| super::surfaces::tray_callback(runtime, event));
            0
        }
        WM_NOTIFY => {
            // SAFETY: `WM_NOTIFY`'s `lParam` is a pointer to an `NMHDR`
            // (the header every notification structure starts with), valid
            // for the duration of this sent message.
            let header = unsafe { &*(lparam as *const NMHDR) };
            if header.code == TCN_SELCHANGE {
                with_runtime(hwnd, |runtime| tab_selected(runtime, header.hwndFrom));
            } else if matches!(header.code, DTN_DATETIMECHANGE | NM_CLICK | NM_RETURN) {
                let (code, from) = (header.code, header.hwndFrom);
                with_runtime(hwnd, |runtime| control_notify(runtime, code, from));
            }
            default()
        }
        WM_HSCROLL => {
            slider_moved(hwnd, lparam as HWND);
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN => {
            with_runtime(hwnd, |runtime| control_color(runtime, wparam, lparam))
                .flatten()
                .unwrap_or_else(default)
        }
        WM_SIZE => {
            with_runtime(hwnd, |runtime| {
                let size = rustnative_core::Size::new(
                    u32::from(super::util::loword_signed(lparam)),
                    u32::from(super::util::hiword_signed(lparam)),
                );
                let window = runtime.window_id;
                if !runtime.dispatch_or_quit(Event::WindowResized { window, size }) {
                    return;
                }
                // `wParam` for `WM_SIZE` documents its resize-type flag
                // (`SIZE_MINIMIZED` = 1, `SIZE_MAXIMIZED` = 2, ...) as a
                // small constant that always fits in `u32`; a value large
                // enough to truncate here would itself already indicate
                // something has gone fundamentally wrong upstream, not a
                // case this truncation could meaningfully guard against.
                #[allow(clippy::cast_possible_truncation)]
                let state = match wparam as u32 {
                    1 => rustnative_core::WindowPresentation::Minimized,
                    2 => rustnative_core::WindowPresentation::Maximized,
                    _ => rustnative_core::WindowPresentation::Normal,
                };
                if runtime.dispatch_or_quit(Event::WindowStateChanged { window, state }) {
                    runtime.relayout();
                }
            });
            0
        }
        WM_MOVE => {
            with_runtime(hwnd, |runtime| {
                // `WM_MOVE`'s `lParam` packs the window's signed x/y
                // position (negative on a multi-monitor setup with a
                // monitor to the left of/above the primary one) as two
                // 16-bit fields; reinterpreting each extracted `u16` as
                // `i16` is exactly that documented bit pattern, not a
                // truncating or wrapping value change.
                #[allow(clippy::cast_possible_wrap)]
                let position = rustnative_core::Point::new(
                    i32::from(super::util::loword_signed(lparam) as i16),
                    i32::from(super::util::hiword_signed(lparam) as i16),
                );
                let window = runtime.window_id;
                runtime.dispatch_or_quit(Event::WindowMoved { window, position });
                // An owned popup does not follow its owner.
                super::inspect::sync_overlay(runtime);
            });
            0
        }
        WM_CLOSE => {
            let destroy = with_runtime(hwnd, |runtime| {
                let window = runtime.window_id;
                if !runtime.dispatch_or_quit(Event::WindowCloseRequested { window }) {
                    // The dispatch already recorded the error and posted
                    // `WM_QUIT`; tearing the window down on top of that
                    // would run destruction during an error unwind.
                    return false;
                }
                // The primary window's close is the application's own exit
                // path, handled by `WM_DESTROY` posting `WM_QUIT`; any
                // other window must additionally be removed from the
                // application's window set so `WindowRegistry::sync` does
                // not immediately recreate it.
                if window != rustnative_core::WindowId::PRIMARY {
                    runtime.with_application(|application| application.close_window(window));
                }
                true
            });
            // A window with no runtime published yet is still a real window
            // that asked to close, so honor it.
            if destroy.unwrap_or(true) {
                // SAFETY: `hwnd` is the HWND Win32 just invoked this
                // callback with, and is still live.
                //
                // Best effort: the documented failure is `hwnd` not being
                // a window this thread owns, which cannot be true inside
                // its own `WNDPROC`, and there is nothing left to report a
                // failure to on a teardown path.
                let destroyed = unsafe { DestroyWindow(hwnd) } != 0;
                best_effort(destroyed, "DestroyWindow", "this is the window's own teardown path");
            }
            0
        }
        WM_TIMER => {
            match wparam {
                LONG_PRESS_TIMER_ID => {
                    with_runtime(hwnd, input::pointer::long_press_timer);
                }
                super::lifecycle::FLUSH_TIMER_ID => {
                    with_runtime(hwnd, |runtime| super::lifecycle::idle_flush(runtime));
                }
                input::gamepad::GAMEPAD_TIMER_ID => {
                    with_runtime(hwnd, input::gamepad::poll);
                }
                _ => return default(),
            }
            0
        }
        WM_CAPTURECHANGED => {
            // Sent synchronously from inside `SetCapture`/`ReleaseCapture`,
            // which `native::input::pointer` calls while this window's
            // runtime is borrowed — so do not resolve it here; re-post, and
            // let the posted message check the real capture state.
            // SAFETY: `hwnd` is the live window this callback is for.
            let posted = unsafe { PostMessageW(hwnd, WM_FRAMEWORK_CAPTURE_LOST, 0, 0) } != 0;
            best_effort(posted, "PostMessageW(capture lost)", "a lost capture ends at release");
            0
        }
        super::animation::WM_FRAMEWORK_FRAME => {
            with_runtime(hwnd, super::animation::frame);
            0
        }
        windows_sys::Win32::UI::WindowsAndMessaging::WM_SETCURSOR
        | windows_sys::Win32::UI::WindowsAndMessaging::WM_INITMENUPOPUP
        | WM_SETTINGCHANGE
        | WM_DWMCOLORIZATIONCOLORCHANGED => shell_message(hwnd, message, wparam, lparam),
        super::uia::WM_FRAMEWORK_UIA => {
            // Drained inside the borrow, raised after it ends — see
            // `native::uia`'s module docs for why raising must not happen
            // while the runtime is borrowed.
            if let Some(ready) = with_runtime(hwnd, super::uia::take_ready) {
                super::uia::raise(ready);
            }
            0
        }
        WM_FRAMEWORK_CAPTURE_LOST => {
            with_runtime(hwnd, input::pointer::capture_lost);
            0
        }
        clipboard::WM_CLIPBOARDUPDATE => {
            // Re-posted for the reason documented on
            // `WM_FRAMEWORK_CLIPBOARD_CHANGED`.
            // SAFETY: `hwnd` is the live window this callback is for.
            let posted =
                unsafe { PostMessageW(hwnd, clipboard::WM_FRAMEWORK_CLIPBOARD_CHANGED, 0, 0) } != 0;
            best_effort(posted, "PostMessageW(clipboard)", "one clipboard change goes unreported");
            0
        }
        clipboard::WM_FRAMEWORK_CLIPBOARD_CHANGED => {
            with_runtime(hwnd, clipboard::changed);
            0
        }
        WM_DESTROY => {
            // Saved while the window still has a placement to read.
            with_runtime(hwnd, |runtime| super::lifecycle::save_placement(runtime));
            // The tray icon goes with its window.
            with_runtime(hwnd, super::surfaces::teardown);
            // UI Automation is released first, and its disconnections raised
            // outside the runtime borrow, like every other call into it.
            if let Some(ready) = with_runtime(hwnd, super::uia::release_window) {
                super::uia::raise(ready);
            }
            with_runtime(hwnd, |runtime| {
                runtime.destroyed = true;
                // Before Windows destroys the children with this window.
                runtime.renderer.registry.release_borrowed_foreign();
                super::animation::release(runtime);
                input::drop_target::revoke(runtime);
                clipboard::stop_listening(runtime);
                for timer in [
                    LONG_PRESS_TIMER_ID,
                    input::gamepad::GAMEPAD_TIMER_ID,
                    super::lifecycle::FLUSH_TIMER_ID,
                ] {
                    // SAFETY: `hwnd` is live inside its own `WM_DESTROY`;
                    // killing a timer that was never set is harmless.
                    ignored_by_contract(unsafe { KillTimer(hwnd, timer) });
                }
                if runtime.input.pointer.captured(MOUSE_POINTER_ID).is_some() {
                    // The window is going away; nothing more to cancel to.
                    runtime.input.pointer = input::pointer::PointerState::default();
                }
                // Stop advertising this window as a dialog-owner candidate
                // before it actually stops existing — see
                // `native::window_handles`'s module doc comment.
                super::window_handles::clear(runtime.window_id);
                if !runtime.modal_parent.is_null() {
                    // SAFETY: `modal_parent` was just checked non-null
                    // and, per `create_window_once`, is a live HWND from
                    // `WindowRegistry::runtimes` that owns this window as
                    // a modal child.
                    //
                    // `EnableWindow` reports the window's *previous*
                    // disabled state, not whether the call worked.
                    ignored_by_contract(unsafe { EnableWindow(runtime.modal_parent, 1) });
                }
                // An embedded root going away ends nothing but itself.
                if runtime.window_id == rustnative_core::WindowId::PRIMARY && !runtime.embedded {
                    super::embed::request_quit(0);
                }
            });
            0
        }
        _ => default(),
    }
}

/// Routes a native menu selection back to the component tree.
///
/// A command id with no entry in `menu_commands` is ignored rather than
/// treated as an error: Windows itself sends `WM_COMMAND` for system menu
/// items (Close, Minimize, ...) this crate never registered.
fn menu_command(runtime: &mut Runtime, command_id: u16) {
    let Some(item) = runtime.menu_commands.get(&command_id).copied() else {
        return;
    };
    let window = runtime.window_id;
    runtime.dispatch_or_quit(Event::MenuAction { window, item });
}

/// Routes a native control's notification (a button click, an edit
/// control's text change) back to the component that owns the control.
/// The window's conversation with the desktop shell: the cursor over it,
/// a menu about to open, and a system setting changing.
fn shell_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: exactly what Win32 delivered this callback with; the default
    // procedure is valid for any message.
    let default = || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    match message {
        windows_sys::Win32::UI::WindowsAndMessaging::WM_SETCURSOR => {
            // The hit-test code is the low word of `lParam`.
            let hit_test = u32::try_from(lparam & 0xFFFF).unwrap_or(0);
            let set = with_runtime(hwnd, |runtime| {
                super::cursor::set_cursor(runtime, wparam as HWND, hit_test)
            })
            .unwrap_or(false);
            if set { 1 } else { default() }
        }
        windows_sys::Win32::UI::WindowsAndMessaging::WM_INITMENUPOPUP => {
            with_runtime(hwnd, |runtime| {
                let focused = focused_node(runtime);
                super::commands::init_menu_popup(
                    runtime,
                    wparam as windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
                    focused,
                );
            });
            default()
        }
        _ => {
            // `WM_SETTINGCHANGE`: re-read every host trait this backend
            // follows. Unchanged values invalidate nothing.
            with_runtime(hwnd, |runtime| {
                super::animation::sync_motion_preference(runtime);
                let traits = super::host_traits::read();
                let mode = super::host_traits::window_mode(runtime.window);
                let window = runtime.window_id;
                runtime.with_application(|application| {
                    super::host_traits::apply(application, &traits);
                    let _ = application.set_window_environment(
                        window,
                        &rustnative_core::keys::WINDOW_MODE,
                        mode,
                    );
                });
                let rendered = runtime.render();
                super::win32::best_effort(
                    rendered.is_ok(),
                    "render(settings changed)",
                    "the window shows the previous settings until its next render",
                );
            });
            default()
        }
    }
}

/// Messages about the application's life rather than about one window:
/// deep links handed over by a second launch, the session ending, and the
/// machine sleeping or waking (see `native::lifecycle`).
fn session_message(hwnd: HWND, message: u32, wparam: WPARAM) -> LRESULT {
    match message {
        super::single_instance::WM_FRAMEWORK_DEEP_LINK => {
            with_runtime(hwnd, |runtime| {
                for url in super::single_instance::take_pending() {
                    if !runtime.dispatch_or_quit(Event::DeepLink { url }) {
                        return;
                    }
                }
            });
            0
        }
        super::memory_watch::WM_FRAMEWORK_LOW_MEMORY => {
            with_runtime(hwnd, |runtime| {
                if super::lifecycle::is_primary(runtime) {
                    super::lifecycle::notify(runtime, rustnative_core::Lifecycle::LowMemory);
                }
            });
            0
        }
        WM_QUERYENDSESSION => {
            // The person is signing out or shutting down. Windows may end
            // the process at any moment after this returns, so state is
            // written now, and the application told.
            with_runtime(hwnd, |runtime| {
                if super::lifecycle::is_primary(runtime) {
                    super::lifecycle::notify(runtime, rustnative_core::Lifecycle::Terminating);
                }
            });
            1
        }
        WM_ENDSESSION => {
            // Anything written in answer to `Terminating` is flushed too.
            if wparam != 0 {
                with_runtime(hwnd, |runtime| super::lifecycle::idle_flush(runtime));
            }
            0
        }
        WM_POWERBROADCAST => {
            with_runtime(hwnd, |runtime| {
                if super::lifecycle::is_primary(runtime) {
                    super::lifecycle::power_broadcast(runtime, u32::try_from(wparam).unwrap_or(0));
                }
            });
            1
        }
        _ => 0,
    }
}

/// The person chose a tab: report it, then make the control show whatever
/// the component decided (see `rendering::tabs` on why the control is
/// controlled).
fn tab_selected(runtime: &mut Runtime, control: HWND) {
    let Some(id) = runtime.renderer.registry.id_for_hwnd(control) else {
        return;
    };
    let Some(index) = super::rendering::tabs::selection(control) else {
        return;
    };
    if !runtime.dispatch_or_quit(Event::TabSelected { target: id, index }) {
        return;
    }
    let selected = runtime.renderer.snapshot.get(id).and_then(|node| node.tabs.as_ref());
    if let Some(tabs) = selected {
        if tabs.selected() != index {
            super::rendering::tabs::select(control, tabs.selected());
        }
    }
}

/// A person's change to a native control (`rendering::native_controls`):
/// report it, then put the control back to whatever the component decided.
fn control_changed(runtime: &mut Runtime, id: rustnative_core::NodeId, event: Event) {
    if !runtime.dispatch_or_quit(event) {
        return;
    }
    resync_control(runtime, id);
}

fn resync_control(runtime: &mut Runtime, id: rustnative_core::NodeId) {
    let Some(NativeObject::Control { hwnd, companion, .. }) = runtime.renderer.registry.get(id)
    else {
        return;
    };
    let (hwnd, companion) = (*hwnd, *companion);
    if let Some(control) = runtime.renderer.snapshot.get(id).and_then(|node| node.control.clone()) {
        // Forget what was shown: the person changed the control since.
        super::rendering::native_controls::forget(hwnd);
        let edited = matches!(
            control,
            rustnative_core::Control::Spinner { .. }
                | rustnative_core::Control::MultilineText { .. }
        );
        let before = window_text(hwnd);
        super::rendering::native_controls::sync(hwnd, companion, &control);
        if edited && window_text(hwnd) != before {
            runtime.renderer.suppress_text_change.insert(id);
        }
    }
}

/// The person changed their accent color (`WM_DWMCOLORIZATIONCOLORCHANGED`):
/// handled with the other settings changes, which re-read the host palette.
const WM_DWMCOLORIZATIONCOLORCHANGED: u32 = 0x0320;

/// A trackbar moved (a scroll bar sends no control handle).
fn slider_moved(hwnd: HWND, control: HWND) {
    if control.is_null() {
        return;
    }
    with_runtime(hwnd, |runtime| {
        if let Some(id) = runtime.renderer.registry.id_for_hwnd(control) {
            let value = super::rendering::native_controls::slider_position(control);
            control_changed(runtime, id, Event::ValueChanged { target: id, value });
        }
    });
}

/// A `WM_NOTIFY` from a native control: a date picked, a link followed.
fn control_notify(runtime: &mut Runtime, code: u32, control: HWND) {
    let Some(id) = runtime.renderer.registry.id_for_hwnd(control) else { return };
    if code == DTN_DATETIMECHANGE {
        if let Some(date) = super::rendering::native_controls::date(control) {
            control_changed(runtime, id, Event::DateChanged { target: id, date });
        }
    } else {
        runtime.dispatch_or_quit(Event::Click { target: id });
    }
}

/// A `WM_COMMAND` from a native control; `true` when it was one.
fn native_control_command(
    runtime: &mut Runtime,
    id: rustnative_core::NodeId,
    notification_code: u32,
    control: HWND,
) -> bool {
    let Some(NativeObject::Control { tag, .. }) = runtime.renderer.registry.get(id) else {
        return false;
    };
    let tag = *tag;
    let event = match (tag, notification_code) {
        ("check", BN_CLICKED) => {
            // A manual check box does not change itself: the new state is
            // the opposite of what it shows.
            Some(Event::Toggled {
                target: id,
                on: !super::rendering::native_controls::is_checked(control),
            })
        }
        ("radio", BN_CLICKED) => Some(Event::Toggled { target: id, on: true }),
        // A link realized as clickable text (no Common Controls 6).
        ("link", BN_CLICKED) => {
            runtime.dispatch_or_quit(Event::Click { target: id });
            return true;
        }
        ("combobox", CBN_SELCHANGE) => Some(Event::SelectionChanged {
            target: id,
            index: super::rendering::native_controls::selection(control, false),
        }),
        ("listbox", LBN_SELCHANGE) => Some(Event::SelectionChanged {
            target: id,
            index: super::rendering::native_controls::selection(control, true),
        }),
        ("spinner", EN_CHANGE) => {
            if runtime.renderer.suppress_text_change.remove(&id) {
                return true;
            }
            window_text(control)
                .trim()
                .parse::<i64>()
                .ok()
                .map(|value| Event::ValueChanged { target: id, value })
        }
        ("multiline", EN_CHANGE) => {
            if runtime.renderer.suppress_text_change.remove(&id) {
                return true;
            }
            // The text is the person's own: not written back, which would
            // move the caret.
            runtime
                .dispatch_or_quit(Event::TextChanged { target: id, value: window_text(control) });
            return true;
        }
        _ => None,
    };
    if let Some(event) = event {
        control_changed(runtime, id, event);
    }
    true
}

fn control_notification(
    runtime: &mut Runtime,
    id: rustnative_core::NodeId,
    notification_code: u32,
    control: HWND,
) {
    if native_control_command(runtime, id, notification_code, control) {
        return;
    }
    if notification_code == BN_CLICKED {
        runtime.dispatch_or_quit(Event::Click { target: id });
        return;
    }
    if notification_code != EN_CHANGE {
        return;
    }
    // A text change this renderer itself just wrote via `SetWindowTextW`,
    // not one the person typed — echoing it back as an event would loop.
    if runtime.renderer.suppress_text_change.remove(&id) {
        return;
    }
    let is_text_input = runtime
        .renderer
        .registry
        .get(id)
        .is_some_and(|object| matches!(object, NativeObject::TextInput(_)));
    if !is_text_input {
        return;
    }
    let value = window_text(control);
    runtime.dispatch_or_quit(Event::TextChanged { target: id, value });
}

/// Answers a `WM_CTLCOLOR*` message with the resolved foreground/background
/// of the control it concerns, returning `None` when this crate has no
/// realized style for that control and Win32's default painting should
/// stand.
fn control_color(runtime: &Runtime, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    let control = lparam as HWND;
    let hdc = wparam as HDC;
    let id = runtime.renderer.registry.id_for_hwnd(control)?;
    let style = runtime.renderer.styles.get(id)?;
    // SAFETY: `hdc` is the `HDC` Win32 passed via `wparam` for this
    // control-color message, valid for the duration of this callback;
    // `TRANSPARENT` is a documented mode constant, not a pointer.
    //
    // All three setters return the *previous* color/mode rather than a
    // status code.
    unsafe {
        informational(SetTextColor(hdc, style.foreground));
        informational(SetBkColor(hdc, style.background));
        // `TRANSPARENT` is a small, fixed Win32 background-mode constant
        // (value `1`); it can never approach `i32::MAX`.
        #[allow(clippy::cast_possible_wrap)]
        let transparent_mode = TRANSPARENT as i32;
        informational(SetBkMode(hdc, transparent_mode));
    }
    Some(style.background_brush as LRESULT)
}
