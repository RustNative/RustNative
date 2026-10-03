//! The person's locale, as the POSIX environment states it.
//!
//! glibc resolves a category's locale from `LC_ALL`, then the category's
//! own variable, then `LANG`; the interface language is `LC_MESSAGES`'s. A
//! POSIX locale name (`pt_BR.UTF-8@euro`) becomes a BCP 47 tag (`pt-BR`)
//! for the portable model; `C` and `POSIX` are the absence of a preference.

use rustnative_core::Locale;

/// The locale the person's interface language is in, from `variable` (an
/// environment lookup).
#[must_use]
pub fn from_environment(variable: impl Fn(&str) -> Option<String>) -> Option<Locale> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| variable(name).filter(|value| !value.is_empty()))
        .find_map(|value| bcp47(&value))
        .map(Locale::new)
}

/// This process's interface locale, or the portable default.
#[must_use]
pub fn current() -> Locale {
    from_environment(|name| std::env::var(name).ok()).unwrap_or_default()
}

/// A POSIX locale name as a BCP 47 tag, or `None` for `C`/`POSIX`.
#[must_use]
pub fn bcp47(posix: &str) -> Option<String> {
    // `language[_territory][.codeset][@modifier]`
    let without_modifier = posix.split('@').next().unwrap_or_default();
    let name = without_modifier.split('.').next().unwrap_or_default();
    if name.is_empty() || name == "C" || name == "POSIX" {
        return None;
    }
    let mut parts = name.split('_');
    let language = parts.next()?.to_ascii_lowercase();
    if !language.chars().all(|c| c.is_ascii_alphabetic()) || !(2..=3).contains(&language.len()) {
        return None;
    }
    let script = match posix.split('@').nth(1) {
        Some("latin") => Some("Latn"),
        Some("cyrillic") => Some("Cyrl"),
        Some("devanagari") => Some("Deva"),
        _ => None,
    };
    let mut tag = language;
    if let Some(script) = script {
        tag.push('-');
        tag.push_str(script);
    }
    if let Some(territory) = parts.next().filter(|territory| !territory.is_empty()) {
        tag.push('-');
        tag.push_str(&territory.to_ascii_uppercase());
    }
    Some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_names_become_bcp47_tags() {
        assert_eq!(bcp47("en_US.UTF-8").as_deref(), Some("en-US"));
        assert_eq!(bcp47("pt_BR").as_deref(), Some("pt-BR"));
        assert_eq!(bcp47("ar_EG.UTF-8").as_deref(), Some("ar-EG"));
        assert_eq!(bcp47("sr_RS@latin").as_deref(), Some("sr-Latn-RS"));
        assert_eq!(bcp47("de").as_deref(), Some("de"));
        assert_eq!(bcp47("C.UTF-8"), None);
        assert_eq!(bcp47("POSIX"), None);
    }

    #[test]
    fn lc_all_wins_then_messages_then_lang() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
            }
        };
        assert_eq!(
            from_environment(env(&[("LANG", "en_US.UTF-8"), ("LC_MESSAGES", "fr_FR.UTF-8")])),
            Some(Locale::new("fr-FR"))
        );
        assert_eq!(
            from_environment(env(&[("LANG", "en_US.UTF-8"), ("LC_ALL", "he_IL.UTF-8")])),
            Some(Locale::new("he-IL"))
        );
        assert_eq!(from_environment(env(&[("LANG", "C.UTF-8")])), None);
    }
}
