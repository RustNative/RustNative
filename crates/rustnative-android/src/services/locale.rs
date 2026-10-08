//! Locale-aware formatting, collation, and casing through ICU — Android's
//! own (`android.icu`, API 24+), so the answers match the system's.

use rustnative_core::Locale;
use rustnative_core::i18n::{Date, DateStyle, LocaleService, Time};

use super::java;
use crate::jni_host::{Arg, Class, Ret};

/// ICU's answers.
#[derive(Debug, Default, Clone, Copy)]
pub struct AndroidLocale;

fn text(name: &str, signature: &str, args: &[Arg<'_>]) -> String {
    java(Class::Services, name, signature, args).ok().and_then(Ret::string).unwrap_or_default()
}

impl LocaleService for AndroidLocale {
    fn format_number(&self, locale: &Locale, value: f64, decimals: u8) -> String {
        text(
            "formatNumber",
            "(Ljava/lang/String;DI)Ljava/lang/String;",
            &[Arg::Str(locale.tag()), Arg::Double(value), Arg::Int(i32::from(decimals))],
        )
    }

    fn format_currency(&self, locale: &Locale, value: f64, currency: &str) -> String {
        text(
            "formatCurrency",
            "(Ljava/lang/String;DLjava/lang/String;)Ljava/lang/String;",
            &[Arg::Str(locale.tag()), Arg::Double(value), Arg::Str(currency)],
        )
    }

    fn format_date(&self, locale: &Locale, date: Date, style: DateStyle) -> String {
        text(
            "formatDate",
            "(Ljava/lang/String;IIIZ)Ljava/lang/String;",
            &[
                Arg::Str(locale.tag()),
                Arg::Int(date.year),
                Arg::Int(i32::from(date.month)),
                Arg::Int(i32::from(date.day)),
                Arg::Bool(matches!(style, DateStyle::Long)),
            ],
        )
    }

    fn format_time(&self, locale: &Locale, time: Time) -> String {
        text(
            "formatTime",
            "(Ljava/lang/String;III)Ljava/lang/String;",
            &[
                Arg::Str(locale.tag()),
                Arg::Int(i32::from(time.hour)),
                Arg::Int(i32::from(time.minute)),
                Arg::Int(i32::from(time.second)),
            ],
        )
    }

    fn compare(&self, locale: &Locale, a: &str, b: &str) -> std::cmp::Ordering {
        let ret = java(
            Class::Services,
            "compare",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[Arg::Str(locale.tag()), Arg::Str(a), Arg::Str(b)],
        );
        match ret {
            Ok(Ret::Int(value)) => value.cmp(&0),
            _ => a.cmp(b),
        }
    }

    fn to_upper(&self, locale: &Locale, value: &str) -> String {
        text(
            "upper",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[Arg::Str(locale.tag()), Arg::Str(value)],
        )
    }

    fn to_lower(&self, locale: &Locale, value: &str) -> String {
        text(
            "lower",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[Arg::Str(locale.tag()), Arg::Str(value)],
        )
    }
}
