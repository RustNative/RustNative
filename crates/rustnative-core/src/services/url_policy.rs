//! Which URLs [`SystemService::open_url`](super::SystemService::open_url)
//! hands to the operating system.
//!
//! Every backend's `open_url` ends in a call that launches whatever the OS
//! has registered for the string it is given: `ShellExecuteW` runs a local
//! executable, a UNC path (`\\host\share\x.exe`), or any protocol handler
//! (`ms-msdt:`, `search-ms:`); GIO and the OpenURI portal, and Android's
//! `ACTION_VIEW`, open any scheme an installed application claims. When a
//! link's target comes from data the application does not control, that is
//! a remote-code-execution path, so every backend checks the URL against
//! one [`UrlPolicy`] first.
//!
//! The default policy allows `http`, `https`, and `mailto`. An application
//! that opens its own registered schemes (`notes://…`) widens it explicitly
//! with [`UrlPolicy::allow_scheme`] and gives the result to its backend's
//! system service.

use super::ServiceError;

/// The schemes every policy allows.
const STANDARD: [&str; 3] = ["http", "https", "mailto"];

/// The URLs a system service will open.
///
/// ```
/// use rustnative_core::services::UrlPolicy;
///
/// let policy = UrlPolicy::default();
/// assert!(policy.check("https://example.com/docs").is_ok());
/// assert!(policy.check("mailto:someone@example.com").is_ok());
/// assert!(policy.check(r"\\host\share\x.exe").is_err());
/// assert!(policy.check("notes://items/7").is_err());
///
/// let policy = UrlPolicy::default().allow_scheme("notes").unwrap();
/// assert!(policy.check("notes://items/7").is_ok());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UrlPolicy {
    /// Schemes the application added, lowercase.
    extra: Vec<String>,
}

impl UrlPolicy {
    /// The default policy: `http`, `https`, and `mailto`.
    #[must_use]
    pub fn standard() -> Self {
        Self::default()
    }

    /// This policy, also allowing `scheme` (matched case-insensitively).
    ///
    /// # Errors
    ///
    /// `scheme` is not a URL scheme (a letter, then letters, digits, `+`,
    /// `-`, or `.`), or is a single letter, which Windows reads as a drive
    /// (`C:\…`).
    pub fn allow_scheme(mut self, scheme: &str) -> Result<Self, ServiceError> {
        if !is_scheme(scheme) {
            return Err(ServiceError::new(format!("`{scheme}` is not a URL scheme")));
        }
        if scheme.len() < 2 {
            return Err(ServiceError::new(format!(
                "`{scheme}` is one letter, which Windows opens as a drive"
            )));
        }
        let scheme = scheme.to_ascii_lowercase();
        if !self.allows(&scheme) {
            self.extra.push(scheme);
        }
        Ok(self)
    }

    /// The schemes this policy allows, lowercase.
    pub fn schemes(&self) -> impl Iterator<Item = &str> {
        STANDARD.into_iter().chain(self.extra.iter().map(String::as_str))
    }

    /// Returns `url` if this policy lets it reach the operating system: an
    /// allowed scheme, something after it (an `http`/`https` URL names a
    /// host), and no whitespace, control or invisible formatting
    /// characters, quotes, backslashes, or shell metacharacters — so the
    /// handler receives one well-formed URL and nothing it could read as
    /// more.
    ///
    /// # Errors
    ///
    /// The URL has no scheme, a scheme this policy does not allow, nothing
    /// after the scheme, or such a character.
    pub fn check<'a>(&self, url: &'a str) -> Result<&'a str, ServiceError> {
        let Some((scheme, rest)) = url.split_once(':').filter(|(scheme, _)| is_scheme(scheme))
        else {
            return Err(ServiceError::new(format!("`{url}` does not start with a URL scheme")));
        };
        let scheme = scheme.to_ascii_lowercase();
        if !self.allows(&scheme) {
            let allowed: Vec<&str> = self.schemes().collect();
            return Err(ServiceError::new(format!(
                "`{scheme}:` URLs are not opened (allowed: {})",
                allowed.join(", ")
            )));
        }
        let empty = match scheme.as_str() {
            "http" | "https" => rest
                .strip_prefix("//")
                .is_none_or(|after| after.split(['/', '?', '#']).next().is_none_or(str::is_empty)),
            _ => rest.is_empty(),
        };
        if empty {
            return Err(ServiceError::new(format!("`{url}` names nothing to open")));
        }
        if let Some(bad) = url.chars().find(|&c| refused(c)) {
            return Err(ServiceError::new(format!(
                "the URL contains {bad:?}, which a URL carries only percent-encoded"
            )));
        }
        Ok(url)
    }

    fn allows(&self, scheme: &str) -> bool {
        self.schemes().any(|allowed| allowed == scheme)
    }
}

/// RFC 3986: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn refused(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '^' | '\\')
        // Invisible characters that make a URL read as something else:
        // zero-width spaces and joiners, directional marks, embeddings,
        // overrides and isolates, and the byte-order mark.
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_links_are_opened() {
        let policy = UrlPolicy::default();
        for url in [
            "https://example.com/search?q=a&lang=en#top",
            "HTTP://EXAMPLE.COM",
            "https://example.com",
            "https://[::1]:8080/x",
            "mailto:someone@example.com",
            "mailto:€",
            "https://例え.jp/パス",
        ] {
            assert_eq!(policy.check(url).map_err(|e| e.to_string()), Ok(url), "{url:?}");
        }
    }

    #[test]
    fn what_the_os_would_run_is_refused_by_default() {
        let policy = UrlPolicy::default();
        for url in [
            r"C:\Windows\System32\calc.exe",
            "C:/Windows/System32/calc.exe",
            r"\\host\share\x",
            "//host/share/x",
            "file:///etc/passwd",
            "ms-msdt:/id PCWDiagnostic /skip force",
            "ms-msdt:-id",
            "search-ms:query=x",
            "javascript:alert(1)",
            "vbscript:msgbox",
            "data:text/html,<script>",
            "calc.exe",
            "--help",
            "",
            ":",
            "https:",
            "https://",
            "https:///etc/passwd",
            "https://?q",
            "mailto:",
            "https://example.com/\" & calc.exe",
            "https://example.com/a b",
            "https://example.com/|whoami",
            "https://example.com/^&x",
            "https://example.com/\nnext",
            "https://example.com/\u{202E}exe.pdf",
            "https://example.com\\@evil.test",
        ] {
            assert!(policy.check(url).is_err(), "{url:?} was opened");
        }
    }

    #[test]
    fn non_ascii_near_the_scheme_is_refused_not_a_panic() {
        // Each of these panicked when the scheme was compared by byte-index
        // slicing (`url[..scheme.len()]`).
        let policy = UrlPolicy::default();
        assert!(policy.check("mailto:€").is_ok());
        for url in ["mailt€x", "http😀", "https€//x", "€", "h€:x", "😀:x", "ht\u{301}tp://x"]
        {
            assert!(policy.check(url).is_err(), "{url:?} was opened");
        }
    }

    #[test]
    fn an_application_widens_the_policy_with_its_own_schemes() {
        let policy = UrlPolicy::default().allow_scheme("Notes").unwrap();
        assert!(policy.check("notes://items/7").is_ok());
        assert!(policy.check("NOTES:items/7").is_ok());
        assert!(policy.check("notes:").is_err());
        assert!(policy.check("javascript:alert(1)").is_err());
        assert_eq!(policy.schemes().collect::<Vec<_>>(), ["http", "https", "mailto", "notes"]);
        assert_eq!(policy.clone().allow_scheme("https").unwrap(), policy);
    }

    #[test]
    fn a_scheme_that_is_not_one_is_never_allowed() {
        for scheme in ["", "c", "C", "1abc", "no tes", "notes:", "ex€", "a/b", "-x"] {
            assert!(UrlPolicy::default().allow_scheme(scheme).is_err(), "{scheme:?} was allowed");
        }
    }
}
