//! Internationalization (`PLAN.md` Milestone 46): messages that are data
//! until they are shown, resolved against the locale where they are shown.
//!
//! A [`Message`] names a catalogue entry and carries its arguments. The
//! typed functions `rustnative_build::compile_messages` generates from the
//! project's catalogues build them, so a missing or extra argument is a
//! compile error. A message becomes text where a node needs text
//! (`Node::label("count", messages::inbox(n))`), in either syntax: a
//! message is an ordinary expression in a builder call and in a markup
//! attribute alike.
//!
//! It becomes text while a component renders. The locale used is the
//! rendering component's environment (`keys::LOCALE`), and the catalogues
//! are those the application's services carry
//! ([`crate::Services::with_catalogues`]). Using a message records that the
//! component read the locale, so a locale switch re-renders exactly the
//! components that show messages (`C15`) and nothing else.
//!
//! Two locales are the pseudo-locales, for the layout suite and the
//! development loop:
//! - [`crate::preview::PSEUDO_LOCALE`] (`en-XA`): the source text accented,
//!   lengthened, and bracketed;
//! - [`PSEUDO_RTL_LOCALE`] (`ar-XB`): the same, laid out right to left.
//!
//! With `RUSTNATIVE_I18N_SHOW_KEYS=1`, every message shows its key (`⟦inbox⟧`)
//! — the translator's view of where each string is.
//!
//! ```
//! use std::sync::Arc;
//! use rustnative_core::i18n::{Catalogues, Message};
//! use rustnative_core::{Component, ComponentTree, Event, Node, Services, keys};
//!
//! struct Inbox;
//! impl Component for Inbox {
//!     type Props = ();
//!     type Message = ();
//!     fn new((): ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node {
//!         Node::label("count", Message::new("inbox").arg("n", 3))
//!     }
//!     fn update(&mut self, _: Event) {}
//! }
//!
//! let catalogues = Catalogues::parse("en", &[
//!     ("en", "inbox = { $n ->\n    [one] One message\n   *[other] { $n } messages\n}\n"),
//!     ("pl", "inbox = { $n ->\n    [one] Jedna wiadomość\n    [few] { $n } wiadomości\n   *[many] { $n } wiadomości\n}\n"),
//! ]).unwrap();
//! let services = Services::default().with_catalogues(Arc::new(catalogues));
//! let mut tree = ComponentTree::with_services(Inbox, services);
//! assert_eq!(tree.view(), Node::label("count", "3 messages"));
//! tree.set_environment(&keys::LOCALE, rustnative_core::Locale::new("pl"));
//! assert_eq!(tree.view(), Node::label("count", "3 wiadomości"));
//! ```
//!
//! <!-- single-syntax: the example is about message resolution, which is the same in both syntaxes -->

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

pub use rustnative_i18n::{Arg, Bundle, Catalogues, PluralCategory, plural_category};

use crate::environment::Locale;

/// The right-to-left pseudo-locale: pseudo-localized text, mirrored layout.
pub const PSEUDO_RTL_LOCALE: &str = "ar-XB";

/// A message to show: a catalogue entry and its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    id: Cow<'static, str>,
    args: Vec<(Cow<'static, str>, Arg)>,
}

impl Message {
    /// The message `id`.
    pub fn new(id: impl Into<Cow<'static, str>>) -> Self {
        Self { id: id.into(), args: Vec::new() }
    }

    /// With argument `name` set to `value`.
    #[must_use]
    pub fn arg(mut self, name: impl Into<Cow<'static, str>>, value: impl Into<Arg>) -> Self {
        self.args.push((name.into(), value.into()));
        self
    }

    /// Its identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its arguments.
    pub fn args(&self) -> impl Iterator<Item = (&str, &Arg)> {
        self.args.iter().map(|(name, value)| (name.as_ref(), value))
    }

    /// This message's text in `locale`, from `catalogues`.
    #[must_use]
    pub fn format(&self, catalogues: &Catalogues, locale: &Locale) -> String {
        if show_keys() {
            return rustnative_i18n::missing(&self.id);
        }
        let args: Vec<(&str, Arg)> =
            self.args.iter().map(|(name, value)| (name.as_ref(), value.clone())).collect();
        let tag = locale.tag();
        if tag == crate::preview::PSEUDO_LOCALE || tag == PSEUDO_RTL_LOCALE {
            let source = catalogues.format(catalogues.source(), &self.id, &args);
            return crate::localization::pseudo_localize(&source);
        }
        catalogues.format(tag, &self.id, &args)
    }
}

fn show_keys() -> bool {
    std::env::var("RUSTNATIVE_I18N_SHOW_KEYS").is_ok_and(|value| value == "1")
}

/// The locale and catalogues a component renders with, and whether it used
/// them.
struct Scope {
    locale: Locale,
    catalogues: Option<Arc<Catalogues>>,
    read: Cell<bool>,
}

thread_local! {
    static SCOPES: RefCell<Vec<Scope>> = const { RefCell::new(Vec::new()) };
}

/// Called by the component tree around each component's render.
pub(crate) fn enter(locale: Locale, catalogues: Option<Arc<Catalogues>>) {
    SCOPES.with(|scopes| {
        scopes.borrow_mut().push(Scope { locale, catalogues, read: Cell::new(false) });
    });
}

/// How many render scopes are open, recorded by an error boundary so the
/// scopes a panic left open can be closed.
pub(crate) fn depth() -> usize {
    SCOPES.with(|scopes| scopes.borrow().len())
}

/// Closes every scope above `depth`.
pub(crate) fn truncate(depth: usize) {
    SCOPES.with(|scopes| scopes.borrow_mut().truncate(depth));
}

/// Ends the innermost scope; returns whether a message was shown in it.
pub(crate) fn leave() -> bool {
    SCOPES.with(|scopes| scopes.borrow_mut().pop().is_some_and(|scope| scope.read.get()))
}

impl From<Message> for String {
    /// The message's text for the component rendering it — or, outside a
    /// render, its key, bracketed, so it is found rather than lost.
    fn from(message: Message) -> Self {
        SCOPES.with(|scopes| {
            let scopes = scopes.borrow();
            match scopes.last() {
                Some(scope) => {
                    scope.read.set(true);
                    match &scope.catalogues {
                        Some(catalogues) => message.format(catalogues, &scope.locale),
                        None => rustnative_i18n::missing(&message.id),
                    }
                }
                None => rustnative_i18n::missing(&message.id),
            }
        })
    }
}

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    /// The year.
    pub year: i32,
    /// The month, 1–12.
    pub month: u8,
    /// The day of the month, 1–31.
    pub day: u8,
}

/// A time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Time {
    /// The hour, 0–23.
    pub hour: u8,
    /// The minute.
    pub minute: u8,
    /// The second.
    pub second: u8,
}

/// How long a date is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    /// Numeric (`9/24/2026`).
    Short,
    /// With the month's name (`Thursday, September 24, 2026`).
    Long,
}

/// A unit a quantity is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unit {
    /// Kilometres.
    Kilometer,
    /// Metres.
    Meter,
    /// Kilograms.
    Kilogram,
    /// Degrees Celsius.
    Celsius,
    /// Bytes.
    Byte,
    /// Percent.
    Percent,
}

impl Unit {
    /// The unit's international symbol.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Kilometer => "km",
            Self::Meter => "m",
            Self::Kilogram => "kg",
            Self::Celsius => "°C",
            Self::Byte => "B",
            Self::Percent => "%",
        }
    }
}

/// Locale-aware formatting, delegated to the host where it has the facility
/// (`PLAN.md` 2.2's text-stack position): numbers, currencies, dates,
/// times, units, collation, and casing.
pub trait LocaleService: Send + Sync {
    /// `value` with `decimals` fraction digits, grouped as `locale` groups.
    fn format_number(&self, locale: &Locale, value: f64, decimals: u8) -> String;
    /// `value` in `currency` (an ISO 4217 code) as `locale` writes money.
    fn format_currency(&self, locale: &Locale, value: f64, currency: &str) -> String;
    /// `date` as `locale` writes dates.
    fn format_date(&self, locale: &Locale, date: Date, style: DateStyle) -> String;
    /// `time` as `locale` writes times.
    fn format_time(&self, locale: &Locale, time: Time) -> String;
    /// `value` of `unit`.
    fn format_unit(&self, locale: &Locale, value: f64, unit: Unit) -> String {
        format!("{} {}", self.format_number(locale, value, 0), unit.symbol())
    }
    /// How `a` sorts against `b` in `locale`.
    fn compare(&self, locale: &Locale, a: &str, b: &str) -> std::cmp::Ordering;
    /// `text` upper-cased by `locale`'s rules.
    fn to_upper(&self, locale: &Locale, text: &str) -> String;
    /// `text` lower-cased by `locale`'s rules.
    fn to_lower(&self, locale: &Locale, text: &str) -> String;
}

/// A deterministic locale service, the same on every machine: what the
/// headless backend uses, so a test's output does not depend on the host.
/// Numbers are grouped by three with `,` and a `.` decimal point; dates are
/// ISO 8601 (`Short`) or `24 September 2026` (`Long`); times are 24-hour;
/// collation compares case-insensitively, then by code point; casing is
/// Unicode's default.
#[derive(Debug, Clone, Copy, Default)]
pub struct InvariantLocale;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl LocaleService for InvariantLocale {
    fn format_number(&self, _: &Locale, value: f64, decimals: u8) -> String {
        let text = format!("{:.*}", usize::from(decimals), value.abs());
        let (whole, fraction) = text.split_once('.').map_or((text.as_str(), ""), |(w, f)| (w, f));
        let mut grouped = String::new();
        for (index, digit) in whole.chars().enumerate() {
            if index > 0 && (whole.len() - index) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        let sign = if value < 0.0 { "-" } else { "" };
        if fraction.is_empty() {
            format!("{sign}{grouped}")
        } else {
            format!("{sign}{grouped}.{fraction}")
        }
    }

    fn format_currency(&self, locale: &Locale, value: f64, currency: &str) -> String {
        format!("{currency} {}", self.format_number(locale, value, 2))
    }

    fn format_date(&self, _: &Locale, date: Date, style: DateStyle) -> String {
        match style {
            DateStyle::Short => format!("{:04}-{:02}-{:02}", date.year, date.month, date.day),
            DateStyle::Long => format!(
                "{} {} {}",
                date.day,
                MONTHS.get(usize::from(date.month.saturating_sub(1))).copied().unwrap_or("?"),
                date.year
            ),
        }
    }

    fn format_time(&self, _: &Locale, time: Time) -> String {
        format!("{:02}:{:02}:{:02}", time.hour, time.minute, time.second)
    }

    fn compare(&self, _: &Locale, a: &str, b: &str) -> std::cmp::Ordering {
        a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
    }

    fn to_upper(&self, _: &Locale, text: &str) -> String {
        text.to_uppercase()
    }

    fn to_lower(&self, _: &Locale, text: &str) -> String {
        text.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_invariant_service_is_the_same_everywhere() {
        let service = InvariantLocale;
        let locale = Locale::default();
        assert_eq!(service.format_number(&locale, 1_234_567.891, 2), "1,234,567.89");
        assert_eq!(service.format_number(&locale, -42.0, 0), "-42");
        assert_eq!(service.format_currency(&locale, 9.5, "EUR"), "EUR 9.50");
        let date = Date { year: 2026, month: 9, day: 24 };
        assert_eq!(service.format_date(&locale, date, DateStyle::Short), "2026-09-24");
        assert_eq!(service.format_date(&locale, date, DateStyle::Long), "24 September 2026");
        assert_eq!(
            service.format_time(&locale, Time { hour: 7, minute: 5, second: 0 }),
            "07:05:00"
        );
        assert_eq!(service.format_unit(&locale, 1500.0, Unit::Meter), "1,500 m");
        assert_eq!(service.compare(&locale, "apple", "Banana"), std::cmp::Ordering::Less);
    }

    #[test]
    fn outside_a_render_a_message_shows_its_key() {
        assert_eq!(String::from(Message::new("greeting")), "⟦greeting⟧");
    }
}
