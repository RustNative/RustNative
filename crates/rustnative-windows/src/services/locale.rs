//! Locale-aware formatting through Windows' National Language Support
//! (`PLAN.md` Milestone 46): numbers, currencies, dates, times, collation,
//! and casing are the system's, not reimplemented here.
//!
//! Each call names the locale by its BCP 47 tag, which Windows accepts as a
//! locale name. A tag Windows does not know falls back to the user's
//! default locale, as Windows does itself. For the person's own locale,
//! the changes they made in the system's regional settings (a date order,
//! a currency symbol) are honored, as every Windows application honors
//! them.
//!
//! A currency is formatted by Windows when it is the locale's own
//! currency (`LOCALE_SINTLSYMBOL`); another currency is the locale's
//! number with the ISO code before it, since Windows formats only a
//! locale's own currency. Units have no Windows API; they are the locale's
//! number followed by the unit's international symbol.

use std::cmp::Ordering;

use rustnative_core::Locale;
use rustnative_core::i18n::{Date, DateStyle, LocaleService, Time};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::Globalization::{
    COMPARESTRING_RESULT, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringEx, DATE_LONGDATE,
    DATE_SHORTDATE, GetCurrencyFormatEx, GetDateFormatEx, GetLocaleInfoEx, GetNumberFormatEx,
    GetTimeFormatEx, LCMAP_LINGUISTIC_CASING, LCMAP_LOWERCASE, LCMAP_UPPERCASE, LCMapStringEx,
    LINGUISTIC_IGNORECASE, LOCALE_IDIGITS, LOCALE_RETURN_NUMBER, LOCALE_SINTLSYMBOL, NUMBERFMTW,
};

/// Windows' locale services.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsLocale;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Calls `call` twice, the Windows way: once for the length, once to fill.
fn fill(mut call: impl FnMut(*mut u16, i32) -> i32) -> Option<String> {
    let length = call(std::ptr::null_mut(), 0);
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0_u16; usize::try_from(length).ok()?];
    let written = call(buffer.as_mut_ptr(), length);
    if written <= 0 {
        return None;
    }
    buffer.truncate(usize::try_from(written).ok()?.saturating_sub(1));
    Some(String::from_utf16_lossy(&buffer))
}

fn locale_number(locale: &[u16], kind: u32) -> Option<u32> {
    let mut value = 0_u32;
    // SAFETY: `locale` is NUL-terminated; `LOCALE_RETURN_NUMBER` writes one
    // `u32` into the buffer, whose size is given in `u16`s (two).
    let written = unsafe {
        GetLocaleInfoEx(locale.as_ptr(), kind | LOCALE_RETURN_NUMBER, (&raw mut value).cast(), 2)
    };
    (written > 0).then_some(value)
}

fn locale_text(locale: &[u16], kind: u32) -> Option<String> {
    // SAFETY: `locale` is NUL-terminated; the buffer and its length are
    // `fill`'s.
    fill(|buffer, length| unsafe { GetLocaleInfoEx(locale.as_ptr(), kind, buffer, length) })
}

fn number_text(value: f64, decimals: u8) -> Vec<u16> {
    wide(&format!("{:.*}", usize::from(decimals), value))
}

impl LocaleService for WindowsLocale {
    fn format_number(&self, locale: &Locale, value: f64, decimals: u8) -> String {
        let name = wide(locale.tag());
        let number = number_text(value, decimals);
        // The locale's own format, with the fraction digits asked for.
        let mut decimal = locale_text(&name, windows_sys::Win32::Globalization::LOCALE_SDECIMAL)
            .map_or_else(|| wide("."), |text| wide(&text));
        let mut thousand = locale_text(&name, windows_sys::Win32::Globalization::LOCALE_STHOUSAND)
            .map_or_else(|| wide(","), |text| wide(&text));
        let format = NUMBERFMTW {
            NumDigits: u32::from(decimals),
            LeadingZero: 1,
            Grouping: 3,
            lpDecimalSep: decimal.as_mut_ptr(),
            lpThousandSep: thousand.as_mut_ptr(),
            NegativeOrder: 1,
        };
        // SAFETY: every pointer is to a NUL-terminated buffer alive for the
        // call; the output buffer and its length are `fill`'s.
        fill(|buffer, length| unsafe {
            GetNumberFormatEx(name.as_ptr(), 0, number.as_ptr(), &raw const format, buffer, length)
        })
        .unwrap_or_else(|| format!("{:.*}", usize::from(decimals), value))
    }

    fn format_currency(&self, locale: &Locale, value: f64, currency: &str) -> String {
        let name = wide(locale.tag());
        let own = locale_text(&name, LOCALE_SINTLSYMBOL);
        if own.as_deref().is_some_and(|own| own.eq_ignore_ascii_case(currency)) {
            let digits = locale_number(&name, LOCALE_IDIGITS).unwrap_or(2);
            let number = number_text(value, u8::try_from(digits).unwrap_or(2));
            // SAFETY: as in `format_number`; a null format is the locale's
            // own.
            if let Some(text) = fill(|buffer, length| unsafe {
                GetCurrencyFormatEx(
                    name.as_ptr(),
                    0,
                    number.as_ptr(),
                    std::ptr::null(),
                    buffer,
                    length,
                )
            }) {
                return text;
            }
        }
        format!("{currency} {}", self.format_number(locale, value, 2))
    }

    fn format_date(&self, locale: &Locale, date: Date, style: DateStyle) -> String {
        let name = wide(locale.tag());
        let time = SYSTEMTIME {
            wYear: u16::try_from(date.year).unwrap_or(1601),
            wMonth: u16::from(date.month),
            wDayOfWeek: 0,
            wDay: u16::from(date.day),
            wHour: 0,
            wMinute: 0,
            wSecond: 0,
            wMilliseconds: 0,
        };
        let flags = match style {
            DateStyle::Short => DATE_SHORTDATE,
            DateStyle::Long => DATE_LONGDATE,
        };
        // SAFETY: `name` is NUL-terminated; `time` is a valid date; a null
        // format is the locale's own; `fill` owns the buffer.
        fill(|buffer, length| unsafe {
            GetDateFormatEx(
                name.as_ptr(),
                flags,
                &raw const time,
                std::ptr::null(),
                buffer,
                length,
                std::ptr::null(),
            )
        })
        .unwrap_or_else(|| format!("{:04}-{:02}-{:02}", date.year, date.month, date.day))
    }

    fn format_time(&self, locale: &Locale, time: Time) -> String {
        let name = wide(locale.tag());
        let system = SYSTEMTIME {
            wYear: 2000,
            wMonth: 1,
            wDayOfWeek: 0,
            wDay: 1,
            wHour: u16::from(time.hour),
            wMinute: u16::from(time.minute),
            wSecond: u16::from(time.second),
            wMilliseconds: 0,
        };
        // SAFETY: as in `format_date`.
        fill(|buffer, length| unsafe {
            GetTimeFormatEx(name.as_ptr(), 0, &raw const system, std::ptr::null(), buffer, length)
        })
        .unwrap_or_else(|| format!("{:02}:{:02}:{:02}", time.hour, time.minute, time.second))
    }

    fn compare(&self, locale: &Locale, a: &str, b: &str) -> Ordering {
        let name = wide(locale.tag());
        let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
        let length = |text: &[u16]| i32::try_from(text.len()).unwrap_or(i32::MAX);
        // SAFETY: both strings are passed with their lengths; the reserved
        // arguments are null/zero as documented.
        let result: COMPARESTRING_RESULT = unsafe {
            CompareStringEx(
                name.as_ptr(),
                LINGUISTIC_IGNORECASE,
                a.as_ptr(),
                length(&a),
                b.as_ptr(),
                length(&b),
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        match result {
            CSTR_LESS_THAN => Ordering::Less,
            CSTR_GREATER_THAN => Ordering::Greater,
            // Equal ignoring case: the case decides, so the order is total.
            _ => a.cmp(&b),
        }
    }

    fn to_upper(&self, locale: &Locale, text: &str) -> String {
        map(locale, text, LCMAP_UPPERCASE).unwrap_or_else(|| text.to_uppercase())
    }

    fn to_lower(&self, locale: &Locale, text: &str) -> String {
        map(locale, text, LCMAP_LOWERCASE).unwrap_or_else(|| text.to_lowercase())
    }
}

fn map(locale: &Locale, text: &str, flags: u32) -> Option<String> {
    let name = wide(locale.tag());
    let source: Vec<u16> = text.encode_utf16().collect();
    let length = i32::try_from(source.len()).ok()?;
    if length == 0 {
        return Some(String::new());
    }
    let call = |buffer: *mut u16, capacity: i32| {
        // SAFETY: `source` is passed with its length; the output buffer and
        // capacity are `fill`'s (a zero capacity asks for the length).
        unsafe {
            LCMapStringEx(
                name.as_ptr(),
                flags | LCMAP_LINGUISTIC_CASING,
                source.as_ptr(),
                length,
                buffer,
                capacity,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        }
    };
    // `LCMapStringEx` with an explicit length writes no terminator, so the
    // result is exactly the returned length.
    let needed = call(std::ptr::null_mut(), 0);
    let mut buffer = vec![0_u16; usize::try_from(needed).ok()?];
    let written = call(buffer.as_mut_ptr(), needed);
    (written > 0)
        .then(|| String::from_utf16_lossy(&buffer[..usize::try_from(written).unwrap_or(0)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_formats_as_each_locale_writes() {
        let service = WindowsLocale;
        // Locales that are not this machine's own, whose formats carry no
        // personal overrides.
        let (nz, de, fr) = (Locale::new("en-NZ"), Locale::new("de-DE"), Locale::new("fr-FR"));
        assert_eq!(service.format_number(&nz, 1_234_567.891, 2), "1,234,567.89");
        assert_eq!(service.format_number(&de, 1_234_567.891, 2), "1.234.567,89");
        let french = service.format_number(&fr, 1_234.5, 1);
        assert!(french.ends_with(",5") && french.starts_with('1'), "{french}");
        assert_eq!(service.format_currency(&de, 9.5, "EUR"), "9,50 €");
        assert_eq!(service.format_currency(&de, 9.5, "USD"), "USD 9,50", "not the locale's own");
        let date = Date { year: 2026, month: 9, day: 24 };
        assert_eq!(service.format_date(&de, date, DateStyle::Short), "24.09.2026");
        assert!(service.format_date(&de, date, DateStyle::Long).contains("September"));
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
