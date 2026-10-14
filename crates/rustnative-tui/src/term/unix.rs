//! The process's terminal on Unix: `termios`, `TIOCGWINSZ`, `poll`, and
//! signals through a self-pipe.
//!
//! # Restoration
//!
//! The terminal's original `termios` and file-status flags are saved before
//! anything is changed, and [`restore_now`] puts them back together with
//! [`crate::encode::seq::RESTORE`]. It uses only async-signal-safe calls
//! (`write`, `tcsetattr`, `fcntl`), so the same function serves `Drop`, the
//! panic hook, and a signal handler: a second terminating signal while the
//! loop is stuck restores from the handler and exits with `128 + signal`.

use std::cell::UnsafeCell;
use std::io;
use std::mem::MaybeUninit;
use std::os::raw::c_int;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::time::Duration;

use super::{Readiness, Signal, Terminal, Waker};
use crate::encode::seq;

/// The saved terminal state, written once per session before
/// [`ACTIVE`] is set and read only after it is.
struct Saved(UnsafeCell<MaybeUninit<(libc::termios, c_int)>>);

// SAFETY: written only while `ACTIVE` is false, by the one thread opening
// the terminal; read only after an acquire load of `ACTIVE` sees it set.
unsafe impl Sync for Saved {}

static SAVED: Saved = Saved(UnsafeCell::new(MaybeUninit::uninit()));
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TTY: AtomicI32 = AtomicI32::new(-1);
static PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);
static TERMINATIONS: AtomicU32 = AtomicU32::new(0);

const SIGNALS: [c_int; 7] = [
    libc::SIGINT,
    libc::SIGTERM,
    libc::SIGHUP,
    libc::SIGQUIT,
    libc::SIGTSTP,
    libc::SIGCONT,
    libc::SIGWINCH,
];

/// Puts the terminal back as it was found, once; later calls do nothing
/// until it is taken again. Async-signal-safe.
pub(crate) fn restore_now() {
    if !ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }
    let fd = TTY.load(Ordering::Acquire);
    if fd < 0 {
        return;
    }
    // SAFETY: `ACTIVE` was set, so `SAVED` holds the state saved when the
    // terminal was taken; `fd` is the terminal's open descriptor; the
    // buffer is a static string.
    unsafe {
        let (termios, flags) = (*SAVED.0.get()).assume_init();
        libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        let bytes = seq::RESTORE.as_bytes();
        let mut written = 0;
        while written < bytes.len() {
            let count = libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written);
            if count <= 0 {
                break;
            }
            written += usize::try_from(count).unwrap_or(bytes.len());
        }
        libc::tcsetattr(fd, libc::TCSANOW, &raw const termios);
        libc::fcntl(fd, libc::F_SETFL, flags);
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn errno_location() -> *mut c_int {
    // SAFETY: always valid for the calling thread.
    unsafe { libc::__errno_location() }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn errno_location() -> *mut c_int {
    // SAFETY: always valid for the calling thread.
    unsafe { libc::__error() }
}

extern "C" fn on_signal(signal: c_int) {
    // SAFETY: the errno location is the calling thread's own.
    let errno = unsafe { *errno_location() };
    let code = match signal {
        libc::SIGWINCH => b'W',
        libc::SIGHUP => b'H',
        libc::SIGTSTP => b'S',
        libc::SIGCONT => b'C',
        _ => b'T',
    };
    if matches!(code, b'T' | b'H') && TERMINATIONS.fetch_add(1, Ordering::AcqRel) >= 1 {
        // The loop did not end after the first: end from here.
        restore_now();
        // SAFETY: `_exit` is async-signal-safe and ends the process.
        unsafe { libc::_exit(128 + signal) };
    }
    let fd = PIPE_WRITE.load(Ordering::Acquire);
    if fd >= 0 {
        // SAFETY: writing one byte from a local to an open descriptor.
        unsafe { libc::write(fd, (&raw const code).cast(), 1) };
    }
    // SAFETY: as above.
    unsafe { *errno_location() = errno };
}

fn check(result: c_int) -> io::Result<c_int> {
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(result) }
}

/// The process's own terminal, in raw mode while this value lives.
pub struct ProcessTerminal {
    fd: c_int,
    owned: bool,
    pipe: [c_int; 2],
    previous: Vec<(c_int, libc::sigaction)>,
    raw: libc::termios,
}

impl std::fmt::Debug for ProcessTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessTerminal").field("fd", &self.fd).finish_non_exhaustive()
    }
}

fn make_raw(original: &libc::termios) -> libc::termios {
    let mut raw = *original;
    raw.c_iflag &= !(libc::IGNBRK
        | libc::BRKINT
        | libc::PARMRK
        | libc::ISTRIP
        | libc::INLCR
        | libc::IGNCR
        | libc::ICRNL
        | libc::IXON);
    raw.c_oflag &= !libc::OPOST;
    raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
    raw.c_cflag &= !(libc::CSIZE | libc::PARENB);
    raw.c_cflag |= libc::CS8;
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;
    raw
}

impl ProcessTerminal {
    pub(crate) fn open() -> io::Result<Self> {
        if ACTIVE.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the terminal is already in use by a session",
            ));
        }
        let path = c"/dev/tty";
        // SAFETY: a valid C string and flags.
        let opened =
            unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
        let (fd, owned) = if opened >= 0 {
            (opened, true)
        } else if unsafe { libc::isatty(0) } == 1 {
            // SAFETY (above): `isatty` only inspects the descriptor.
            (0, false)
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no terminal: the process has no controlling terminal and standard input is not one",
            ));
        };
        let mut original = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `fd` is open; `original` is written by `tcgetattr`.
        check(unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) })?;
        // SAFETY: initialized by the successful call above.
        let original = unsafe { original.assume_init() };
        // SAFETY: `fd` is open.
        let flags = check(unsafe { libc::fcntl(fd, libc::F_GETFL) })?;
        // SAFETY: `ACTIVE` is false, so nothing reads `SAVED` now.
        unsafe { (*SAVED.0.get()).write((original, flags)) };
        let mut pipe = [-1; 2];
        // SAFETY: `pipe` has room for two descriptors.
        check(unsafe { libc::pipe(pipe.as_mut_ptr()) })?;
        for end in pipe {
            // SAFETY: both ends were just opened.
            unsafe {
                libc::fcntl(end, libc::F_SETFL, libc::fcntl(end, libc::F_GETFL) | libc::O_NONBLOCK);
                libc::fcntl(end, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
        let raw = make_raw(&original);
        TTY.store(fd, Ordering::Release);
        PIPE_WRITE.store(pipe[1], Ordering::Release);
        TERMINATIONS.store(0, Ordering::Release);
        // Armed before the terminal is changed, so any failure from here on
        // is restored.
        ACTIVE.store(true, Ordering::Release);
        crate::guard::install_panic_hook();
        let mut terminal = Self { fd, owned, pipe, previous: Vec::new(), raw };
        // SAFETY: `fd` is open; `raw` is a complete termios.
        check(unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw const terminal.raw) })?;
        // SAFETY: `fd` is open.
        check(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) })?;
        terminal.install_handlers()?;
        Ok(terminal)
    }

    fn install_handlers(&mut self) -> io::Result<()> {
        for signal in SIGNALS {
            // SAFETY: `action` is zeroed then filled; `previous` receives the
            // old disposition.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as *const () as usize;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&raw mut action.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                check(libc::sigaction(signal, &raw const action, &raw mut previous))?;
                self.previous.push((signal, previous));
            }
        }
        Ok(())
    }

    fn uninstall_handlers(&mut self) {
        for (signal, previous) in self.previous.drain(..) {
            // SAFETY: restoring a disposition `sigaction` returned.
            unsafe { libc::sigaction(signal, &raw const previous, std::ptr::null_mut()) };
        }
    }

    fn read_available(&self, into: &mut Vec<u8>) -> io::Result<bool> {
        let mut buffer = [0_u8; 4096];
        loop {
            // SAFETY: reading into a local buffer of its stated length.
            let count = unsafe { libc::read(self.fd, buffer.as_mut_ptr().cast(), buffer.len()) };
            match count {
                0 => return Ok(false),
                n if n > 0 => into.extend_from_slice(&buffer[..usize::try_from(n).unwrap_or(0)]),
                _ => {
                    let error = io::Error::last_os_error();
                    return match error.kind() {
                        io::ErrorKind::WouldBlock => Ok(true),
                        io::ErrorKind::Interrupted => continue,
                        _ => Err(error),
                    };
                }
            }
        }
    }
}

impl Terminal for ProcessTerminal {
    fn size(&self) -> (u16, u16) {
        // SAFETY: `size` is written by the ioctl.
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        // SAFETY: TIOCGWINSZ writes a winsize.
        let result = unsafe { libc::ioctl(self.fd, libc::TIOCGWINSZ, &raw mut size) };
        if result < 0 || size.ws_col == 0 || size.ws_row == 0 {
            // A serial console reports nothing: the VT100 default.
            (80, 24)
        } else {
            (size.ws_col, size.ws_row)
        }
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut written = 0;
        while written < bytes.len() {
            // SAFETY: writing from a slice of its stated length.
            let count = unsafe {
                libc::write(self.fd, bytes[written..].as_ptr().cast(), bytes.len() - written)
            };
            if count >= 0 {
                written += usize::try_from(count).unwrap_or(0);
                continue;
            }
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => {}
                io::ErrorKind::WouldBlock => {
                    let mut descriptor =
                        libc::pollfd { fd: self.fd, events: libc::POLLOUT, revents: 0 };
                    // SAFETY: one valid pollfd.
                    unsafe { libc::poll(&raw mut descriptor, 1, 1000) };
                }
                _ => return Err(error),
            }
        }
        Ok(())
    }

    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<Readiness> {
        let mut readiness = Readiness::default();
        let mut descriptors = [
            libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: self.pipe[0], events: libc::POLLIN, revents: 0 },
        ];
        let milliseconds = timeout.map_or(-1, |timeout| {
            c_int::try_from(timeout.as_millis().max(u128::from(!timeout.is_zero())))
                .unwrap_or(c_int::MAX)
        });
        // SAFETY: two valid pollfds.
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, milliseconds) };
        if result < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(readiness)
            } else {
                Err(error)
            };
        }
        if descriptors[1].revents & libc::POLLIN != 0 {
            let mut codes = [0_u8; 64];
            loop {
                // SAFETY: reading into a local buffer.
                let count =
                    unsafe { libc::read(self.pipe[0], codes.as_mut_ptr().cast(), codes.len()) };
                if count <= 0 {
                    break;
                }
                for code in &codes[..usize::try_from(count).unwrap_or(0)] {
                    match code {
                        b'W' => readiness.resized = true,
                        b'T' => readiness.signals.push(Signal::Terminate),
                        b'H' => readiness.signals.push(Signal::Hangup),
                        b'S' => readiness.signals.push(Signal::Suspend),
                        b'C' => readiness.signals.push(Signal::Continue),
                        _ => readiness.woken = true,
                    }
                }
            }
        }
        if descriptors[0].revents & libc::POLLIN != 0 {
            let alive = self.read_available(&mut readiness.input)?;
            if !alive {
                readiness.signals.push(Signal::Hangup);
            }
        } else if descriptors[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            readiness.signals.push(Signal::Hangup);
        }
        Ok(readiness)
    }

    fn waker(&self) -> Waker {
        Arc::new(|| {
            let fd = PIPE_WRITE.load(Ordering::Acquire);
            if fd >= 0 {
                let byte = b'w';
                // SAFETY: one byte from a local, to the open pipe.
                unsafe { libc::write(fd, (&raw const byte).cast(), 1) };
            }
        })
    }

    fn suspend(&mut self) -> io::Result<()> {
        restore_now();
        // SAFETY: default disposition, then stop ourselves; on SIGCONT the
        // process continues here.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_DFL;
            libc::sigaction(libc::SIGTSTP, &raw const action, std::ptr::null_mut());
            libc::raise(libc::SIGTSTP);
            let mut ours: libc::sigaction = std::mem::zeroed();
            ours.sa_sigaction = on_signal as *const () as usize;
            ours.sa_flags = libc::SA_RESTART;
            libc::sigaction(libc::SIGTSTP, &raw const ours, std::ptr::null_mut());
        }
        self.retake()
    }

    fn name(&self) -> String {
        std::env::var("TERM").unwrap_or_else(|_| "unknown".to_owned())
    }
}

impl ProcessTerminal {
    /// Takes the terminal again after it was given back (raw mode,
    /// non-blocking input), re-arming restoration.
    fn retake(&mut self) -> io::Result<()> {
        TERMINATIONS.store(0, Ordering::Release);
        ACTIVE.store(true, Ordering::Release);
        // SAFETY: `fd` is open; `raw` is a complete termios.
        check(unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &raw const self.raw) })?;
        // SAFETY: as above.
        unsafe {
            let flags = libc::fcntl(self.fd, libc::F_GETFL);
            libc::fcntl(self.fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        Ok(())
    }

    /// The current `termios` local flags (`ICANON`, `ECHO` â€¦), for tests
    /// that read the mode back.
    #[must_use]
    pub fn local_flags(&self) -> Option<libc::tcflag_t> {
        let mut termios = MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `fd` is open; written on success.
        (unsafe { libc::tcgetattr(self.fd, termios.as_mut_ptr()) } == 0)
            // SAFETY: initialized by the successful call.
            .then(|| unsafe { termios.assume_init() }.c_lflag)
    }
}

impl Drop for ProcessTerminal {
    fn drop(&mut self) {
        restore_now();
        self.uninstall_handlers();
        PIPE_WRITE.store(-1, Ordering::Release);
        TTY.store(-1, Ordering::Release);
        // SAFETY: closing descriptors this value opened.
        unsafe {
            libc::close(self.pipe[0]);
            libc::close(self.pipe[1]);
            if self.owned {
                libc::close(self.fd);
            }
        }
    }
}
