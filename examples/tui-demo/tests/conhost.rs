//! The demo in a real console window: conhost itself, not ConPTY
//! (`docs/tui.md`, "Verified on").
//!
//! The test starts `cmd /k tui-demo` in a new console, attaches to that
//! console, reads what conhost shows from its screen buffer, and types as a
//! person does — as input records, both the characters a key produces and
//! virtual-key records conhost itself translates for a program in
//! virtual-terminal input mode. When the demo exits, the shell's screen and
//! a visible cursor must be back.
//!
//! It opens a window and takes over this process's console, so it is
//! ignored by default: `cargo test -p tui-demo --test conhost -- --ignored`.

#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, CONSOLE_CURSOR_INFO, CONSOLE_SCREEN_BUFFER_INFO, COORD, FreeConsole,
    GetConsoleCursorInfo, GetConsoleScreenBufferInfo, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
    KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, LEFT_CTRL_PRESSED, ReadConsoleOutputCharacterW,
    SHIFT_PRESSED, WriteConsoleInputW,
};

const CREATE_NEW_CONSOLE: u32 = 0x10;

fn open(name: &str) -> HANDLE {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated name; the handle is closed by the caller.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    assert!(handle != INVALID_HANDLE_VALUE, "open {name}");
    handle
}

/// The console's visible window, row by row.
fn screen() -> String {
    let output = open("CONOUT$");
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: a console output handle; `info` is written.
    unsafe { GetConsoleScreenBufferInfo(output, &raw mut info) };
    let window = info.srWindow;
    let width = u32::try_from(i32::from(window.Right) - i32::from(window.Left) + 1).unwrap_or(80);
    let mut rows = Vec::new();
    for y in window.Top..=window.Bottom {
        let mut buffer = vec![0_u16; width as usize];
        let mut read = 0;
        // SAFETY: a buffer of `width` units.
        unsafe {
            ReadConsoleOutputCharacterW(
                output,
                buffer.as_mut_ptr(),
                width,
                COORD { X: window.Left, Y: y },
                &raw mut read,
            );
        }
        rows.push(String::from_utf16_lossy(&buffer[..read as usize]).trim_end().to_owned());
    }
    // SAFETY: closing what `open` opened.
    unsafe { CloseHandle(output) };
    rows.join("\n")
}

fn cursor_visible() -> bool {
    let output = open("CONOUT$");
    let mut info = CONSOLE_CURSOR_INFO::default();
    // SAFETY: a console output handle.
    unsafe { GetConsoleCursorInfo(output, &raw mut info) };
    // SAFETY: closing what `open` opened.
    unsafe { CloseHandle(output) };
    info.bVisible != 0
}

fn key(down: bool, virtual_key: u16, unit: u16, state: u32) -> INPUT_RECORD {
    INPUT_RECORD {
        EventType: u16::try_from(KEY_EVENT).unwrap_or(1),
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: i32::from(down),
                wRepeatCount: 1,
                wVirtualKeyCode: virtual_key,
                wVirtualScanCode: 0,
                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: unit },
                dwControlKeyState: state,
            },
        },
    }
}

fn send(records: &[INPUT_RECORD]) {
    let input = open("CONIN$");
    let mut written = 0;
    // SAFETY: a console input handle and a slice of records.
    unsafe {
        WriteConsoleInputW(
            input,
            records.as_ptr(),
            u32::try_from(records.len()).unwrap_or(0),
            &raw mut written,
        );
        CloseHandle(input);
    }
    std::thread::sleep(Duration::from_millis(150));
}

/// Types `text`: each unit as a key's character, as a keyboard produces it.
fn type_text(text: &str) {
    let mut records = Vec::new();
    for unit in text.encode_utf16() {
        records.push(key(true, 0, unit, 0));
        records.push(key(false, 0, unit, 0));
    }
    send(&records);
}

/// Presses a virtual key, which conhost translates for a program in
/// virtual-terminal input mode.
fn press(virtual_key: u16, unit: u16, state: u32) {
    send(&[key(true, virtual_key, unit, state), key(false, virtual_key, unit, state)]);
}

fn wait_for(text: &str) -> String {
    let started = Instant::now();
    loop {
        let shown = screen();
        if shown.contains(text) || started.elapsed() > Duration::from_secs(20) {
            return shown;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ends the shell and everything it started, however the test ends, so no
/// process outlives it holding this one's output.
struct Shell(std::process::Child);

impl Drop for Shell {
    fn drop(&mut self) {
        // SAFETY: leaving whatever console this process is attached to.
        unsafe { FreeConsole() };
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &self.0.id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "opens a console window and attaches this process to it"]
fn the_demo_runs_in_a_real_console_window_and_gives_it_back() {
    let exe = env!("CARGO_BIN_EXE_tui-demo");
    let shell = Shell(
        std::process::Command::new("cmd")
            .args(["/k", exe])
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .expect("start cmd in a new console"),
    );
    std::thread::sleep(Duration::from_millis(1500));
    // SAFETY: leaving this process's console and joining the shell's.
    let attached = unsafe {
        FreeConsole();
        AttachConsole(shell.0.id())
    };
    assert_ne!(attached, 0, "attached to the new console");
    let shown = wait_for("To do");
    assert!(shown.contains("To do") && shown.contains("1 of 3 done"), "the demo drew:\n{shown}");
    assert!(shown.contains("[ All ]") || shown.contains(" All "), "{shown}");

    // Tab is a virtual key here: conhost translates it.
    for _ in 0..4 {
        press(0x09, 0x09, 0);
    }
    type_text("Buy milk");
    press(0x0D, 0x0D, 0);
    let shown = wait_for("1 of 4 done");
    assert!(shown.contains("Buy milk"), "typed and added:\n{shown}");

    // Shift+Tab twice to the list. Up is a virtual key with no
    // character: conhost sends its VT sequence.
    for _ in 0..2 {
        press(0x09, 0x09, SHIFT_PRESSED);
    }
    press(0x26, 0, 0); // Up: the task above is chosen.
    let shown = wait_for("Done: Resize the terminal");
    assert!(shown.contains("Done: Resize the terminal"), "Up reached the list:\n{shown}");
    // Tab to the check box; Space checks the chosen task.
    press(0x09, 0x09, 0);
    press(0x20, 0x20, 0);
    let after = wait_for("2 of 4 done");
    assert!(after.contains("2 of 4 done"), "Space checked it:\n{after}");

    // Ctrl+C: the demo closes and the shell is back.
    press(0x43, 0x03, LEFT_CTRL_PRESSED);
    let back = wait_for(">");
    std::thread::sleep(Duration::from_millis(500));
    let back = if back.contains("To do") { screen() } else { back };
    assert!(!back.contains("1 of 4 done"), "the shell's screen is back:\n{back}");
    assert!(cursor_visible(), "the cursor is shown again");
    drop(shell);
}
