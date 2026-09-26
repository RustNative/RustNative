//! Standalone reproduction of the `DeleteObject`/font anomaly found while
//! debugging `rustnative-windows`'s `ControlStyle` GDI tests.
//!
//! Deliberately has no dependency on this workspace's own crates and is a
//! plain binary, not a `cargo test` target — the point is to run in a
//! process of its own, so that anything specific to the test-harness
//! process (a debugger, an injected accessibility/overlay/AV hook on
//! `gdi32.dll`, etc.) is ruled out rather than assumed away.
//!
//! Run with `cargo run -p gdi-font-diagnostic`. It prints this process's
//! ID first so you can also watch it in Task Manager's "Details" tab
//! (enable the "GDI objects" column) or in Process Explorer while it's
//! paused at the end, before pressing Enter to let it exit.

use std::io::Read;

use windows_sys::Win32::Foundation::COLORREF;
use windows_sys::Win32::Graphics::Gdi::{
    CLIP_DEFAULT_PRECIS, CreateFontIndirectW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH,
    DEFAULT_QUALITY, DeleteObject, FF_DONTCARE, GetObjectType, HGDIOBJ, LOGFONTW,
    OUT_DEFAULT_PRECIS,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn wide_face_name(family: &str) -> [u16; 32] {
    let mut face_name = [0u16; 32];
    let encoded: Vec<u16> = family.encode_utf16().take(31).collect();
    face_name[..encoded.len()].copy_from_slice(&encoded);
    face_name
}

/// Returns the created font already cast to `HGDIOBJ`, the same way
/// `rustnative-windows`'s own `ControlStyle::drop` casts `HFONT`/`HBRUSH`
/// before calling `DeleteObject` — so this reproduction goes through the
/// exact same cast path as the code under investigation.
fn create_font(family: &str, size: i32, weight: i32) -> HGDIOBJ {
    let logfont = LOGFONTW {
        lfHeight: -size,
        lfWidth: 0,
        lfEscapement: 0,
        lfOrientation: 0,
        lfWeight: weight,
        lfItalic: 0,
        lfUnderline: 0,
        lfStrikeOut: 0,
        lfCharSet: DEFAULT_CHARSET,
        lfOutPrecision: OUT_DEFAULT_PRECIS,
        lfClipPrecision: CLIP_DEFAULT_PRECIS,
        lfQuality: DEFAULT_QUALITY,
        lfPitchAndFamily: DEFAULT_PITCH | FF_DONTCARE,
        lfFaceName: wide_face_name(family),
    };
    // SAFETY: `logfont` is fully initialized and exclusively borrowed for
    // the duration of this call; `lfFaceName` is null-terminated by
    // construction (`wide_face_name` starts zeroed and writes at most 31
    // of its 32 slots).
    unsafe { CreateFontIndirectW(&raw const logfont) as HGDIOBJ }
}

/// Reports `GetObjectType` on `handle`, deletes it, reports
/// `DeleteObject`'s return value, and reports `GetObjectType` again —
/// printing every step under `label`.
fn probe(label: &str, handle: HGDIOBJ) {
    // SAFETY: `handle` was just returned by a `Create*` call the caller
    // made immediately before calling `probe`, so it is either a live GDI
    // handle or null; `GetObjectType`/`DeleteObject` both accept any
    // `HGDIOBJ` value, including null, and report failure for it rather
    // than requiring the caller to pre-validate.
    let type_before = unsafe { GetObjectType(handle) };
    // SAFETY: same handle, never selected into any DC, deleted here
    // exactly once.
    let deleted = unsafe { DeleteObject(handle) };
    // SAFETY: inspecting validity only, regardless of what `DeleteObject`
    // just reported.
    let type_after = unsafe { GetObjectType(handle) };
    let still_valid = if type_after != 0 { "  <-- STILL VALID" } else { "" };
    println!(
        "{label}: type_before={type_before}, DeleteObject returned {deleted}, \
         type_after={type_after}{still_valid}"
    );
}

fn main() {
    // SAFETY: takes no arguments and cannot fail.
    let pid = unsafe { GetCurrentProcessId() };
    println!(
        "PID: {pid} — check Task Manager/Process Explorer's GDI-objects column for this PID now."
    );
    println!();

    // SAFETY: plain COLORREF value, no pointer arguments.
    let brush = unsafe { CreateSolidBrush(0x001E_140A as COLORREF) as HGDIOBJ };
    probe("brush (known-good baseline)", brush);
    probe(
        "font: Segoe UI, size 10, weight 700 (the failing case)",
        create_font("Segoe UI", 10, 700),
    );
    probe(
        "font: Segoe UI, size 10, weight 400 (regular, not bold)",
        create_font("Segoe UI", 10, 400),
    );
    probe("font: Arial, size 10, weight 700 (different family)", create_font("Arial", 10, 700));
    probe("font: Segoe UI, size 24, weight 700 (different size)", create_font("Segoe UI", 24, 700));

    println!();
    println!("Press Enter to exit (so you can inspect this process's handle count first)...");
    let _ = std::io::stdin().read(&mut [0u8]);
}
