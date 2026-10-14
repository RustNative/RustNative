//! The process's terminal on Windows: the console API in virtual-terminal
//! mode (`PLAN.md` Milestone 38, "the Windows console API in
//! virtual-terminal mode").
//!
//! Input is read as console input records. With
//! `ENABLE_VIRTUAL_TERMINAL_INPUT`, the console (conhost, or ConPTY under
//! Windows Terminal) translates keys, mouse reports, and paste into the same
//! VT sequences a Unix terminal sends, so one decoder serves both; window
//! size records are resize notifications, and focus records become the
//! focus sequences. Output is UTF-8 with `ENABLE_VIRTUAL_TERMINAL_PROCESSING`.
//!
//! # Restoration
//!
//! The input and output modes and code pages are saved first and restored
//! by [`restore_now`] — from `Drop`, the panic hook, and the console control
//! handler (Ctrl+Break, the window closing, logoff, shutdown), which runs on
//! a thread of its own: it signals the loop, gives it a moment to end
//! cleanly, and restores whatever is left before Windows ends the process.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, WriteFile,
};
use windows_sys::Win32::System::Console::{
    CONSOLE_MODE, CONSOLE_SCREEN_BUFFER_INFO, CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT,
    DISABLE_NEWLINE_AUTO_RETURN, ENABLE_EXTENDED_FLAGS, ENABLE_PROCESSED_OUTPUT,
    ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
    FOCUS_EVENT, GetConsoleCP, GetConsoleMode, GetConsoleOutputCP, GetConsoleScreenBufferInfo,
    GetNumberOfConsoleInputEvents, INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, SetConsoleCP,
    SetConsoleCtrlHandler, SetConsoleMode, SetConsoleOutputCP, WINDOW_BUFFER_SIZE_EVENT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};

use super::{Readiness, Signal, Terminal, Waker};
use crate::encode::seq;

const CP_UTF8: u32 = 65_001;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static INPUT: AtomicUsize = AtomicUsize::new(0);
static OUTPUT: AtomicUsize = AtomicUsize::new(0);
static WAKE: AtomicUsize = AtomicUsize::new(0);
static SAVED_INPUT_MODE: AtomicU32 = AtomicU32::new(0);
static SAVED_OUTPUT_MODE: AtomicU32 = AtomicU32::new(0);
static SAVED_INPUT_CP: AtomicU32 = AtomicU32::new(0);
static SAVED_OUTPUT_CP: AtomicU32 = AtomicU32::new(0);
/// Control events not yet seen by the loop: bit 0 terminate, bit 1 hang up.
static PENDING: AtomicU32 = AtomicU32::new(0);

fn handle(value: &AtomicUsize) -> HANDLE {
    value.load(Ordering::Acquire) as HANDLE
}

/// Puts the console back as it was found, once; later calls do nothing
/// until it is taken again.
pub(crate) fn restore_now() {
    if !ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }
    let (input, output) = (handle(&INPUT), handle(&OUTPUT));
    let bytes = seq::RESTORE.as_bytes();
    let mut written = 0_u32;
    // SAFETY: the handles are the console's, opened by `ProcessTerminal::open`
    // and closed only after this has run; the buffer is a static string.
    unsafe {
        WriteFile(
            output,
            bytes.as_ptr(),
            u32::try_from(bytes.len()).unwrap_or(0),
            &raw mut written,
            std::ptr::null_mut(),
        );
        SetConsoleMode(output, SAVED_OUTPUT_MODE.load(Ordering::Acquire));
        SetConsoleMode(input, SAVED_INPUT_MODE.load(Ordering::Acquire));
        SetConsoleOutputCP(SAVED_OUTPUT_CP.load(Ordering::Acquire));
        SetConsoleCP(SAVED_INPUT_CP.load(Ordering::Acquire));
    }
}

unsafe extern "system" fn on_control(event: u32) -> windows_sys::core::BOOL {
    let bit = match event {
        // With processed input off Ctrl+C arrives as a key; this is only
        // reached through `GenerateConsoleCtrlEvent`.
        CTRL_C_EVENT | CTRL_BREAK_EVENT => 1,
        _ => 2,
    };
    PENDING.fetch_or(bit, Ordering::AcqRel);
    // SAFETY: the wake event is open while a session runs.
    unsafe { SetEvent(handle(&WAKE)) };
    if event == CTRL_CLOSE_EVENT || bit == 2 {
        // Windows ends the process when this returns (after about five
        // seconds at most): let the loop end and restore, then make sure.
        let started = Instant::now();
        while ACTIVE.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        restore_now();
    }
    1
}

fn open_console(name: &str) -> io::Result<HANDLE> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated wide string; no security attributes or
    // template.
    let opened = unsafe {
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
    if opened == INVALID_HANDLE_VALUE || opened.is_null() {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no console: {name} could not be opened ({})", io::Error::last_os_error()),
        ))
    } else {
        Ok(opened)
    }
}

/// An event handle closed when its last holder lets go, so a waker that
/// outlives the terminal never signals a reused handle.
#[derive(Debug)]
struct Event(usize);

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: the event this value created; nothing else closes it.
        unsafe { CloseHandle(self.0 as HANDLE) };
    }
}

/// The process's console, in virtual-terminal raw mode while this value
/// lives.
#[derive(Debug)]
pub struct ProcessTerminal {
    input: HANDLE,
    output: HANDLE,
    wake: HANDLE,
    event: Arc<Event>,
    high_surrogate: Option<u16>,
}

impl ProcessTerminal {
    pub(crate) fn open() -> io::Result<Self> {
        if ACTIVE.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the console is already in use by a session",
            ));
        }
        let input = open_console("CONIN$")?;
        let output = open_console("CONOUT$")?;
        let (mut input_mode, mut output_mode): (CONSOLE_MODE, CONSOLE_MODE) = (0, 0);
        // SAFETY: valid console handles; the modes are written.
        let ok = unsafe {
            GetConsoleMode(input, &raw mut input_mode) != 0
                && GetConsoleMode(output, &raw mut output_mode) != 0
        };
        if !ok {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: no attributes, auto-reset, unsignalled, unnamed.
        let wake = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if wake.is_null() {
            return Err(io::Error::last_os_error());
        }
        SAVED_INPUT_MODE.store(input_mode, Ordering::Release);
        SAVED_OUTPUT_MODE.store(output_mode, Ordering::Release);
        // SAFETY: reading the code pages has no preconditions.
        SAVED_INPUT_CP.store(unsafe { GetConsoleCP() }, Ordering::Release);
        // SAFETY: as above.
        SAVED_OUTPUT_CP.store(unsafe { GetConsoleOutputCP() }, Ordering::Release);
        INPUT.store(input as usize, Ordering::Release);
        OUTPUT.store(output as usize, Ordering::Release);
        WAKE.store(wake as usize, Ordering::Release);
        PENDING.store(0, Ordering::Release);
        // Armed before anything is changed.
        ACTIVE.store(true, Ordering::Release);
        crate::guard::install_panic_hook();
        let terminal = Self {
            input,
            output,
            wake,
            event: Arc::new(Event(wake as usize)),
            high_surrogate: None,
        };
        // SAFETY: valid console handles and documented mode bits.
        unsafe {
            let raw_output = output_mode
                | ENABLE_PROCESSED_OUTPUT
                | ENABLE_VIRTUAL_TERMINAL_PROCESSING
                | DISABLE_NEWLINE_AUTO_RETURN;
            if SetConsoleMode(output, raw_output) == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "this console does not process virtual-terminal sequences (Windows 10 1607 or later is needed)",
                ));
            }
            // Line input, echo, processed input (Ctrl+C as a signal), mouse
            // records, and quick edit are all off: input is VT sequences.
            let raw_input =
                ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_WINDOW_INPUT | ENABLE_EXTENDED_FLAGS;
            if SetConsoleMode(input, raw_input) == 0 {
                return Err(io::Error::last_os_error());
            }
            SetConsoleOutputCP(CP_UTF8);
            SetConsoleCP(CP_UTF8);
            SetConsoleCtrlHandler(Some(on_control), 1);
        }
        Ok(terminal)
    }

    /// The console's current input mode, for tests that read it back.
    #[must_use]
    pub fn input_mode(&self) -> Option<u32> {
        let mut mode = 0;
        // SAFETY: a valid console handle.
        (unsafe { GetConsoleMode(self.input, &raw mut mode) } != 0).then_some(mode)
    }

    fn push_unit(&mut self, unit: u16, into: &mut Vec<u8>) {
        if (0xD800..0xDC00).contains(&unit) {
            self.high_surrogate = Some(unit);
            return;
        }
        let units: Vec<u16> = match self.high_surrogate.take() {
            Some(high) if (0xDC00..0xE000).contains(&unit) => vec![high, unit],
            _ => vec![unit],
        };
        for character in char::decode_utf16(units) {
            let mut buffer = [0_u8; 4];
            into.extend_from_slice(
                character.unwrap_or('\u{FFFD}').encode_utf8(&mut buffer).as_bytes(),
            );
        }
    }

    fn read_records(&mut self, readiness: &mut Readiness) -> io::Result<()> {
        loop {
            let mut count = 0_u32;
            // SAFETY: a valid console input handle.
            if unsafe { GetNumberOfConsoleInputEvents(self.input, &raw mut count) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if count == 0 {
                return Ok(());
            }
            let mut records: Vec<INPUT_RECORD> =
                vec![INPUT_RECORD::default(); count.min(512) as usize];
            let mut read = 0_u32;
            // SAFETY: the buffer holds `records.len()` records.
            let ok = unsafe {
                ReadConsoleInputW(
                    self.input,
                    records.as_mut_ptr(),
                    u32::try_from(records.len()).unwrap_or(0),
                    &raw mut read,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            for record in &records[..read as usize] {
                match u32::from(record.EventType) {
                    KEY_EVENT => {
                        // SAFETY: the record's type says which union field is set.
                        let key = unsafe { record.Event.KeyEvent };
                        // SAFETY: as above.
                        let unit = unsafe { key.uChar.UnicodeChar };
                        if key.bKeyDown != 0 && unit != 0 {
                            for _ in 0..key.wRepeatCount.max(1) {
                                self.push_unit(unit, &mut readiness.input);
                            }
                        }
                    }
                    WINDOW_BUFFER_SIZE_EVENT => readiness.resized = true,
                    FOCUS_EVENT => {
                        // SAFETY: as above.
                        let focus = unsafe { record.Event.FocusEvent };
                        readiness.input.extend_from_slice(if focus.bSetFocus != 0 {
                            b"\x1b[I"
                        } else {
                            b"\x1b[O"
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Terminal for ProcessTerminal {
    fn size(&self) -> (u16, u16) {
        let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
        // SAFETY: a valid console output handle; `info` is written.
        if unsafe { GetConsoleScreenBufferInfo(self.output, &raw mut info) } == 0 {
            return (80, 25);
        }
        let window = info.srWindow;
        let columns =
            u16::try_from(i32::from(window.Right) - i32::from(window.Left) + 1).unwrap_or(80);
        let rows =
            u16::try_from(i32::from(window.Bottom) - i32::from(window.Top) + 1).unwrap_or(25);
        (columns.max(1), rows.max(1))
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            let chunk = &bytes[offset..bytes.len().min(offset + (1 << 20))];
            let mut written = 0_u32;
            // SAFETY: a valid console output handle and a slice of its length.
            let ok = unsafe {
                WriteFile(
                    self.output,
                    chunk.as_ptr(),
                    u32::try_from(chunk.len()).unwrap_or(0),
                    &raw mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            offset += written as usize;
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "the console accepted nothing",
                ));
            }
        }
        Ok(())
    }

    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<Readiness> {
        let mut readiness = Readiness::default();
        let handles = [self.input, self.wake];
        let milliseconds = timeout.map_or(u32::MAX, |timeout| {
            u32::try_from(timeout.as_millis().max(u128::from(!timeout.is_zero())))
                .unwrap_or(u32::MAX - 1)
        });
        // SAFETY: two valid handles.
        let result = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, milliseconds) };
        if result == WAIT_OBJECT_0 + 1 {
            readiness.woken = true;
        }
        // Records may be waiting whichever handle woke the wait.
        self.read_records(&mut readiness)?;
        let pending = PENDING.swap(0, Ordering::AcqRel);
        if pending & 1 != 0 {
            readiness.signals.push(Signal::Terminate);
            readiness.woken = false;
        }
        if pending & 2 != 0 {
            readiness.signals.push(Signal::Hangup);
            readiness.woken = false;
        }
        Ok(readiness)
    }

    fn waker(&self) -> Waker {
        let event = Arc::clone(&self.event);
        Arc::new(move || {
            // SAFETY: the event stays open while this closure holds it.
            unsafe { SetEvent(event.0 as HANDLE) };
        })
    }

    fn is_windows_console(&self) -> bool {
        true
    }

    fn name(&self) -> String {
        if std::env::var_os("WT_SESSION").is_some() {
            "Windows Terminal".to_owned()
        } else {
            std::env::var("TERM").unwrap_or_else(|_| "Windows console".to_owned())
        }
    }
}

impl Drop for ProcessTerminal {
    fn drop(&mut self) {
        restore_now();
        // SAFETY: removing the handler this value added, and closing the
        // handles it opened.
        unsafe {
            SetConsoleCtrlHandler(Some(on_control), 0);
            CloseHandle(self.input);
            CloseHandle(self.output);
        }
        WAKE.store(0, Ordering::Release);
    }
}
