//! The terminal backend on a real terminal driver (`docs/superpowers/plans/
//! 2026-10-13-terminal-milestone-38.md`, Phase 1's acceptance tests).
//!
//! Each test starts this binary again as a child on a pseudo-terminal
//! (`tests/pty`), running one scenario with [`TuiPlatform`], and plays the
//! terminal in the parent. On Unix the bytes cross the kernel's terminal
//! driver; on Windows, the console host behind `ConPTY`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

mod pty;

use std::time::Duration;

use rustnative_core::{
    Application, Component, Event, KeyCode, KeyModifiers, Node, NodeId, Platform, Size, Window,
};
use rustnative_tui::TuiPlatform;

const WAIT: Duration = Duration::from_secs(20);

// ----------------------------------------------------------------------
// The child
// ----------------------------------------------------------------------

/// Shows a greeting, the window size, and the last key and text it got.
struct Probe {
    size: Size,
    key: String,
    text: String,
}

impl Component for Probe {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self { size: Size::new(0, 0), key: "none".into(), text: String::new() }
    }

    fn props(&self) -> &() {
        &()
    }

    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("hello", "hello pty"),
                Node::label(
                    "size",
                    format!("size {}x{}", self.size.width / 8, self.size.height / 16),
                ),
                Node::label("key", format!("key [{}]", self.key)),
                Node::label("text", format!("text [{}]", self.text)),
                Node::button("boom", "Boom"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::WindowResized { size, .. } => self.size = size,
            Event::KeyDown { key, modifiers, .. } => {
                self.key = format!(
                    "{key:?}{}{}{}",
                    if modifiers.shift { " shift" } else { "" },
                    if modifiers.alt { " alt" } else { "" },
                    if modifiers.ctrl { " ctrl" } else { "" }
                );
            }
            Event::TextInput { text, .. } => self.text.push_str(&text),
            Event::Click { target } if target == NodeId::from_key("boom") => {
                if std::env::args().any(|argument| argument.ends_with("=stuck")) {
                    // A component that never returns: only a second signal
                    // ends the process.
                    std::thread::sleep(Duration::from_secs(60));
                }
                panic!("a component panicked on purpose");
            }
            _ => {}
        }
    }
}

#[cfg(unix)]
fn terminal_state() -> String {
    // SAFETY: reading the terminal's mode into a zeroed termios.
    let flags = unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        libc::tcgetattr(0, &raw mut termios);
        termios.c_lflag
    };
    format!("icanon={} echo={}", flags & libc::ICANON != 0, flags & libc::ECHO != 0)
}

#[cfg(windows)]
fn terminal_state() -> String {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    let (mut input, mut output) = (0, 0);
    // SAFETY: the console's standard handles and written modes.
    unsafe {
        GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &raw mut input);
        GetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), &raw mut output);
    }
    format!("in={input:#x} out={output:#x}")
}

/// The child: runs its scenario, then reports how it left the terminal.
#[test]
fn child() {
    let Some(scenario) = pty::scenario() else { return };
    println!("BEFORE {}", terminal_state());
    println!(
        "SESSION {}",
        if std::env::var_os("SSH_CONNECTION").is_some() { "ssh" } else { "local" }
    );
    if scenario == "raw-panic" {
        // A panic outside every boundary, with the terminal held: only the
        // panic hook stands between it and a ruined terminal.
        let mut terminal = rustnative_tui::term::open().expect("a terminal");
        rustnative_tui::term::Terminal::write(
            &mut terminal,
            b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?2004hheld",
        )
        .expect("write");
        std::thread::sleep(Duration::from_millis(300));
        panic!("raw panic with the terminal held");
    }
    let mut application = Application::new(Probe::new(()), Window::new("Probe", Size::new(0, 0)));
    let outcome = TuiPlatform::new().run(&mut application);
    println!("AFTER {}", terminal_state());
    match outcome {
        Ok(()) => println!("RESULT ok"),
        Err(error) => println!("RESULT err {error}"),
    }
}

// ----------------------------------------------------------------------
// The parent
// ----------------------------------------------------------------------

/// Everything a session turns on is off again, and the shell's screen is
/// back.
fn assert_restored(terminal: &pty::Pty) {
    let emulator = &terminal.screen.emulator;
    assert!(
        !emulator.on_alternate_screen(),
        "left the alternate screen:\n{}",
        terminal.screen.text()
    );
    assert!(emulator.mode(25), "the cursor is shown again");
    assert!(emulator.mode(7), "autowrap is back on");
    for mode in [1000, 1002, 1003, 1006, 1004, 2004, 2026] {
        assert!(!emulator.mode(mode), "mode {mode} was turned off");
    }
    assert_eq!(emulator.kitty_flags(), 0);
}

#[cfg(unix)]
fn assert_cooked(terminal: &pty::Pty) {
    let flags = terminal.local_flags();
    assert!(
        flags & libc::ICANON != 0 && flags & libc::ECHO != 0,
        "the terminal is back in cooked mode"
    );
}

/// The BEFORE and AFTER lines the child printed agree.
fn assert_same_mode(terminal: &pty::Pty) {
    // From everything written, not the screen: the line may have scrolled
    // away by the time the application is done.
    let text = String::from_utf8_lossy(&terminal.screen.output).into_owned();
    let after_marker = |marker: &str| {
        text.split(marker)
            .nth(1)
            .and_then(|rest| rest.split(['\r', '\n', '\u{1b}']).next())
            .map(str::trim)
    };
    let (before, after) = (after_marker("BEFORE "), after_marker("AFTER "));
    assert!(
        before.is_some() && before == after,
        "the mode was restored: {before:?} → {after:?}\n{text}"
    );
}

fn start(columns: u16, rows: u16) -> pty::Pty {
    let mut terminal = pty::Pty::spawn("probe", columns, rows);
    assert!(
        terminal.wait_for("hello pty", WAIT),
        "the application started:\n{}",
        terminal.screen.text()
    );
    terminal
}

#[test]
fn a_session_takes_the_terminal_and_gives_it_back() {
    let mut terminal = start(60, 16);
    terminal.pump(Duration::from_millis(300));
    assert!(terminal.screen.emulator.mode(2004), "bracketed paste is on");
    assert!(terminal.screen.emulator.mode(1004), "focus reporting is on");
    if cfg!(unix) {
        assert!(terminal.screen.emulator.on_alternate_screen());
        assert!(terminal.screen.emulator.mode(1006), "SGR mouse is on");
        assert!(!terminal.screen.emulator.mode(7), "autowrap is off while it runs");
    }
    terminal.send(&[0x03]); // Ctrl+C: close
    let status = terminal.wait_exit(WAIT);
    assert!(status.is_some(), "Ctrl+C ended it");
    assert!(terminal.wait_for("RESULT ok", WAIT), "{}", terminal.screen.text());
    assert_restored(&terminal);
    assert_same_mode(&terminal);
    #[cfg(unix)]
    assert_cooked(&terminal);
}

#[test]
fn a_panic_restores_the_terminal() {
    let mut terminal = start(60, 16);
    terminal.send(b"\t\r"); // focus Boom, press it
    assert!(
        terminal.wait_exit(WAIT).is_some(),
        "the panic ended the run:\n{}",
        terminal.screen.text()
    );
    assert!(terminal.wait_for("RESULT err", WAIT), "{}", terminal.screen.text());
    assert!(
        terminal.screen.text().contains("panicked on purpose"),
        "the report shows on the restored screen"
    );
    assert_restored(&terminal);
    assert_same_mode(&terminal);
    #[cfg(unix)]
    assert_cooked(&terminal);
}

#[test]
fn a_panic_outside_every_boundary_restores_the_terminal() {
    let mut terminal = pty::Pty::spawn("raw-panic", 60, 16);
    assert!(terminal.wait_for("held", WAIT), "{}", terminal.screen.text());
    assert!(terminal.wait_exit(WAIT).is_some());
    assert!(
        terminal.wait_for("raw panic with the terminal held", WAIT),
        "{}",
        terminal.screen.text()
    );
    assert_restored(&terminal);
    #[cfg(unix)]
    assert_cooked(&terminal);
}

#[test]
fn a_resize_is_followed() {
    let mut terminal = start(60, 16);
    assert!(terminal.wait_for("size 60x16", WAIT), "{}", terminal.screen.text());
    terminal.resize(72, 20);
    assert!(terminal.wait_for("size 72x20", WAIT), "{}", terminal.screen.text());
    terminal.send(&[0x03]);
    assert!(terminal.wait_exit(WAIT).is_some());
    assert_restored(&terminal);
}

/// Every key the terminal reports reaches the application as the portable
/// key, with its modifiers — through the real driver (and, on Windows,
/// through the console's own translation to input records and back).
#[test]
fn every_key_the_terminal_reports_is_decoded() {
    let mut terminal = start(70, 16);
    let none = KeyModifiers::default();
    let shift = KeyModifiers { shift: true, ..none };
    let ctrl = KeyModifiers { ctrl: true, ..none };
    let alt = KeyModifiers { alt: true, ..none };
    let cases: Vec<(KeyCode, KeyModifiers, &str)> = vec![
        (KeyCode::ArrowUp, none, "ArrowUp"),
        (KeyCode::ArrowDown, none, "ArrowDown"),
        (KeyCode::ArrowLeft, none, "ArrowLeft"),
        (KeyCode::ArrowRight, none, "ArrowRight"),
        (KeyCode::ArrowRight, ctrl, "ArrowRight ctrl"),
        (KeyCode::ArrowLeft, shift, "ArrowLeft shift"),
        (KeyCode::Home, none, "Home"),
        (KeyCode::End, none, "End"),
        (KeyCode::PageUp, none, "PageUp"),
        (KeyCode::PageDown, none, "PageDown"),
        (KeyCode::Insert, none, "Insert"),
        (KeyCode::Delete, none, "Delete"),
        (KeyCode::Delete, ctrl, "Delete ctrl"),
        (KeyCode::Backspace, none, "Backspace"),
        (KeyCode::Enter, none, "Enter"),
        (KeyCode::Escape, none, "Escape"),
        (KeyCode::Function(1), none, "Function(1)"),
        (KeyCode::Function(4), none, "Function(4)"),
        (KeyCode::Function(5), none, "Function(5)"),
        (KeyCode::Function(12), none, "Function(12)"),
        (KeyCode::Character('a'), ctrl, "Character('a') ctrl"),
        (KeyCode::Character('x'), alt, "Character('x') alt"),
        (KeyCode::Character('q'), none, "Character('q')"),
    ];
    for (key, modifiers, shown) in cases {
        terminal.send(&rustnative_tui::harness::key_bytes(key, modifiers));
        let expected = format!("key [{shown}]");
        assert!(
            terminal.wait_for(&expected, WAIT),
            "{key:?} {modifiers:?} reached the application as {expected}:\n{}",
            terminal.screen.text()
        );
    }
    terminal.send("é日".as_bytes());
    assert!(
        terminal.wait_for("text [qé日]", WAIT),
        "text arrives whole:\n{}",
        terminal.screen.text()
    );
    terminal.send(b"\x1b[200~pasted\x1b[201~");
    assert!(terminal.wait_for("text [qé日pasted]", WAIT), "{}", terminal.screen.text());
    terminal.send(&[0x03]);
    assert!(terminal.wait_exit(WAIT).is_some());
    assert_restored(&terminal);
}

#[cfg(unix)]
#[test]
fn a_terminating_signal_ends_the_run_cleanly() {
    let mut terminal = start(60, 16);
    terminal.kill(libc::SIGTERM);
    assert!(terminal.wait_exit(WAIT).is_some(), "SIGTERM ended it");
    assert!(terminal.wait_for("RESULT ok", WAIT), "{}", terminal.screen.text());
    assert_restored(&terminal);
    assert_cooked(&terminal);
}

#[cfg(unix)]
#[test]
fn a_second_signal_restores_a_stuck_application() {
    let mut terminal = pty::Pty::spawn("stuck", 60, 16);
    assert!(terminal.wait_for("hello pty", WAIT), "{}", terminal.screen.text());
    terminal.send(b"\t\r"); // the component now sleeps in `update`
    std::thread::sleep(Duration::from_millis(500));
    terminal.kill(libc::SIGTERM);
    std::thread::sleep(Duration::from_millis(200));
    terminal.kill(libc::SIGTERM);
    let status = terminal.wait_exit(WAIT).expect("the second signal ended it");
    assert_eq!(status.code(), Some(128 + libc::SIGTERM), "{status:?}");
    assert_restored(&terminal);
    assert_cooked(&terminal);
}

/// `SIGTSTP` gives the terminal back (cooked mode, the shell's screen) and
/// stops; continuing retakes it and redraws. The child here is alone in its
/// session — an orphaned process group, where POSIX discards stop signals —
/// so it is not actually stopped, and what is checked is the sequence: the
/// restoration written, then the session's modes and a full frame again.
#[cfg(unix)]
#[test]
fn a_stop_gives_the_terminal_back_and_retakes_it() {
    let mut terminal = start(60, 16);
    terminal.pump(Duration::from_millis(300));
    let mark = terminal.screen.output.len();
    terminal.kill(libc::SIGTSTP);
    let started = std::time::Instant::now();
    let retaken = loop {
        terminal.pump(Duration::from_millis(20));
        let after = String::from_utf8_lossy(&terminal.screen.output[mark..]).into_owned();
        let left = after.find("\x1b[?1049l");
        let back = left.and_then(|at| after[at..].find("\x1b[?1049h").map(|offset| at + offset));
        if let (Some(_), Some(_)) = (left, back) {
            // Retaken, the screen was cleared and drawn again in full.
            let shown = terminal.screen.emulator.screen().text().contains("hello pty");
            if shown || started.elapsed() > WAIT {
                break shown;
            }
        }
        assert!(started.elapsed() < WAIT * 2, "no give-back and retake in {after:?}");
    };
    assert!(
        retaken,
        "the screen was redrawn after retaking it: {:?}",
        String::from_utf8_lossy(&terminal.screen.output[mark..])
    );
    assert!(terminal.screen.emulator.on_alternate_screen());
    terminal.send(&[0x03]);
    assert!(terminal.wait_exit(WAIT).is_some());
    assert_restored(&terminal);
    assert_cooked(&terminal);
}

#[cfg(windows)]
#[test]
fn closing_the_console_ends_the_run() {
    let mut terminal = start(60, 16);
    terminal.close_console();
    assert!(terminal.wait_exit(WAIT).is_some(), "closing the console ended the application");
}

/// The same application on a terminal reached over SSH
/// (`tools/tui-session.sh ssh`, which starts a private `sshd` and names it
/// in `RUSTNATIVE_TUI_SSH`): the child runs on the pty `sshd` allocates, its
/// questions and input cross the SSH channel, and a resize here reaches it
/// as the SSH window-change it becomes.
#[cfg(unix)]
#[test]
#[ignore = "needs an SSH server: tools/tui-session.sh ssh"]
fn over_ssh() {
    let Ok(target) = std::env::var("RUSTNATIVE_TUI_SSH") else {
        panic!("RUSTNATIVE_TUI_SSH names the server: run tools/tui-session.sh ssh");
    };
    let mut terminal = pty::Pty::spawn_over_ssh(&target, "probe", 70, 16);
    assert!(terminal.wait_for("hello pty", WAIT), "{}", terminal.screen.text());
    assert!(terminal.screen.text().contains("SESSION ssh"), "the far side knows it is remote");
    assert!(terminal.screen.emulator.on_alternate_screen());
    for (bytes, shown) in [
        (b"\x1b[1;5C".as_slice(), "key [ArrowRight ctrl]"),
        (b"\x1bOP".as_slice(), "key [Function(1)]"),
        (b"\x1b[3~".as_slice(), "key [Delete]"),
    ] {
        terminal.send(bytes);
        assert!(terminal.wait_for(shown, WAIT), "{shown}:\n{}", terminal.screen.text());
    }
    terminal.send("é日".as_bytes());
    terminal.send(b"\x1b[200~over ssh\x1b[201~");
    assert!(terminal.wait_for("text [é日over ssh]", WAIT), "{}", terminal.screen.text());
    terminal.resize(72, 20);
    assert!(terminal.wait_for("size 72x20", WAIT), "{}", terminal.screen.text());
    terminal.send(&[0x03]);
    assert!(terminal.wait_exit(WAIT).is_some());
    assert!(terminal.wait_for("RESULT ok", WAIT), "{}", terminal.screen.text());
    assert_restored(&terminal);
    assert_same_mode(&terminal);
}
