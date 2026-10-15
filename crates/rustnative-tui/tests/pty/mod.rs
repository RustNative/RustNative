//! A real pseudo-terminal for the tests: the test binary runs itself as a
//! child (`--exact child rnscenario=<name>`) on the slave side of a pty
//! (`posix_openpt` on Unix) or a `ConPTY` (Windows), and the parent plays the
//! terminal — its output runs through the crate's own VT emulator, which
//! also answers the child's questions, and input goes in as bytes.
//!
//! So the bytes cross a real terminal driver: the kernel's line discipline
//! (`termios`, `SIGWINCH`, job control) on Unix, and on Windows the console
//! host behind `ConPTY` — which turns the parent's VT input into console
//! input records and back into VT for a child in virtual-terminal mode, as
//! Windows Terminal does.

#![allow(dead_code, reason = "each platform uses part of it")]

use std::time::{Duration, Instant};

use rustnative_tui::emulator::{Answers, Emulator};

/// The argument that names the scenario the child runs.
pub const SCENARIO: &str = "rnscenario=";

/// The scenario this process was started to run, if it is a child.
pub fn scenario() -> Option<String> {
    std::env::args().find_map(|argument| argument.strip_prefix(SCENARIO).map(str::to_owned))
}

/// The child's arguments: just the `child` test, with output shown.
fn child_arguments(scenario: &str) -> Vec<String> {
    vec![
        "--exact".to_owned(),
        "child".to_owned(),
        format!("{SCENARIO}{scenario}"),
        "--nocapture".to_owned(),
        "--test-threads=1".to_owned(),
    ]
}

#[cfg(unix)]
pub use unix::Pty;
#[cfg(windows)]
pub use windows::Pty;

/// What every pty shares: the emulator and the waiting.
pub struct Screen {
    pub emulator: Emulator,
    pub output: Vec<u8>,
}

impl Screen {
    fn new(columns: u16, rows: u16) -> Self {
        Self { emulator: Emulator::new(columns, rows, Answers::modern()), output: Vec::new() }
    }

    fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.output.extend_from_slice(bytes);
        self.emulator.feed(bytes);
        self.emulator.take_replies()
    }

    /// Everything shown now, and the main screen beneath it.
    pub fn text(&self) -> String {
        format!("{}\n{}", self.emulator.screen().text(), self.emulator.main_screen().text())
    }
}

#[cfg(unix)]
mod unix {
    use std::ffi::CStr;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::time::{Duration, Instant};

    use super::Screen;

    /// A child on a pty.
    pub struct Pty {
        master: OwnedFd,
        slave: OwnedFd,
        child: Child,
        pub screen: Screen,
    }

    impl Pty {
        /// Starts scenario `name` on a `columns` × `rows` pty.
        pub fn spawn(name: &str, columns: u16, rows: u16) -> Self {
            let exe = std::env::current_exe().expect("this test binary");
            Self::spawn_command(exe.as_os_str(), &super::child_arguments(name), columns, rows)
        }

        /// Starts scenario `name` over SSH: `target` is
        /// `user@host:port:identity-file` (`tools/tui-session.sh ssh`), and
        /// the far side runs this same binary, with a pty `sshd` allocates.
        pub fn spawn_over_ssh(target: &str, name: &str, columns: u16, rows: u16) -> Self {
            let mut parts = target.rsplitn(3, ':');
            let identity = parts.next().expect("an identity file");
            let port = parts.next().expect("a port");
            let destination = parts.next().expect("user@host");
            let exe = std::env::current_exe().expect("this test binary");
            let mut arguments: Vec<String> = [
                "-tt",
                "-p",
                port,
                "-i",
                identity,
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "LogLevel=ERROR",
                destination,
            ]
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect();
            arguments.push(exe.display().to_string());
            arguments.extend(super::child_arguments(name));
            Self::spawn_command(std::ffi::OsStr::new("ssh"), &arguments, columns, rows)
        }

        fn spawn_command(
            program: &std::ffi::OsStr,
            arguments: &[String],
            columns: u16,
            rows: u16,
        ) -> Self {
            // SAFETY: plain libc calls on descriptors this function owns.
            unsafe {
                let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
                assert!(master >= 0, "posix_openpt");
                assert_eq!(libc::grantpt(master), 0);
                assert_eq!(libc::unlockpt(master), 0);
                let name_ptr = libc::ptsname(master);
                assert!(!name_ptr.is_null());
                let path = CStr::from_ptr(name_ptr).to_owned();
                let size =
                    libc::winsize { ws_row: rows, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0 };
                libc::ioctl(master, libc::TIOCSWINSZ, &raw const size);
                let slave = libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
                assert!(slave >= 0, "open the slave");
                let flags = libc::fcntl(master, libc::F_GETFL);
                libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK);
                let master = OwnedFd::from_raw_fd(master);
                let slave = OwnedFd::from_raw_fd(slave);
                let io = |fd: &OwnedFd| Stdio::from(fd.try_clone().expect("dup the slave"));
                let mut command = Command::new(program);
                command
                    .args(arguments)
                    .env("TERM", "xterm-256color")
                    .env_remove("COLORTERM")
                    .env("RUSTNATIVE_TUI_PROBE_MS", "1500")
                    .stdin(io(&slave))
                    .stdout(io(&slave))
                    .stderr(io(&slave));
                command.pre_exec(|| {
                    // A session of its own, with the pty as its controlling
                    // terminal, so `/dev/tty` is the pty and SIGWINCH reaches it.
                    if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
                let child = command.spawn().expect("start the child");
                Self { master, slave, child, screen: Screen::new(columns, rows) }
            }
        }

        /// Reads what is there, answers what was asked.
        pub fn pump(&mut self, wait: Duration) {
            let mut descriptor =
                libc::pollfd { fd: self.master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let milliseconds = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
            // SAFETY: one valid pollfd.
            unsafe { libc::poll(&raw mut descriptor, 1, milliseconds) };
            let mut buffer = vec![0_u8; 65_536];
            loop {
                // SAFETY: reading into a local buffer.
                let count = unsafe {
                    libc::read(self.master.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len())
                };
                if count <= 0 {
                    break;
                }
                let replies = self.screen.feed(&buffer[..usize::try_from(count).unwrap_or(0)]);
                self.send(&replies);
            }
        }

        /// Types bytes into the terminal.
        pub fn send(&mut self, bytes: &[u8]) {
            let mut written = 0;
            while written < bytes.len() {
                // SAFETY: writing from a slice.
                let count = unsafe {
                    libc::write(
                        self.master.as_raw_fd(),
                        bytes[written..].as_ptr().cast(),
                        bytes.len() - written,
                    )
                };
                if count <= 0 {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                written += usize::try_from(count).unwrap_or(0);
            }
        }

        /// Pumps until the screen (shown or main) contains `text`.
        pub fn wait_for(&mut self, text: &str, timeout: Duration) -> bool {
            let started = Instant::now();
            while started.elapsed() < timeout {
                if self.screen.text().contains(text) {
                    return true;
                }
                self.pump(Duration::from_millis(20));
            }
            self.screen.text().contains(text)
        }

        /// Pumps until the child exits.
        pub fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
            let started = Instant::now();
            while started.elapsed() < timeout {
                self.pump(Duration::from_millis(20));
                if let Ok(Some(status)) = self.child.try_wait() {
                    self.pump(Duration::from_millis(50));
                    return Some(status);
                }
            }
            None
        }

        /// The slave's local flags now (`ICANON`, `ECHO` …).
        pub fn local_flags(&self) -> libc::tcflag_t {
            // SAFETY: zeroed termios written by tcgetattr.
            unsafe {
                let mut termios: libc::termios = std::mem::zeroed();
                libc::tcgetattr(self.slave.as_raw_fd(), &raw mut termios);
                termios.c_lflag
            }
        }

        /// Resizes the pty (the kernel signals the child).
        pub fn resize(&mut self, columns: u16, rows: u16) {
            let size = libc::winsize { ws_row: rows, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0 };
            // SAFETY: a valid winsize.
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &raw const size) };
            self.screen.emulator.resize(columns, rows);
        }

        /// Sends `signal` to the child.
        pub fn kill(&self, signal: i32) {
            // SAFETY: signalling our own child.
            unsafe { libc::kill(i32::try_from(self.child.id()).unwrap_or(0), signal) };
        }
    }

    impl Drop for Pty {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Console::{
        COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    };

    use super::Screen;

    /// A child on a `ConPTY`.
    pub struct Pty {
        console: HPCON,
        input: HANDLE,
        process: HANDLE,
        output: Receiver<Vec<u8>>,
        pub screen: Screen,
        _attributes: Vec<u8>,
    }

    struct SendHandle(usize);
    // SAFETY: a pipe handle moved to the reader thread, used only there.
    unsafe impl Send for SendHandle {}

    impl Pty {
        /// Starts scenario `name` on a `columns` × `rows` `ConPTY`.
        #[allow(clippy::too_many_lines, reason = "the ConPTY recipe, step by step")]
        pub fn spawn(name: &str, columns: u16, rows: u16) -> Self {
            // SAFETY: the documented ConPTY recipe; every handle is owned here.
            unsafe {
                let (mut input_read, mut input_write, mut output_read, mut output_write) = (
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                assert_ne!(
                    CreatePipe(&raw mut input_read, &raw mut input_write, std::ptr::null(), 0),
                    0
                );
                assert_ne!(
                    CreatePipe(&raw mut output_read, &raw mut output_write, std::ptr::null(), 0),
                    0
                );
                let size = COORD {
                    X: i16::try_from(columns).unwrap_or(80),
                    Y: i16::try_from(rows).unwrap_or(24),
                };
                let mut console: HPCON = 0;
                assert_eq!(
                    CreatePseudoConsole(size, input_read, output_write, 0, &raw mut console),
                    0
                );
                CloseHandle(input_read);
                CloseHandle(output_write);
                let mut bytes = 0_usize;
                InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &raw mut bytes);
                let mut attributes = vec![0_u8; bytes];
                let list = attributes.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
                assert_ne!(InitializeProcThreadAttributeList(list, 1, 0, &raw mut bytes), 0);
                assert_ne!(
                    UpdateProcThreadAttribute(
                        list,
                        0,
                        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                        console as *const core::ffi::c_void,
                        std::mem::size_of::<HPCON>(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    ),
                    0
                );
                let mut startup: STARTUPINFOEXW = std::mem::zeroed();
                startup.StartupInfo.cb =
                    u32::try_from(std::mem::size_of::<STARTUPINFOEXW>()).unwrap_or(0);
                startup.lpAttributeList = list;
                // Null standard handles: the child takes the pseudo console's,
                // not this process's (which a test runner redirects).
                startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
                let exe = std::env::current_exe().expect("this test binary");
                let mut line = format!("\"{}\"", exe.display());
                for argument in super::child_arguments(name) {
                    line.push(' ');
                    line.push_str(&argument);
                }
                let mut line: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
                let mut info: PROCESS_INFORMATION = std::mem::zeroed();
                let created = CreateProcessW(
                    std::ptr::null(),
                    line.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    EXTENDED_STARTUPINFO_PRESENT,
                    std::ptr::null(),
                    std::ptr::null(),
                    &raw const startup.StartupInfo,
                    &raw mut info,
                );
                assert_ne!(created, 0, "start the child: {}", std::io::Error::last_os_error());
                CloseHandle(info.hThread);
                let (sender, output) = channel();
                let reader = SendHandle(output_read as usize);
                std::thread::spawn(move || {
                    let reader = reader;
                    let mut buffer = vec![0_u8; 65_536];
                    loop {
                        let mut read = 0_u32;
                        let ok = ReadFile(
                            reader.0 as HANDLE,
                            buffer.as_mut_ptr(),
                            u32::try_from(buffer.len()).unwrap_or(0),
                            &raw mut read,
                            std::ptr::null_mut(),
                        );
                        if ok == 0
                            || read == 0
                            || sender.send(buffer[..read as usize].to_vec()).is_err()
                        {
                            break;
                        }
                    }
                    CloseHandle(reader.0 as HANDLE);
                });
                Self {
                    console,
                    input: input_write,
                    process: info.hProcess,
                    output,
                    screen: Screen::new(columns, rows),
                    _attributes: attributes,
                }
            }
        }

        /// Reads what is there, answers what was asked.
        pub fn pump(&mut self, wait: Duration) {
            let mut first = true;
            loop {
                let chunk = if first {
                    self.output.recv_timeout(wait).ok()
                } else {
                    self.output.try_recv().ok()
                };
                first = false;
                let Some(chunk) = chunk else { break };
                let replies = self.screen.feed(&chunk);
                self.send(&replies);
            }
        }

        /// Types bytes into the terminal.
        pub fn send(&mut self, bytes: &[u8]) {
            if bytes.is_empty() {
                return;
            }
            let mut written = 0_u32;
            // SAFETY: the input pipe's write end, a slice of its length.
            unsafe {
                WriteFile(
                    self.input,
                    bytes.as_ptr(),
                    u32::try_from(bytes.len()).unwrap_or(0),
                    &raw mut written,
                    std::ptr::null_mut(),
                );
            }
        }

        /// Pumps until the screen (shown or main) contains `text`.
        pub fn wait_for(&mut self, text: &str, timeout: Duration) -> bool {
            let started = Instant::now();
            while started.elapsed() < timeout {
                if self.screen.text().contains(text) {
                    return true;
                }
                self.pump(Duration::from_millis(20));
            }
            self.screen.text().contains(text)
        }

        /// Pumps until the child exits; its exit code.
        pub fn wait_exit(&mut self, timeout: Duration) -> Option<u32> {
            let started = Instant::now();
            while started.elapsed() < timeout {
                self.pump(Duration::from_millis(20));
                // SAFETY: the child's process handle.
                if unsafe { WaitForSingleObject(self.process, 0) } == WAIT_OBJECT_0 {
                    self.pump(Duration::from_millis(100));
                    let mut code = 0;
                    // SAFETY: as above.
                    unsafe { GetExitCodeProcess(self.process, &raw mut code) };
                    return Some(code);
                }
            }
            None
        }

        /// Resizes the pseudo console.
        pub fn resize(&mut self, columns: u16, rows: u16) {
            let size = COORD {
                X: i16::try_from(columns).unwrap_or(80),
                Y: i16::try_from(rows).unwrap_or(24),
            };
            // SAFETY: the pseudo console this value created.
            unsafe { ResizePseudoConsole(self.console, size) };
            self.screen.emulator.resize(columns, rows);
        }

        /// Closes the pseudo console, as closing the terminal window does.
        pub fn close_console(&mut self) {
            if self.console != 0 {
                // SAFETY: closed once.
                unsafe { ClosePseudoConsole(self.console) };
                self.console = 0;
            }
        }
    }

    impl Drop for Pty {
        fn drop(&mut self) {
            // SAFETY: ending and releasing what this value created.
            unsafe {
                TerminateProcess(self.process, 1);
                CloseHandle(self.process);
                CloseHandle(self.input);
            }
            self.close_console();
        }
    }
}

/// Waits a little for output to settle.
pub fn settle(wait: Duration) -> Instant {
    Instant::now() + wait
}
