//! Terminal input, decoded (`PLAN.md` Milestone 38: "input that arrives as
//! escape sequences").
//!
//! Every terminal this backend runs on — a Unix terminal through `termios`,
//! the Windows console in virtual-terminal input mode, a terminal over SSH
//! or a serial line — delivers input as one byte stream. The [`Decoder`]
//! turns it into [`InputEvent`]s:
//!
//! | Bytes | Event |
//! |---|---|
//! | UTF-8 text | a [`Key`] per character, with the text it produces |
//! | C0 controls | Enter, Tab, Backspace, Escape, and Ctrl+letter |
//! | `ESC` + a key | that key with Alt |
//! | `CSI … ~`, `CSI 1;m A`, `SS3 P` … | navigation and function keys, with xterm modifiers |
//! | `CSI code;m u` | the kitty keyboard protocol (Escape and Ctrl+I are then unambiguous) |
//! | `CSI 27;m;code ~` | xterm's `modifyOtherKeys` |
//! | `CSI 200~ … CSI 201~` | one [`InputEvent::Paste`] |
//! | `CSI < b;x;y M/m` | SGR mouse: press, release, drag, motion, wheel |
//! | `CSI I`, `CSI O` | focus in, focus out |
//! | `CSI ? … c`, `CSI ? m;s $ y`, `CSI ? f u`, `OSC 52`, `OSC 11` | replies to [`crate::probe`]'s questions |
//!
//! What a terminal cannot tell apart, it cannot report, and that is a
//! capability answer rather than a bug: without the kitty protocol, Ctrl+I
//! is Tab, Ctrl+M is Enter, Ctrl+H is Ctrl+Backspace, Ctrl+[ is Escape,
//! and Ctrl+Shift+letter is Ctrl+letter (`docs/tui.md` records which
//! terminals report what).
//!
//! A lone `ESC` is ambiguous until the next byte arrives or does not: the
//! decoder holds it, and the loop calls [`Decoder::flush`] once
//! [`ESCAPE_TIMEOUT`] passes with nothing after it.

use std::time::Duration;

use rustnative_core::{Color, KeyCode, KeyModifiers};

/// How long a lone `ESC` waits for a sequence to follow it before it is the
/// Escape key.
pub const ESCAPE_TIMEOUT: Duration = Duration::from_millis(40);

/// Whether a key event is a press, an auto-repeat, or a release (the last
/// two only from terminals that report them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyKind {
    /// Pressed.
    #[default]
    Press,
    /// Held, repeating.
    Repeat,
    /// Released.
    Release,
}

/// One key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    /// Which key.
    pub code: KeyCode,
    /// The modifiers held.
    pub modifiers: KeyModifiers,
    /// The text it types, if it types any.
    pub text: Option<String>,
    /// Press, repeat, or release.
    pub kind: KeyKind,
}

impl Key {
    /// `code` with no modifiers and no text.
    #[must_use]
    pub fn new(code: KeyCode) -> Self {
        Self { code, modifiers: KeyModifiers::default(), text: None, kind: KeyKind::Press }
    }

    /// With `modifiers`.
    #[must_use]
    pub const fn with(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    fn typing(code: KeyCode, text: impl Into<String>) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::default(),
            text: Some(text.into()),
            kind: KeyKind::Press,
        }
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// The primary button.
    Left,
    /// The middle button.
    Middle,
    /// The secondary button.
    Right,
    /// A further button (8–11 in the SGR protocol).
    Other(u8),
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseKind {
    /// A button went down.
    Down(MouseButton),
    /// A button came up (`None` when the terminal does not say which).
    Up(Option<MouseButton>),
    /// The pointer moved with a button held.
    Drag(MouseButton),
    /// The pointer moved with no button held.
    Move,
    /// The wheel turned one notch: (horizontal, vertical), down and right
    /// positive.
    Wheel(i8, i8),
}

/// A mouse event, at a 0-based cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mouse {
    /// What happened.
    pub kind: MouseKind,
    /// The column.
    pub column: u16,
    /// The row.
    pub row: u16,
    /// The modifiers held.
    pub modifiers: KeyModifiers,
}

/// A terminal's answer to a question the backend asked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Primary device attributes (DA1): the terminal's conformance level
    /// and features (`4` sixel, `22` colour, `52` clipboard …).
    PrimaryAttributes(Vec<u16>),
    /// A DEC private mode's state (DECRQM): 0 not recognized, 1 set, 2
    /// reset, 3 permanently set, 4 permanently reset.
    Mode {
        /// The mode.
        mode: u16,
        /// Its state.
        state: u8,
    },
    /// The kitty keyboard protocol's current flags.
    KittyKeyboard(u16),
    /// The clipboard's text (OSC 52).
    Clipboard(String),
    /// The terminal's default background colour (OSC 11).
    Background(Color),
}

/// One decoded input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// A key.
    Key(Key),
    /// Pasted text, whole (bracketed paste).
    Paste(String),
    /// The mouse.
    Mouse(Mouse),
    /// The terminal gained (`true`) or lost focus.
    Focus(bool),
    /// An answer to a probe.
    Reply(Reply),
}

/// The stateful decoder: feed it bytes as they arrive.
#[derive(Debug, Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    paste: Option<Vec<u8>>,
}

const PASTE_END: &[u8] = b"\x1b[201~";

enum Parsed {
    /// Consumed this many bytes, producing these events.
    Done(usize, Vec<InputEvent>),
    /// More bytes are needed.
    Incomplete,
}

fn modifiers_from(param: u16) -> KeyModifiers {
    let bits = param.saturating_sub(1);
    KeyModifiers {
        shift: bits & 1 != 0,
        alt: bits & 2 != 0,
        ctrl: bits & 4 != 0,
        meta: bits & 8 != 0 || bits & 32 != 0,
    }
}

fn function_tilde(code: u16) -> Option<KeyCode> {
    Some(match code {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        11..=15 => KeyCode::Function(u8::try_from(code - 10).ok()?),
        17..=21 => KeyCode::Function(u8::try_from(code - 11).ok()?),
        23..=26 => KeyCode::Function(u8::try_from(code - 12).ok()?),
        28 | 29 => KeyCode::Function(u8::try_from(code - 13).ok()?),
        31..=34 => KeyCode::Function(u8::try_from(code - 14).ok()?),
        _ => return None,
    })
}

/// A Unicode key code (kitty's and `modifyOtherKeys`'), as a key.
fn unicode_key(code: u32) -> Option<KeyCode> {
    Some(match code {
        9 => KeyCode::Tab,
        13 => KeyCode::Enter,
        27 => KeyCode::Escape,
        32 => KeyCode::Space,
        8 | 127 => KeyCode::Backspace,
        // kitty's functional keys, in the private-use area.
        57_348 => KeyCode::Insert,
        57_349 => KeyCode::Delete,
        57_350 => KeyCode::ArrowLeft,
        57_351 => KeyCode::ArrowRight,
        57_352 => KeyCode::ArrowUp,
        57_353 => KeyCode::ArrowDown,
        57_354 => KeyCode::PageUp,
        57_355 => KeyCode::PageDown,
        57_356 => KeyCode::Home,
        57_357 => KeyCode::End,
        57_364..=57_398 => KeyCode::Function(u8::try_from(code - 57_363).ok()?),
        57_414 => KeyCode::Enter,
        57_441..=57_454 => return None, // a modifier key on its own
        _ => KeyCode::Character(char::from_u32(code)?),
    })
}

fn control_key(byte: u8) -> Key {
    let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
    match byte {
        0x00 => Key::new(KeyCode::Space).with(ctrl),
        0x08 => Key::new(KeyCode::Backspace).with(ctrl),
        0x09 => Key::typing(KeyCode::Tab, "\t"),
        0x0A | 0x0D => Key::new(KeyCode::Enter),
        0x1B => Key::new(KeyCode::Escape),
        0x7F => Key::new(KeyCode::Backspace),
        0x01..=0x1A => Key::new(KeyCode::Character(char::from(b'a' + byte - 1))).with(ctrl),
        0x1C => Key::new(KeyCode::Character('\\')).with(ctrl),
        0x1D => Key::new(KeyCode::Character(']')).with(ctrl),
        0x1E => Key::new(KeyCode::Character('^')).with(ctrl),
        _ => Key::new(KeyCode::Character('_')).with(ctrl),
    }
}

fn printable(character: char) -> Key {
    let code = if character == ' ' { KeyCode::Space } else { KeyCode::Character(character) };
    let mut key = Key::typing(code, character.to_string());
    key.modifiers.shift = character.is_uppercase();
    key
}

fn utf8_length(first: u8) -> usize {
    match first {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// Splits CSI parameters: `;` between parameters, `:` between their parts.
fn params(raw: &[u8]) -> Vec<Vec<u16>> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(|byte| *byte == b';')
        .map(|param| {
            param
                .split(|byte| *byte == b':')
                .map(|part| {
                    std::str::from_utf8(part).ok().and_then(|text| text.parse().ok()).unwrap_or(0)
                })
                .collect()
        })
        .collect()
}

fn first(params: &[Vec<u16>], index: usize) -> Option<u16> {
    params.get(index).and_then(|param| param.first().copied())
}

/// Decodes standard base64 (padding optional); `None` on any other byte.
#[must_use]
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |byte: u8| -> Option<u32> {
        Some(u32::from(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let bytes: Vec<u8> = text.bytes().filter(|byte| *byte != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut accumulated = 0_u32;
        for (index, byte) in chunk.iter().enumerate() {
            accumulated |= value(*byte)? << (18 - 6 * index);
        }
        let produced = chunk.len().saturating_sub(1);
        for index in 0..produced {
            out.push(u8::try_from((accumulated >> (16 - 8 * index)) & 0xFF).ok()?);
        }
    }
    Some(out)
}

/// Encodes `bytes` as standard base64 with padding.
#[must_use]
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let accumulated = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (index, byte)| acc | (u32::from(*byte) << (16 - 8 * index)));
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(ALPHABET[((accumulated >> (18 - 6 * index)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn parse_hex_channel(text: &str) -> Option<u8> {
    let value = u32::from_str_radix(text, 16).ok()?;
    let digits = u32::try_from(text.len()).ok()?;
    if digits == 0 || digits > 4 {
        return None;
    }
    let max = (1_u32 << (4 * digits)) - 1;
    u8::try_from(value * 255 / max).ok()
}

impl Decoder {
    /// A decoder with nothing buffered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether bytes are held waiting for the rest of a sequence (or for
    /// [`ESCAPE_TIMEOUT`] to make a lone `ESC` the Escape key).
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.buffer.is_empty() && self.paste.is_none()
    }

    /// Decodes `bytes`, returning every event they complete.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<InputEvent> {
        self.buffer.extend_from_slice(bytes);
        self.drain(false)
    }

    /// The timeout passed with bytes held: a lone `ESC` is the Escape key,
    /// and an unfinished sequence is taken as the keys it is made of.
    pub fn flush(&mut self) -> Vec<InputEvent> {
        self.drain(true)
    }

    fn drain(&mut self, flush: bool) -> Vec<InputEvent> {
        let mut events = Vec::new();
        let mut at = 0;
        while at < self.buffer.len() {
            if let Some(paste) = &mut self.paste {
                let rest = &self.buffer[at..];
                if let Some(end) = rest.windows(PASTE_END.len()).position(|w| w == PASTE_END) {
                    paste.extend_from_slice(&rest[..end]);
                    let text =
                        String::from_utf8_lossy(paste).replace("\r\n", "\n").replace('\r', "\n");
                    events.push(InputEvent::Paste(text));
                    self.paste = None;
                    at += end + PASTE_END.len();
                    continue;
                }
                // Keep what might be the start of the end marker.
                let keep = (1..PASTE_END.len())
                    .rev()
                    .find(|len| rest.len() >= *len && rest.ends_with(&PASTE_END[..*len]))
                    .unwrap_or(0);
                paste.extend_from_slice(&rest[..rest.len() - keep]);
                at = self.buffer.len() - keep;
                break;
            }
            match self.parse(at, flush) {
                Parsed::Done(length, produced) => {
                    at += length.max(1);
                    events.extend(produced);
                }
                Parsed::Incomplete => break,
            }
        }
        self.buffer.drain(..at);
        events
    }

    fn parse(&mut self, at: usize, flush: bool) -> Parsed {
        let bytes = &self.buffer[at..];
        let byte = bytes[0];
        match byte {
            0x1B => self.escape(at, flush),
            0x00..=0x1F | 0x7F => Parsed::Done(1, vec![InputEvent::Key(control_key(byte))]),
            _ => {
                let length = utf8_length(byte);
                if bytes.len() < length && !flush {
                    return Parsed::Incomplete;
                }
                let length = length.min(bytes.len());
                match std::str::from_utf8(&bytes[..length]) {
                    Ok(text) => {
                        let events = text.chars().map(|c| InputEvent::Key(printable(c))).collect();
                        Parsed::Done(length, events)
                    }
                    // Not UTF-8 (a terminal in a legacy encoding): one
                    // replacement character, never a crash.
                    Err(_) => Parsed::Done(1, vec![InputEvent::Key(printable('\u{FFFD}'))]),
                }
            }
        }
    }

    fn escape(&mut self, at: usize, flush: bool) -> Parsed {
        let bytes = &self.buffer[at..];
        let Some(&next) = bytes.get(1) else {
            return if flush {
                Parsed::Done(1, vec![InputEvent::Key(Key::new(KeyCode::Escape))])
            } else {
                Parsed::Incomplete
            };
        };
        match next {
            b'[' => self.csi(at, flush),
            b'O' => match bytes.get(2) {
                None if !flush => Parsed::Incomplete,
                None => Parsed::Done(2, vec![alt(printable('O'))]),
                Some(&final_byte) => {
                    let code = match final_byte {
                        b'P' => Some(KeyCode::Function(1)),
                        b'Q' => Some(KeyCode::Function(2)),
                        b'R' => Some(KeyCode::Function(3)),
                        b'S' => Some(KeyCode::Function(4)),
                        b'A' => Some(KeyCode::ArrowUp),
                        b'B' => Some(KeyCode::ArrowDown),
                        b'C' => Some(KeyCode::ArrowRight),
                        b'D' => Some(KeyCode::ArrowLeft),
                        b'H' => Some(KeyCode::Home),
                        b'F' => Some(KeyCode::End),
                        b'M' => Some(KeyCode::Enter),
                        _ => None,
                    };
                    let event = code.map_or_else(
                        || InputEvent::Key(Key::new(KeyCode::Unknown(u32::from(final_byte)))),
                        |code| InputEvent::Key(Key::new(code)),
                    );
                    Parsed::Done(3, vec![event])
                }
            },
            b']' | b'P' | b'_' | b'^' => self.string(at, flush),
            0x1B => {
                // ESC ESC: Alt+Escape, unless a sequence follows the second.
                if bytes.len() == 2 && !flush {
                    return Parsed::Incomplete;
                }
                if matches!(bytes.get(2), Some(b'[' | b'O')) {
                    return Parsed::Done(1, vec![InputEvent::Key(Key::new(KeyCode::Escape))]);
                }
                Parsed::Done(2, vec![alt(Key::new(KeyCode::Escape))])
            }
            _ => {
                // Alt + the key that follows.
                match self.parse(at + 1, flush) {
                    Parsed::Done(length, events) => {
                        let events = events
                            .into_iter()
                            .map(|event| match event {
                                InputEvent::Key(key) => alt(key),
                                other => other,
                            })
                            .collect();
                        Parsed::Done(length + 1, events)
                    }
                    Parsed::Incomplete => Parsed::Incomplete,
                }
            }
        }
    }

    /// OSC, DCS, APC, and PM strings, up to BEL or ST.
    fn string(&mut self, at: usize, flush: bool) -> Parsed {
        let bytes = &self.buffer[at..];
        let mut index = 2;
        while index < bytes.len() {
            if bytes[index] == 0x07 {
                let body = bytes[2..index].to_vec();
                return Parsed::Done(index + 1, reply_of(bytes[1], &body));
            }
            if bytes[index] == 0x1B {
                match bytes.get(index + 1) {
                    Some(b'\\') => {
                        let body = bytes[2..index].to_vec();
                        return Parsed::Done(index + 2, reply_of(bytes[1], &body));
                    }
                    None if !flush => return Parsed::Incomplete,
                    _ => return Parsed::Done(index, Vec::new()),
                }
            }
            index += 1;
        }
        if flush || bytes.len() > 1 << 20 {
            Parsed::Done(bytes.len(), Vec::new())
        } else {
            Parsed::Incomplete
        }
    }

    fn csi(&mut self, at: usize, flush: bool) -> Parsed {
        let bytes = &self.buffer[at..];
        let mut index = 2;
        while index < bytes.len() && (0x20..=0x3F).contains(&bytes[index]) {
            index += 1;
        }
        let Some(&final_byte) = bytes.get(index) else {
            return if flush {
                // ESC [ with nothing after it: Alt+[.
                Parsed::Done(2, vec![alt(printable('['))])
            } else {
                Parsed::Incomplete
            };
        };
        if !(0x40..=0x7E).contains(&final_byte) {
            // Not a sequence after all: drop the introducer.
            return Parsed::Done(2, Vec::new());
        }
        let body = &bytes[2..index];
        let (private, body) = match body.first() {
            Some(marker @ (b'?' | b'<' | b'>' | b'=')) => (Some(*marker), &body[1..]),
            _ => (None, body),
        };
        let intermediates: Vec<u8> =
            body.iter().copied().filter(|b| (0x20..=0x2F).contains(b)).collect();
        let raw: Vec<u8> = body.iter().copied().filter(|b| (0x30..=0x3F).contains(b)).collect();
        let params = params(&raw);
        let length = index + 1;
        if private.is_none() && final_byte == b'~' && first(&params, 0) == Some(200) {
            self.paste = Some(Vec::new());
            return Parsed::Done(length, Vec::new());
        }
        Parsed::Done(length, csi_event(private, &intermediates, &params, final_byte))
    }
}

fn alt(mut key: Key) -> InputEvent {
    key.modifiers.alt = true;
    // Alt+a types nothing; Alt is a command modifier on every host.
    key.text = None;
    InputEvent::Key(key)
}

fn reply_of(kind: u8, body: &[u8]) -> Vec<InputEvent> {
    if kind != b']' {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(body);
    let mut parts = text.splitn(3, ';');
    match parts.next() {
        Some("52") => {
            let _selection = parts.next();
            let data = parts.next().unwrap_or("");
            base64_decode(data)
                .map(|bytes| {
                    vec![InputEvent::Reply(Reply::Clipboard(
                        String::from_utf8_lossy(&bytes).into_owned(),
                    ))]
                })
                .unwrap_or_default()
        }
        Some("11") => {
            let value = parts.next().unwrap_or("");
            let channels: Option<Vec<u8>> = value
                .strip_prefix("rgb:")
                .map(|rgb| rgb.split('/').map(parse_hex_channel).collect::<Option<Vec<u8>>>())
                .unwrap_or(None);
            match channels.as_deref() {
                Some([r, g, b]) => {
                    vec![InputEvent::Reply(Reply::Background(Color::rgb(*r, *g, *b)))]
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn mouse_event(params: &[Vec<u16>], released: bool) -> Option<InputEvent> {
    let code = first(params, 0)?;
    let column = first(params, 1)?.saturating_sub(1);
    let row = first(params, 2)?.saturating_sub(1);
    let modifiers = KeyModifiers {
        shift: code & 4 != 0,
        alt: code & 8 != 0,
        ctrl: code & 16 != 0,
        meta: false,
    };
    let motion = code & 32 != 0;
    let button_bits = code & 3;
    let button = if code & 128 != 0 {
        Some(MouseButton::Other(u8::try_from(8 + button_bits).unwrap_or(8)))
    } else {
        match button_bits {
            0 => Some(MouseButton::Left),
            1 => Some(MouseButton::Middle),
            2 => Some(MouseButton::Right),
            _ => None,
        }
    };
    let kind = if code & 64 != 0 {
        match button_bits {
            0 => MouseKind::Wheel(0, -1),
            1 => MouseKind::Wheel(0, 1),
            2 => MouseKind::Wheel(-1, 0),
            _ => MouseKind::Wheel(1, 0),
        }
    } else if released {
        MouseKind::Up(button)
    } else if motion {
        button.map_or(MouseKind::Move, MouseKind::Drag)
    } else {
        MouseKind::Down(button?)
    };
    Some(InputEvent::Mouse(Mouse { kind, column, row, modifiers }))
}

fn csi_event(
    private: Option<u8>,
    intermediates: &[u8],
    params: &[Vec<u16>],
    final_byte: u8,
) -> Vec<InputEvent> {
    let modifiers = first(params, 1).map_or_else(KeyModifiers::default, modifiers_from);
    let key = |code: KeyCode| vec![InputEvent::Key(Key::new(code).with(modifiers))];
    match (private, final_byte) {
        (Some(b'<'), b'M') => mouse_event(params, false).into_iter().collect(),
        (Some(b'<'), b'm') => mouse_event(params, true).into_iter().collect(),
        (Some(b'?'), b'c') => {
            vec![InputEvent::Reply(Reply::PrimaryAttributes(
                params.iter().filter_map(|p| p.first().copied()).collect(),
            ))]
        }
        (Some(b'?'), b'y') if intermediates == b"$" => {
            let mode = first(params, 0).unwrap_or(0);
            let state = u8::try_from(first(params, 1).unwrap_or(0)).unwrap_or(0);
            vec![InputEvent::Reply(Reply::Mode { mode, state })]
        }
        (Some(b'?'), b'u') => {
            vec![InputEvent::Reply(Reply::KittyKeyboard(first(params, 0).unwrap_or(0)))]
        }
        (Some(_), _) => Vec::new(),
        (None, b'A') => key(KeyCode::ArrowUp),
        (None, b'B') => key(KeyCode::ArrowDown),
        (None, b'C') => key(KeyCode::ArrowRight),
        (None, b'D') => key(KeyCode::ArrowLeft),
        (None, b'H') => key(KeyCode::Home),
        (None, b'F') => key(KeyCode::End),
        (None, b'P') => key(KeyCode::Function(1)),
        (None, b'Q') => key(KeyCode::Function(2)),
        (None, b'R') => key(KeyCode::Function(3)),
        (None, b'S') => key(KeyCode::Function(4)),
        (None, b'Z') => {
            let mut shifted = modifiers;
            shifted.shift = true;
            vec![InputEvent::Key(Key::new(KeyCode::Tab).with(shifted))]
        }
        (None, b'I') if params.is_empty() => vec![InputEvent::Focus(true)],
        (None, b'O') if params.is_empty() => vec![InputEvent::Focus(false)],
        (None, b'~') => {
            let code = first(params, 0).unwrap_or(0);
            if code == 27 {
                // modifyOtherKeys: CSI 27 ; modifiers ; code ~
                let modifiers = first(params, 1).map_or_else(KeyModifiers::default, modifiers_from);
                return first(params, 2)
                    .and_then(|code| unicode_key(u32::from(code)))
                    .map(|code| {
                        vec![InputEvent::Key(modified_key(code, modifiers, None, KeyKind::Press))]
                    })
                    .unwrap_or_default();
            }
            function_tilde(code).map(key).unwrap_or_default()
        }
        (None, b'u') => {
            // kitty: CSI code[:shifted[:base]] ; modifiers[:event] ; text u
            let code = u32::from(first(params, 0).unwrap_or(0));
            let modifiers = first(params, 1).map_or_else(KeyModifiers::default, modifiers_from);
            let kind = match params.get(1).and_then(|param| param.get(1)) {
                Some(2) => KeyKind::Repeat,
                Some(3) => KeyKind::Release,
                _ => KeyKind::Press,
            };
            let text: Option<String> = params
                .get(2)
                .map(|codes| codes.iter().filter_map(|c| char::from_u32(u32::from(*c))).collect());
            unicode_key(code)
                .map(|code| vec![InputEvent::Key(modified_key(code, modifiers, text, kind))])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// A key a protocol reported with explicit modifiers: it types its
/// character only with no command modifier held.
fn modified_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    text: Option<String>,
    kind: KeyKind,
) -> Key {
    let typed = if modifiers.ctrl || modifiers.alt || modifiers.meta {
        None
    } else {
        text.or_else(|| match code {
            KeyCode::Character(c) => {
                Some(if modifiers.shift { c.to_uppercase().collect() } else { c.to_string() })
            }
            KeyCode::Space => Some(" ".to_owned()),
            _ => None,
        })
    };
    let code = match (code, &typed) {
        (KeyCode::Character(_), Some(text)) => text.chars().next().map_or(code, KeyCode::Character),
        _ => code,
    };
    Key { code, modifiers, text: typed, kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(bytes: &[u8]) -> Vec<InputEvent> {
        let mut decoder = Decoder::new();
        let mut events = decoder.feed(bytes);
        events.extend(decoder.flush());
        events
    }

    fn key(code: KeyCode, shift: bool, alt: bool, ctrl: bool) -> Key {
        Key::new(code).with(KeyModifiers { shift, alt, ctrl, meta: false })
    }

    fn only_key(bytes: &[u8]) -> Key {
        match keys(bytes).as_slice() {
            [InputEvent::Key(key)] => key.clone(),
            other => panic!("{bytes:?} decoded to {other:?}"),
        }
    }

    #[test]
    fn text_types_itself() {
        let typed: Vec<_> = keys("aÄ日🙂".as_bytes())
            .into_iter()
            .map(|event| match event {
                InputEvent::Key(key) => key.text.unwrap_or_default(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(typed, ["a", "Ä", "日", "🙂"]);
        assert!(only_key(b"A").modifiers.shift);
        assert_eq!(only_key(b" ").code, KeyCode::Space);
    }

    #[test]
    fn utf8_split_across_reads_waits_for_its_end() {
        let mut decoder = Decoder::new();
        let bytes = "日".as_bytes();
        assert!(decoder.feed(&bytes[..1]).is_empty());
        assert!(decoder.has_pending());
        assert_eq!(decoder.feed(&bytes[1..]).len(), 1);
    }

    #[test]
    fn controls_are_their_keys() {
        assert_eq!(only_key(b"\r").code, KeyCode::Enter);
        assert_eq!(only_key(b"\t").code, KeyCode::Tab);
        assert_eq!(only_key(b"\x7f").code, KeyCode::Backspace);
        assert_eq!(only_key(b"\x08"), key(KeyCode::Backspace, false, false, true));
        assert_eq!(only_key(b"\x01"), key(KeyCode::Character('a'), false, false, true));
        assert_eq!(only_key(b"\x00"), key(KeyCode::Space, false, false, true));
        assert_eq!(only_key(b"\x1b").code, KeyCode::Escape);
    }

    #[test]
    fn a_lone_escape_waits_for_the_timeout() {
        let mut decoder = Decoder::new();
        assert!(decoder.feed(b"\x1b").is_empty());
        assert!(decoder.has_pending());
        assert_eq!(decoder.flush(), vec![InputEvent::Key(Key::new(KeyCode::Escape))]);
        assert!(!decoder.has_pending());
    }

    #[test]
    fn alt_is_an_escape_prefix() {
        assert_eq!(only_key(b"\x1bx"), key(KeyCode::Character('x'), false, true, false));
        assert_eq!(only_key(b"\x1b\x1b"), key(KeyCode::Escape, false, true, false));
        assert_eq!(only_key(b"\x1b\x7f"), key(KeyCode::Backspace, false, true, false));
    }

    #[test]
    fn navigation_and_function_keys_with_xterm_modifiers() {
        assert_eq!(only_key(b"\x1b[A").code, KeyCode::ArrowUp);
        assert_eq!(only_key(b"\x1bOB").code, KeyCode::ArrowDown);
        assert_eq!(only_key(b"\x1b[1;5C"), key(KeyCode::ArrowRight, false, false, true));
        assert_eq!(only_key(b"\x1b[1;2D"), key(KeyCode::ArrowLeft, true, false, false));
        assert_eq!(only_key(b"\x1b[1;3H"), key(KeyCode::Home, false, true, false));
        assert_eq!(only_key(b"\x1b[4~").code, KeyCode::End);
        assert_eq!(only_key(b"\x1b[3;5~"), key(KeyCode::Delete, false, false, true));
        assert_eq!(only_key(b"\x1b[5~").code, KeyCode::PageUp);
        assert_eq!(only_key(b"\x1b[6~").code, KeyCode::PageDown);
        assert_eq!(only_key(b"\x1b[2~").code, KeyCode::Insert);
        assert_eq!(only_key(b"\x1bOP").code, KeyCode::Function(1));
        assert_eq!(only_key(b"\x1b[1;2Q"), key(KeyCode::Function(2), true, false, false));
        for (sequence, number) in [
            ("15", 5),
            ("17", 6),
            ("18", 7),
            ("19", 8),
            ("20", 9),
            ("21", 10),
            ("23", 11),
            ("24", 12),
        ] {
            assert_eq!(
                only_key(format!("\x1b[{sequence}~").as_bytes()).code,
                KeyCode::Function(number)
            );
        }
        assert_eq!(only_key(b"\x1b[Z"), key(KeyCode::Tab, true, false, false));
    }

    #[test]
    fn the_kitty_protocol_and_modify_other_keys() {
        assert_eq!(only_key(b"\x1b[27u").code, KeyCode::Escape);
        assert_eq!(only_key(b"\x1b[105;5u"), key(KeyCode::Character('i'), false, false, true));
        assert_eq!(only_key(b"\x1b[13;2u"), key(KeyCode::Enter, true, false, false));
        let typed = only_key(b"\x1b[97;2u");
        assert_eq!(typed.text.as_deref(), Some("A"));
        assert_eq!(typed.code, KeyCode::Character('A'));
        assert_eq!(only_key(b"\x1b[97;1:3u").kind, KeyKind::Release);
        assert_eq!(only_key(b"\x1b[57364u").code, KeyCode::Function(1));
        assert_eq!(only_key(b"\x1b[27;6;65~"), key(KeyCode::Character('A'), true, false, true));
        assert!(keys(b"\x1b[57441u").is_empty(), "a modifier alone is no key");
    }

    #[test]
    fn bracketed_paste_is_one_event_however_it_arrives() {
        let mut decoder = Decoder::new();
        let mut events = decoder.feed(b"\x1b[200~line one\r\nline");
        events.extend(decoder.feed(b" two\x1b[20"));
        events.extend(decoder.feed(b"1~x"));
        assert_eq!(
            events,
            vec![
                InputEvent::Paste("line one\nline two".to_owned()),
                InputEvent::Key(printable('x'))
            ]
        );
        assert_eq!(
            keys(b"\x1b[200~\x1b[A\x03\x1b[201~"),
            vec![InputEvent::Paste("\x1b[A\x03".to_owned())],
            "keys inside a paste are text"
        );
    }

    #[test]
    fn sgr_mouse() {
        let event = |bytes: &[u8]| match keys(bytes).as_slice() {
            [InputEvent::Mouse(mouse)] => *mouse,
            other => panic!("{other:?}"),
        };
        let down = event(b"\x1b[<0;10;5M");
        assert_eq!((down.kind, down.column, down.row), (MouseKind::Down(MouseButton::Left), 9, 4));
        assert_eq!(event(b"\x1b[<0;10;5m").kind, MouseKind::Up(Some(MouseButton::Left)));
        assert_eq!(event(b"\x1b[<32;11;5M").kind, MouseKind::Drag(MouseButton::Left));
        assert_eq!(event(b"\x1b[<35;1;1M").kind, MouseKind::Move);
        assert_eq!(event(b"\x1b[<64;1;1M").kind, MouseKind::Wheel(0, -1));
        assert_eq!(event(b"\x1b[<65;1;1M").kind, MouseKind::Wheel(0, 1));
        assert_eq!(event(b"\x1b[<2;1;1M").kind, MouseKind::Down(MouseButton::Right));
        assert!(event(b"\x1b[<16;1;1M").modifiers.ctrl);
    }

    #[test]
    fn focus_and_replies() {
        assert_eq!(keys(b"\x1b[I"), vec![InputEvent::Focus(true)]);
        assert_eq!(keys(b"\x1b[O"), vec![InputEvent::Focus(false)]);
        assert_eq!(
            keys(b"\x1b[?62;22;52c"),
            vec![InputEvent::Reply(Reply::PrimaryAttributes(vec![62, 22, 52]))]
        );
        assert_eq!(
            keys(b"\x1b[?2026;2$y"),
            vec![InputEvent::Reply(Reply::Mode { mode: 2026, state: 2 })]
        );
        assert_eq!(keys(b"\x1b[?1u"), vec![InputEvent::Reply(Reply::KittyKeyboard(1))]);
        assert_eq!(
            keys(b"\x1b]52;c;aGVsbG8=\x07"),
            vec![InputEvent::Reply(Reply::Clipboard("hello".to_owned()))]
        );
        assert_eq!(
            keys(b"\x1b]11;rgb:ffff/0000/8080\x1b\\"),
            vec![InputEvent::Reply(Reply::Background(Color::rgb(255, 0, 128)))]
        );
        assert!(keys(b"\x1bP1$r0m\x1b\\").is_empty(), "an unasked DCS is dropped");
    }

    #[test]
    fn garbage_never_panics() {
        let mut decoder = Decoder::new();
        let mut state = 0x1234_5678_u32;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let byte = u8::try_from(state & 0xFF).unwrap_or(0);
            let _ = decoder.feed(&[byte]);
        }
        let _ = decoder.flush();
    }

    #[test]
    fn base64_round_trips() {
        for text in ["", "a", "ab", "abc", "hello, terminal ✓"] {
            assert_eq!(
                base64_decode(&base64_encode(text.as_bytes())).as_deref(),
                Some(text.as_bytes())
            );
        }
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_decode("!!"), None);
    }
}
