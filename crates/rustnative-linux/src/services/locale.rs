//! Locale-aware formatting through glibc's locales (`PLAN.md` Milestone
//! 46): numbers, currencies, dates, times, collation, and casing are the C
//! library's, from the locale data the system has installed — what every
//! Linux program that formats for a locale uses.
//!
//! A BCP 47 tag names a glibc locale (`de-DE` is `de_DE.UTF-8`,
//! `sr-Latn-RS` is `sr_RS.UTF-8@latin`). A tag the system has no locale for
//! falls back to the person's own locale from the environment, as an
//! unknown locale falls back on Windows.
//!
//! A currency is formatted by `strfmon` when it is the locale's own
//! (`INT_CURR_SYMBOL`); another currency is the locale's number with the
//! ISO code before it, as on Windows. glibc has no long date format; the
//! long date is the short one's day/month/year order with the month's
//! name.

use std::cmp::Ordering;
use std::ffi::{CStr, CString, c_char, c_int};

use rustnative_core::Locale;
use rustnative_core::i18n::{Date, DateStyle, LocaleService, Time};

/// glibc's locale services.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxLocale;

// `wint_t` is `unsigned int` in glibc.
unsafe extern "C" {
    fn strfmon_l(
        buffer: *mut c_char,
        size: libc::size_t,
        locale: libc::locale_t,
        format: *const c_char,
        ...
    ) -> libc::ssize_t;
    fn strcoll_l(a: *const c_char, b: *const c_char, locale: libc::locale_t) -> c_int;
    fn towupper_l(character: u32, locale: libc::locale_t) -> u32;
    fn towlower_l(character: u32, locale: libc::locale_t) -> u32;
}

/// `INT_CURR_SYMBOL`: the ISO 4217 code and a separator (`"EUR "`).
const INT_CURR_SYMBOL: libc::nl_item = 0x40000;

/// A glibc locale object, freed on drop.
struct Loaded(libc::locale_t);

impl Drop for Loaded {
    fn drop(&mut self) {
        // SAFETY: created by `newlocale` and freed only here.
        unsafe { libc::freelocale(self.0) };
    }
}

/// The glibc locale names a BCP 47 tag may be installed under.
fn candidates(tag: &str) -> Vec<String> {
    let mut language = None;
    let mut script = None;
    let mut region = None;
    for (index, part) in tag.split(['-', '_']).enumerate() {
        match (index, part.len()) {
            (0, _) => language = Some(part.to_ascii_lowercase()),
            (_, 4) if script.is_none() && region.is_none() => {
                script = Some(part.to_ascii_lowercase());
            }
            (_, 2 | 3) if region.is_none() => region = Some(part.to_ascii_uppercase()),
            _ => {}
        }
    }
    let Some(language) = language.filter(|language| !language.is_empty()) else {
        return Vec::new();
    };
    let modifier = match script.as_deref() {
        Some("latn") => "@latin",
        Some("cyrl") => "@cyrillic",
        Some("deva") => "@devanagari",
        _ => "",
    };
    let mut names = Vec::new();
    if let Some(region) = &region {
        names.push(format!("{language}_{region}.UTF-8{modifier}"));
    }
    names.push(format!("{language}.UTF-8{modifier}"));
    names
}

fn load(locale: &Locale) -> Option<Loaded> {
    let mut names = candidates(locale.tag());
    // The person's own locale, then the C library's UTF-8 default.
    names.extend([String::new(), "C.UTF-8".to_owned()]);
    names.into_iter().find_map(|name| {
        let name = CString::new(name).ok()?;
        // SAFETY: `name` is NUL-terminated; a null base asks for a new
        // object, which the returned `Loaded` frees.
        let handle =
            unsafe { libc::newlocale(libc::LC_ALL_MASK, name.as_ptr(), std::ptr::null_mut()) };
        (!handle.is_null()).then_some(Loaded(handle))
    })
}

fn info(locale: &Loaded, item: libc::nl_item) -> String {
    // SAFETY: `locale` is live; glibc returns a NUL-terminated string it
    // owns, copied before the locale is freed.
    unsafe { CStr::from_ptr(libc::nl_langinfo_l(item, locale.0)) }.to_string_lossy().into_owned()
}

/// Runs `call` with `locale` as this thread's locale, so the C library's
/// `printf` family formats as it does.
fn in_locale<R>(locale: &Loaded, call: impl FnOnce() -> R) -> R {
    // SAFETY: `uselocale` swaps the calling thread's locale only; the
    // previous one is restored before `locale` can be freed.
    let previous = unsafe { libc::uselocale(locale.0) };
    let result = call();
    // SAFETY: as above.
    unsafe { libc::uselocale(previous) };
    result
}

/// `printf("%'.*f")`: the locale's decimal point and digit grouping.
fn grouped(locale: &Loaded, value: f64, decimals: u8) -> String {
    in_locale(locale, || {
        let mut buffer = vec![0_u8; 128];
        // SAFETY: the format takes an `int` precision and a `double`; the
        // buffer and its size match.
        let written = unsafe {
            libc::snprintf(
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                c"%'.*f".as_ptr(),
                c_int::from(decimals),
                value,
            )
        };
        buffer.truncate(usize::try_from(written).unwrap_or(0).min(buffer.len() - 1));
        String::from_utf8_lossy(&buffer).into_owned()
    })
}

fn strftime(locale: &Loaded, format: &str, date: Date, time: Time) -> String {
    let Ok(format) = CString::new(format) else {
        return String::new();
    };
    // SAFETY: `tm` is plain data; zeroed is a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = date.year - 1900;
    tm.tm_mon = c_int::from(date.month) - 1;
    tm.tm_mday = c_int::from(date.day);
    tm.tm_hour = c_int::from(time.hour);
    tm.tm_min = c_int::from(time.minute);
    tm.tm_sec = c_int::from(time.second);
    tm.tm_wday = weekday(date);
    let mut buffer = vec![0_u8; 256];
    // SAFETY: the buffer and its size match; `format` is NUL-terminated.
    let written = unsafe {
        libc::strftime_l(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            format.as_ptr(),
            &raw const tm,
            locale.0,
        )
    };
    buffer.truncate(written);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Sunday = 0, by Sakamoto's method.
fn weekday(date: Date) -> c_int {
    const OFFSETS: [c_int; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let month = usize::from(date.month.clamp(1, 12));
    let year = if month < 3 { date.year - 1 } else { date.year };
    (year + year / 4 - year / 100 + year / 400 + OFFSETS[month - 1] + c_int::from(date.day))
        .rem_euclid(7)
}

/// The long date: the short format's field order, with the month named
/// and the separator the locale puts after a leading day (`24. September`).
fn long_date_format(short: &str) -> String {
    let order: Vec<char> = short
        .split('%')
        .skip(1)
        .filter_map(|field| field.chars().find(char::is_ascii_alphabetic))
        .filter(|c| matches!(c, 'd' | 'e' | 'm' | 'b' | 'B' | 'y' | 'Y'))
        .collect();
    match order.first() {
        Some('d' | 'e') => {
            let after_day = if short.contains("%d.") || short.contains("%e.") { "." } else { "" };
            format!("%A, %-d{after_day} %B %Y")
        }
        Some('y' | 'Y') => "%Y %B %-d, %A".to_owned(),
        _ => "%A, %B %-d, %Y".to_owned(),
    }
}

fn map_case(locale: &Locale, text: &str, upper: bool) -> String {
    let Some(loaded) = load(locale) else {
        return if upper { text.to_uppercase() } else { text.to_lowercase() };
    };
    text.chars()
        .map(|character| {
            // SAFETY: plain conversions of one wide character in a live locale.
            let mapped = unsafe {
                if upper {
                    towupper_l(u32::from(character), loaded.0)
                } else {
                    towlower_l(u32::from(character), loaded.0)
                }
            };
            char::from_u32(mapped).unwrap_or(character)
        })
        .collect()
}

impl LocaleService for LinuxLocale {
    fn format_number(&self, locale: &Locale, value: f64, decimals: u8) -> String {
        load(locale).map_or_else(
            || format!("{value:.*}", usize::from(decimals)),
            |loaded| grouped(&loaded, value, decimals),
        )
    }

    fn format_currency(&self, locale: &Locale, value: f64, currency: &str) -> String {
        let Some(loaded) = load(locale) else {
            return format!("{currency} {value:.2}");
        };
        if info(&loaded, INT_CURR_SYMBOL).trim().eq_ignore_ascii_case(currency) {
            let mut buffer = vec![0_u8; 128];
            // SAFETY: `%n` takes one `double`; the buffer and its size match.
            let written = unsafe {
                strfmon_l(buffer.as_mut_ptr().cast(), buffer.len(), loaded.0, c"%n".as_ptr(), value)
            };
            if let Ok(written) = usize::try_from(written) {
                buffer.truncate(written);
                // glibc separates with a no-break space where the locale does.
                return String::from_utf8_lossy(&buffer).replace('\u{a0}', " ");
            }
        }
        format!("{currency} {}", grouped(&loaded, value, 2))
    }

    fn format_date(&self, locale: &Locale, date: Date, style: DateStyle) -> String {
        let Some(loaded) = load(locale) else {
            return format!("{:04}-{:02}-{:02}", date.year, date.month, date.day);
        };
        let short = info(&loaded, libc::D_FMT);
        let format = match style {
            DateStyle::Short => short,
            DateStyle::Long => long_date_format(&short),
        };
        strftime(&loaded, &format, date, Time { hour: 0, minute: 0, second: 0 })
    }

    fn format_time(&self, locale: &Locale, time: Time) -> String {
        let Some(loaded) = load(locale) else {
            return format!("{:02}:{:02}:{:02}", time.hour, time.minute, time.second);
        };
        let format = info(&loaded, libc::T_FMT);
        strftime(&loaded, &format, Date { year: 2000, month: 1, day: 1 }, time)
    }

    fn compare(&self, locale: &Locale, a: &str, b: &str) -> Ordering {
        let (Some(loaded), Ok(first), Ok(second)) =
            (load(locale), CString::new(a), CString::new(b))
        else {
            return a.cmp(b);
        };
        // SAFETY: both strings are NUL-terminated; the locale is live.
        unsafe { strcoll_l(first.as_ptr(), second.as_ptr(), loaded.0) }.cmp(&0)
    }

    fn to_upper(&self, locale: &Locale, text: &str) -> String {
        map_case(locale, text, true)
    }

    fn to_lower(&self, locale: &Locale, text: &str) -> String {
        map_case(locale, text, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether the system has generated `name` (`locale-gen`); the
    /// formatting tests need the locales they format for.
    fn installed(tag: &str) -> bool {
        candidates(tag).first().is_some_and(|name| {
            let name = CString::new(name.as_str()).expect("no NUL");
            // SAFETY: as in `load`.
            let handle =
                unsafe { libc::newlocale(libc::LC_ALL_MASK, name.as_ptr(), std::ptr::null_mut()) };
            !handle.is_null() && {
                drop(Loaded(handle));
                true
            }
        })
    }

    #[test]
    fn a_tag_names_the_glibc_locale() {
        assert_eq!(candidates("de-DE"), ["de_DE.UTF-8", "de.UTF-8"]);
        assert_eq!(candidates("sr-Latn-RS"), ["sr_RS.UTF-8@latin", "sr.UTF-8@latin"]);
        assert_eq!(candidates("fr"), ["fr.UTF-8"]);
        assert_eq!(long_date_format("%d.%m.%Y"), "%A, %-d. %B %Y");
        assert_eq!(long_date_format("%m/%d/%Y"), "%A, %B %-d, %Y");
        assert_eq!(weekday(Date { year: 2026, month: 9, day: 24 }), 4, "a Thursday");
    }

    #[test]
    fn glibc_formats_as_each_locale_writes() {
        let tags = ["en-NZ", "de-DE", "fr-FR", "tr-TR"];
        if let Some(missing) = tags.iter().find(|tag| !installed(tag)) {
            // `locale-gen en_NZ.UTF-8 de_DE.UTF-8 fr_FR.UTF-8 tr_TR.UTF-8`.
            eprintln!("skipped: the {missing} locale is not generated on this system");
            return;
        }
        let service = LinuxLocale;
        let (nz, de, fr) = (Locale::new("en-NZ"), Locale::new("de-DE"), Locale::new("fr-FR"));
        assert_eq!(service.format_number(&nz, 1_234_567.891, 2), "1,234,567.89");
        assert_eq!(service.format_number(&de, 1_234_567.891, 2), "1.234.567,89");
        let french = service.format_number(&fr, 1_234.5, 1);
        assert!(french.ends_with(",5") && french.starts_with('1'), "{french}");
        assert_eq!(service.format_currency(&de, 9.5, "EUR"), "9,50 €");
        assert_eq!(service.format_currency(&de, 9.5, "USD"), "USD 9,50", "not the locale's own");
        let date = Date { year: 2026, month: 9, day: 24 };
        assert_eq!(service.format_date(&de, date, DateStyle::Short), "24.09.2026");
        assert_eq!(
            service.format_date(&de, date, DateStyle::Long),
            "Donnerstag, 24. September 2026"
        );
        assert!(service.format_date(&fr, date, DateStyle::Long).contains("septembre"));
        let time = service.format_time(&de, Time { hour: 17, minute: 5, second: 0 });
        assert!(time.starts_with("17:05"), "{time}");
        assert_eq!(
            service.compare(&nz, "apple", "Banana"),
            Ordering::Less,
            "linguistic, not code point"
        );
        assert_eq!(service.to_upper(&Locale::new("tr-TR"), "i"), "İ", "Turkish casing");
        assert_eq!(service.to_lower(&nz, "ÀB"), "àb");
    }
}
