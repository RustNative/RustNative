//! The reduced form for constrained targets: deferred formatting.
//!
//! A device with no second screen, and no room for formatting code, emits
//! each diagnostic as a frame — a message id from a [`FormatTable`] both
//! sides share, then its arguments — over whatever it has (a debug probe,
//! a serial line); the host holds the table and formats. The device pays
//! for a few bytes per message and no format strings.
//!
//! A frame is `id` (LEB128), the argument count (one byte), then each
//! argument: tag `0` and a LEB128 integer, or tag `1`, a LEB128 length, and
//! UTF-8 bytes.
//!
//! ```
//! use rustnative_core::inspect::compact::{Arg, TRACE, decode, encode};
//!
//! let mut frames = Vec::new();
//! encode(&mut frames, 1, &[Arg::Text("Root/counter".into()), Arg::Text("event".into())]);
//! assert_eq!(decode(&frames, &TRACE)?, ["render Root/counter cause=event"]);
//! # Ok::<(), rustnative_core::inspect::compact::CompactError>(())
//! ```

use std::fmt;

use super::trace::{PassInfo, TraceEntry, TraceKind};

/// Message templates, by id; `{}` is replaced by each argument in turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatTable(&'static [&'static str]);

impl FormatTable {
    /// A table of `formats`.
    #[must_use]
    pub const fn new(formats: &'static [&'static str]) -> Self {
        Self(formats)
    }
}

/// The trace's table: what [`encode_trace`] emits and [`format_trace`]
/// produces directly.
pub const TRACE: FormatTable = FormatTable::new(&[
    "event {} handled={} cost={}us",
    "render {} cause={}",
    "skip {}",
    "tasks delivered cost={}us",
    "edit {} field={} cost={}us",
]);

/// One argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    /// An integer.
    Int(u64),
    /// Text.
    Text(String),
}

impl fmt::Display for Arg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(f, "{value}"),
            Self::Text(value) => f.write_str(value),
        }
    }
}

/// A stream that does not decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactError {
    /// The stream ends inside a frame.
    Truncated,
    /// A message id the table does not have.
    UnknownMessage(u64),
    /// An argument tag other than `0` or `1`.
    BadTag(u8),
    /// Text that is not UTF-8.
    BadText,
}

impl fmt::Display for CompactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the stream ends inside a frame"),
            Self::UnknownMessage(id) => write!(f, "message {id} is not in the format table"),
            Self::BadTag(tag) => write!(f, "argument tag {tag} is neither integer nor text"),
            Self::BadText => f.write_str("a text argument is not UTF-8"),
        }
    }
}

impl std::error::Error for CompactError {}

fn put(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap_or(0);
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn take(bytes: &[u8], at: &mut usize) -> Result<u64, CompactError> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at).ok_or(CompactError::Truncated)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(CompactError::Truncated)
}

/// Appends one frame: message `id` with `args`.
pub fn encode(out: &mut Vec<u8>, id: u16, args: &[Arg]) {
    put(out, u64::from(id));
    out.push(u8::try_from(args.len()).unwrap_or(u8::MAX));
    for arg in args.iter().take(usize::from(u8::MAX)) {
        match arg {
            Arg::Int(value) => {
                out.push(0);
                put(out, *value);
            }
            Arg::Text(text) => {
                out.push(1);
                put(out, u64::try_from(text.len()).unwrap_or(u64::MAX));
                out.extend_from_slice(text.as_bytes());
            }
        }
    }
}

/// Formats every frame in `bytes` with `table`.
///
/// # Errors
///
/// The first frame that does not decode.
pub fn decode(bytes: &[u8], table: &FormatTable) -> Result<Vec<String>, CompactError> {
    let mut at = 0;
    let mut lines = Vec::new();
    while at < bytes.len() {
        let id = take(bytes, &mut at)?;
        let format = usize::try_from(id)
            .ok()
            .and_then(|index| table.0.get(index))
            .ok_or(CompactError::UnknownMessage(id))?;
        let count = *bytes.get(at).ok_or(CompactError::Truncated)?;
        at += 1;
        let mut args = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let tag = *bytes.get(at).ok_or(CompactError::Truncated)?;
            at += 1;
            args.push(match tag {
                0 => Arg::Int(take(bytes, &mut at)?),
                1 => {
                    let length = usize::try_from(take(bytes, &mut at)?)
                        .map_err(|_| CompactError::Truncated)?;
                    let end = at.checked_add(length).ok_or(CompactError::Truncated)?;
                    let text = bytes.get(at..end).ok_or(CompactError::Truncated)?;
                    at = end;
                    Arg::Text(String::from_utf8(text.to_vec()).map_err(|_| CompactError::BadText)?)
                }
                other => return Err(CompactError::BadTag(other)),
            });
        }
        lines.push(fill(format, &args));
    }
    Ok(lines)
}

fn fill(format: &str, args: &[Arg]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut pieces = format.split("{}").peekable();
    while let Some(piece) = pieces.next() {
        out.push_str(piece);
        if pieces.peek().is_some() {
            out.push_str(&args.next().map(ToString::to_string).unwrap_or_default());
        }
    }
    out
}

/// A trace entry's frames and their arguments, in order.
fn frames(entry: &TraceEntry) -> Vec<(u16, Vec<Arg>)> {
    let cost = Arg::Int(entry.micros);
    let text = |value: &str| Arg::Text(value.to_owned());
    let pass_frames = |pass: &PassInfo| {
        let rendered = pass
            .rendered
            .iter()
            .map(|render| (1, vec![text(&render.component), text(&render.cause)]));
        let skipped = pass.skipped.iter().map(|component| (2, vec![text(component)]));
        rendered.chain(skipped).collect::<Vec<_>>()
    };
    let (head, pass) = match &entry.kind {
        TraceKind::Event { event, handled, pass } => {
            ((0, vec![text(event), text(if *handled { "yes" } else { "no" }), cost]), pass.as_ref())
        }
        TraceKind::Tasks { pass } => ((3, vec![cost]), Some(pass)),
        TraceKind::Edit { component, field, pass } => {
            ((4, vec![text(component), text(field), cost]), Some(pass))
        }
        TraceKind::Failure { component, message, attempt } => {
            ((5, vec![text(component), text(message), Arg::Int(u64::from(*attempt))]), None)
        }
    };
    std::iter::once(head).chain(pass.map(pass_frames).unwrap_or_default()).collect()
}

/// `entry` as the device would emit it.
#[must_use]
pub fn encode_trace(entry: &TraceEntry) -> Vec<u8> {
    let mut out = Vec::new();
    for (id, args) in frames(entry) {
        encode(&mut out, id, &args);
    }
    out
}

/// `entry` formatted directly — what the host shows after decoding.
#[must_use]
pub fn format_trace(entry: &TraceEntry) -> Vec<String> {
    frames(entry)
        .into_iter()
        .map(|(id, args)| fill(TRACE.0.get(usize::from(id)).copied().unwrap_or_default(), &args))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::RenderInfo;

    #[test]
    fn a_trace_round_trips_through_the_compact_form() {
        let entry = TraceEntry {
            seq: 3,
            window: 1,
            micros: 1_234_567,
            kind: TraceKind::Event {
                event: "Click { target: NodeId(5) }".into(),
                handled: true,
                pass: Some(PassInfo {
                    rendered: vec![RenderInfo { component: "Root".into(), cause: "event".into() }],
                    skipped: vec!["Root/list".into(), "Root/ünïcode".into()],
                }),
            },
        };
        let bytes = encode_trace(&entry);
        assert_eq!(decode(&bytes, &TRACE).unwrap(), format_trace(&entry));
        assert_eq!(
            format_trace(&entry)[0],
            "event Click { target: NodeId(5) } handled=yes cost=1234567us"
        );
        // Smaller than the JSON it replaces.
        assert!(bytes.len() < serde_json::to_vec(&entry).unwrap().len());
        // Every prefix of a valid stream but the whole is truncated or
        // decodes fewer frames; none panics.
        for end in 0..bytes.len() {
            let _ = decode(&bytes[..end], &TRACE);
        }
        assert_eq!(decode(&[9, 0], &TRACE), Err(CompactError::UnknownMessage(9)));
        assert_eq!(decode(&[0, 1, 7], &TRACE), Err(CompactError::BadTag(7)));
    }
}
