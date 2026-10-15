//! What the terminal is: asked, not assumed (`PLAN.md` 2.5 and Milestone
//! 38's "capabilities, advertised honestly").
//!
//! At startup the backend writes its questions and goes on starting; the
//! answers arrive as input and are routed here:
//!
//! 1. DECRQM for modes 1003 (all-motion mouse), 1004 (focus), 1006 (SGR
//!    mouse), 2004 (bracketed paste), and 2026 (synchronized output);
//! 2. the kitty keyboard protocol's flags (`CSI ? u`);
//! 3. the default background colour (OSC 11), for the colour scheme;
//! 4. primary device attributes (DA1) — last, because every VT terminal
//!    answers it, so its answer means every earlier question has been
//!    answered or ignored.
//!
//! The probe is finished at the DA1 answer or at its deadline
//! ([`PROBE_DEADLINE`], `RUSTNATIVE_TUI_PROBE_MS`), whichever is first; a
//! question with no answer by then is *unknown*, and an unknown feature is
//! never advertised. What the environment says (the terminal's own
//! variables) fills in what terminals cannot be asked: which notification
//! sequence they implement, and whether OSC 52 reaches a clipboard.

use std::collections::BTreeMap;
use std::time::Duration;

use rustnative_core::Color;

use crate::color::{ColorDepth, DetectInput};
use crate::decode::Reply;
use crate::encode::seq;

/// How long the probe waits for answers by default.
pub const PROBE_DEADLINE: Duration = Duration::from_millis(250);

/// The modes asked about.
pub const PROBED_MODES: [u16; 5] = [1003, 1004, 1006, 2004, 2026];

/// Which sequence posts a notification on this terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationProtocol {
    /// `OSC 9 ; text` (iTerm2, Ghostty, `WezTerm`).
    Osc9,
    /// `OSC 99` (kitty).
    Osc99,
    /// `OSC 777 ; notify ; title ; body` (foot, urxvt).
    Osc777,
}

/// Whether OSC 52 reaches the system clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardSupport {
    /// It does not, or it is not known to.
    None,
    /// Writes reach it; reads are refused or unknown.
    Write,
    /// Writes and reads.
    ReadWrite,
}

/// What is known about the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalProfile {
    /// The terminal's name, for diagnostics.
    pub name: String,
    /// Its colour depth.
    pub color_depth: ColorDepth,
    /// Its DA1 answer, when it gave one.
    pub attributes: Option<Vec<u16>>,
    /// Its DECRQM answers: mode → state (0 unrecognized … 4).
    pub modes: BTreeMap<u16, u8>,
    /// Whether it speaks the kitty keyboard protocol.
    pub kitty_keyboard: bool,
    /// Its default background colour, when it answered.
    pub background: Option<Color>,
    /// OSC 52.
    pub clipboard: ClipboardSupport,
    /// Its notification sequence, if it has one.
    pub notifications: Option<NotificationProtocol>,
    /// Whether the session runs over SSH (a URL opened "locally" would open
    /// on the far machine).
    pub remote: bool,
    /// Whether it is the Windows console.
    pub windows_console: bool,
}

impl TerminalProfile {
    /// What the environment alone says, before any answer.
    #[must_use]
    pub fn from_environment(input: &DetectInput, name: String) -> Self {
        let get = |key: &str| input.vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let term = get("TERM").unwrap_or_default().to_ascii_lowercase();
        let program = get("TERM_PROGRAM").unwrap_or_default();
        let notifications = if term.contains("kitty") {
            Some(NotificationProtocol::Osc99)
        } else if term.starts_with("foot") || term.starts_with("rxvt") {
            Some(NotificationProtocol::Osc777)
        } else if matches!(program.as_str(), "iTerm.app" | "WezTerm" | "ghostty")
            || term.contains("ghostty")
        {
            Some(NotificationProtocol::Osc9)
        } else {
            None
        };
        // Terminals known to put OSC 52 writes on the clipboard by default;
        // tmux forwards them when its `set-clipboard` allows. Others answer
        // through DA1's `52`.
        let clipboard = if term.contains("kitty")
            || term.starts_with("foot")
            || term.contains("alacritty")
            || term.contains("ghostty")
            || matches!(program.as_str(), "iTerm.app" | "WezTerm" | "ghostty")
            || get("WT_SESSION").is_some()
        {
            ClipboardSupport::Write
        } else {
            ClipboardSupport::None
        };
        Self {
            name,
            color_depth: ColorDepth::detect(input),
            attributes: None,
            modes: BTreeMap::new(),
            kitty_keyboard: false,
            background: None,
            clipboard,
            notifications,
            remote: get("SSH_CONNECTION").is_some() || get("SSH_TTY").is_some(),
            windows_console: input.windows_vt,
        }
    }

    /// Whether DEC private mode `mode` was answered as recognized.
    #[must_use]
    pub fn recognizes(&self, mode: u16) -> bool {
        self.modes.get(&mode).is_some_and(|state| *state != 0)
    }

    /// Whether the terminal reports pointer motion with no button held, in
    /// SGR encoding — what hover needs.
    #[must_use]
    pub fn reports_hover(&self) -> bool {
        self.recognizes(1003) && self.recognizes(1006)
    }

    /// Whether frames can be wrapped in synchronized output.
    #[must_use]
    pub fn synchronized_output(&self) -> bool {
        self.recognizes(2026)
    }

    /// The colour scheme the default background implies, if it answered.
    #[must_use]
    pub fn dark_background(&self) -> Option<bool> {
        self.background.map(|color| {
            let luminance = u32::from(color.red) * 299
                + u32::from(color.green) * 587
                + u32::from(color.blue) * 114;
            luminance < 128_000
        })
    }

    /// Takes in one answer.
    pub fn accept(&mut self, reply: &Reply) {
        match reply {
            Reply::PrimaryAttributes(attributes) => {
                if attributes.contains(&52) && self.clipboard == ClipboardSupport::None {
                    self.clipboard = ClipboardSupport::Write;
                }
                self.attributes = Some(attributes.clone());
            }
            Reply::Mode { mode, state } => {
                self.modes.insert(*mode, *state);
            }
            Reply::KittyKeyboard(_) => self.kitty_keyboard = true,
            Reply::Background(color) => self.background = Some(*color),
            Reply::Clipboard(_) => self.clipboard = ClipboardSupport::ReadWrite,
        }
    }
}

/// The questions, in the order they are asked (DA1 last).
#[must_use]
pub fn questions() -> String {
    let mut out = String::new();
    for mode in PROBED_MODES {
        out.push_str(&seq::ask_mode(mode));
    }
    out.push_str(seq::ASK_KITTY_KEYS);
    out.push_str(seq::ASK_BACKGROUND);
    out.push_str(seq::ASK_ATTRIBUTES);
    out
}

/// The probe's deadline: `RUSTNATIVE_TUI_PROBE_MS`, or [`PROBE_DEADLINE`].
#[must_use]
pub fn deadline() -> Duration {
    std::env::var("RUSTNATIVE_TUI_PROBE_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or(PROBE_DEADLINE, Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{Decoder, InputEvent};
    use crate::emulator::{Answers, Emulator};

    fn profile(answers: Answers, vars: &[(&str, &str)]) -> TerminalProfile {
        let input = DetectInput {
            vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            windows_vt: false,
        };
        let mut profile = TerminalProfile::from_environment(&input, "test".to_owned());
        let mut terminal = Emulator::new(80, 24, answers);
        terminal.feed(questions().as_bytes());
        for event in Decoder::new().feed(&terminal.take_replies()) {
            if let InputEvent::Reply(reply) = event {
                profile.accept(&reply);
            }
        }
        profile
    }

    #[test]
    fn a_terminal_that_answers_is_believed() {
        let profile = profile(Answers::modern(), &[("TERM", "xterm-256color")]);
        assert!(profile.reports_hover());
        assert!(profile.synchronized_output());
        assert_eq!(profile.clipboard, ClipboardSupport::Write, "DA1 says 52");
        assert_eq!(profile.dark_background(), Some(true));
        assert!(!profile.kitty_keyboard);
    }

    #[test]
    fn a_silent_terminal_is_assumed_to_have_nothing() {
        let profile = profile(Answers::silent(), &[("TERM", "vt220")]);
        assert!(!profile.reports_hover());
        assert!(!profile.synchronized_output());
        assert_eq!(profile.clipboard, ClipboardSupport::None);
        assert_eq!(profile.notifications, None);
        assert_eq!(profile.dark_background(), None);
    }

    #[test]
    fn the_environment_names_what_cannot_be_asked() {
        let kitty = profile(Answers::silent(), &[("TERM", "xterm-kitty")]);
        assert_eq!(kitty.notifications, Some(NotificationProtocol::Osc99));
        assert_eq!(kitty.clipboard, ClipboardSupport::Write);
        let ssh = profile(Answers::silent(), &[("SSH_CONNECTION", "1 2 3 4")]);
        assert!(ssh.remote);
        let questions = questions();
        assert!(questions.ends_with(seq::ASK_ATTRIBUTES), "DA1 is the sentinel, asked last");
    }
}
