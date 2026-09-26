//! Route patterns: turning a path like `/users/42/files/a%20b` into a named
//! route and its parameters, and back.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::str::FromStr;

/// One segment of a [`Route`] pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Segment {
    /// Must match exactly.
    Literal(String),
    /// Matches any one non-empty segment, captured under this name.
    Param(String),
    /// Matches the rest of the path (possibly empty), captured under this
    /// name. Only allowed last.
    Rest(String),
}

/// Why a route pattern could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// A `*rest` segment appeared before the end of the pattern.
    RestNotLast(String),
    /// A `:param` or `*rest` segment has no name.
    UnnamedParameter,
    /// Two parameters share a name.
    DuplicateParameter(String),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RestNotLast(name) => write!(f, "`*{name}` must be the last segment"),
            Self::UnnamedParameter => f.write_str("a route parameter has no name"),
            Self::DuplicateParameter(name) => write!(f, "route parameter `{name}` appears twice"),
        }
    }
}

impl std::error::Error for RouteError {}

/// A route pattern: literal segments, `:name` parameters, and an optional
/// trailing `*name` that captures the rest of the path.
///
/// # Example
///
/// ```
/// use rustnative_core::Route;
///
/// let route = Route::parse("/users/:id/files/*path")?;
/// let matched = route.matches("/users/42/files/docs/a%20b.txt").expect("it matches");
/// assert_eq!(matched.param::<u32>("id"), Some(42));
/// assert_eq!(matched.get("path"), Some("docs/a b.txt"), "percent-decoded");
///
/// assert!(route.matches("/users/42").is_none(), "`files` is required");
/// # Ok::<(), rustnative_core::RouteError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Route {
    segments: Vec<Segment>,
}

impl Route {
    /// Parses a pattern. Leading, trailing, and repeated `/` are ignored.
    ///
    /// # Errors
    ///
    /// [`RouteError`] for a `*rest` that is not last, an unnamed parameter,
    /// or a parameter name used twice.
    pub fn parse(pattern: &str) -> Result<Self, RouteError> {
        let parts = split(pattern).collect::<Vec<_>>();
        let mut segments = Vec::with_capacity(parts.len());
        let mut names = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let segment = if let Some(name) = part.strip_prefix(':') {
                Segment::Param(name.to_owned())
            } else if let Some(name) = part.strip_prefix('*') {
                if index + 1 != parts.len() {
                    return Err(RouteError::RestNotLast(name.to_owned()));
                }
                Segment::Rest(name.to_owned())
            } else {
                Segment::Literal(decode(part))
            };
            if let Segment::Param(name) | Segment::Rest(name) = &segment {
                if name.is_empty() {
                    return Err(RouteError::UnnamedParameter);
                }
                if names.contains(name) {
                    return Err(RouteError::DuplicateParameter(name.clone()));
                }
                names.push(name.clone());
            }
            segments.push(segment);
        }
        Ok(Self { segments })
    }

    /// The parameters' names, in the order the pattern names them.
    #[must_use]
    pub fn parameter_names(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Param(name) | Segment::Rest(name) => Some(name.as_str()),
                Segment::Literal(_) => None,
            })
            .collect()
    }

    /// Matches `path` (a URL path, optionally followed by `?query` or
    /// `#fragment`, which are ignored), returning the captured parameters.
    #[must_use]
    pub fn matches(&self, path: &str) -> Option<RouteParams> {
        let path = path.split(['?', '#']).next().unwrap_or_default();
        let parts = split(path).collect::<Vec<_>>();
        let mut params = BTreeMap::new();
        for (index, segment) in self.segments.iter().enumerate() {
            match segment {
                Segment::Literal(literal) => {
                    if parts.get(index).map(|part| decode(part)) != Some(literal.clone()) {
                        return None;
                    }
                }
                Segment::Param(name) => {
                    params.insert(name.clone(), decode(parts.get(index)?));
                }
                Segment::Rest(name) => {
                    let rest = parts.get(index..).unwrap_or_default();
                    let rest = rest.iter().map(|part| decode(part)).collect::<Vec<_>>().join("/");
                    params.insert(name.clone(), rest);
                    return Some(RouteParams { params });
                }
            }
        }
        (parts.len() == self.segments.len()).then_some(RouteParams { params })
    }

    /// Builds the path this route matches with `params` filled in,
    /// percent-encoding each value; `None` if a parameter is missing (or a
    /// single-segment parameter is empty, which could not match).
    ///
    /// ```
    /// use rustnative_core::Route;
    ///
    /// let route = Route::parse("/search/:query")?;
    /// let path = route.build(&[("query", "rust & ui")]).expect("all parameters given");
    /// assert_eq!(path, "/search/rust%20%26%20ui");
    /// assert_eq!(route.matches(&path).unwrap().get("query"), Some("rust & ui"));
    /// # Ok::<(), rustnative_core::RouteError>(())
    /// ```
    #[must_use]
    pub fn build(&self, params: &[(&str, &str)]) -> Option<String> {
        let lookup =
            |name: &str| params.iter().find(|(key, _)| *key == name).map(|(_, value)| *value);
        let mut path = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(literal) => {
                    path.push('/');
                    path.push_str(&encode(literal));
                }
                Segment::Param(name) => {
                    let value = lookup(name).filter(|value| !value.is_empty())?;
                    path.push('/');
                    path.push_str(&encode(value));
                }
                Segment::Rest(name) => {
                    let value = lookup(name)?;
                    for part in value.split('/').filter(|part| !part.is_empty()) {
                        path.push('/');
                        path.push_str(&encode(part));
                    }
                }
            }
        }
        if path.is_empty() {
            path.push('/');
        }
        Some(path)
    }
}

/// The parameters a [`Route`] captured.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteParams {
    params: BTreeMap<String, String>,
}

impl RouteParams {
    /// The decoded value of parameter `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    /// Parameter `name` parsed as `T`; `None` if missing or unparsable.
    #[must_use]
    pub fn param<T: FromStr>(&self, name: &str) -> Option<T> {
        self.get(name)?.parse().ok()
    }

    /// Every captured parameter.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.params.iter().map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

/// A named set of routes, matched in the order they were added.
///
/// # Example
///
/// ```
/// use rustnative_core::Router;
///
/// let router = Router::new()
///     .route("home", "/")?
///     .route("user", "/users/:id")?
///     .route("not-found", "/*path")?;
///
/// let (name, params) = router.resolve("/users/7").expect("a route matches");
/// assert_eq!((name, params.param::<u32>("id")), ("user", Some(7)));
/// assert_eq!(router.resolve("/nope").map(|(name, _)| name), Some("not-found"));
/// # Ok::<(), rustnative_core::RouteError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct Router {
    routes: Vec<(String, Route)>,
    /// Per-locale patterns of localized routes (`C41-2`): name → (locale,
    /// pattern).
    localized: Vec<(String, Vec<(String, Route)>)>,
}

impl Router {
    /// An empty router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a route named `name`.
    ///
    /// # Errors
    ///
    /// The pattern's [`RouteError`].
    pub fn route(mut self, name: impl Into<String>, pattern: &str) -> Result<Self, RouteError> {
        self.routes.push((name.into(), Route::parse(pattern)?));
        Ok(self)
    }

    /// Adds a route named `name` whose path differs by locale (`C41-2`):
    /// `/settings` in English, `/einstellungen` in German. Each pattern is
    /// also an ordinary route of that name, so any of them resolves.
    ///
    /// ```
    /// use rustnative_core::navigation::Router;
    ///
    /// let router = Router::new()
    ///     .localized("settings", &[("en", "/settings"), ("de", "/einstellungen"), ("ar", "/الإعدادات")])?;
    /// assert_eq!(router.resolve("/einstellungen").map(|(name, _)| name), Some("settings"));
    /// assert_eq!(router.locale_of("/einstellungen"), Some("de"));
    /// let alternates = router.alternates("settings", &[]);
    /// assert_eq!(alternates[0], ("en".to_owned(), "/settings".to_owned()));
    /// assert_eq!(alternates.len(), 3);
    /// # Ok::<(), rustnative_core::RouteError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// A pattern's [`RouteError`].
    pub fn localized(
        mut self,
        name: impl Into<String>,
        patterns: &[(&str, &str)],
    ) -> Result<Self, RouteError> {
        let name = name.into();
        let mut parsed = Vec::new();
        for (locale, pattern) in patterns {
            let route = Route::parse(pattern)?;
            self.routes.push((name.clone(), route.clone()));
            parsed.push(((*locale).to_owned(), route));
        }
        self.localized.push((name, parsed));
        Ok(self)
    }

    /// The locale whose localized pattern `path` matches, if any.
    #[must_use]
    pub fn locale_of(&self, path: &str) -> Option<&str> {
        self.localized
            .iter()
            .flat_map(|(_, patterns)| patterns)
            .find_map(|(locale, route)| route.matches(path).map(|_| locale.as_str()))
    }

    /// Route `name`'s path in every locale it is localized for, with
    /// `params` filled in — the language alternates a page or a share sheet
    /// offers.
    #[must_use]
    pub fn alternates(&self, name: &str, params: &[(&str, &str)]) -> Vec<(String, String)> {
        self.localized
            .iter()
            .filter(|(route, _)| route == name)
            .flat_map(|(_, patterns)| patterns)
            .filter_map(|(locale, route)| Some((locale.clone(), route.build(params)?)))
            .collect()
    }

    /// The first route matching `path`, and what it captured.
    #[must_use]
    pub fn resolve(&self, path: &str) -> Option<(&str, RouteParams)> {
        self.routes
            .iter()
            .find_map(|(name, route)| route.matches(path).map(|params| (name.as_str(), params)))
    }

    /// The route named `name`, to build a path with.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Route> {
        self.routes.iter().find(|(route, _)| route == name).map(|(_, route)| route)
    }
}

/// The path of a URL: `app://host/users/7?x=1` gives `/users/7?x=1`, and a
/// bare path is returned unchanged. What a deep link is routed by.
#[must_use]
pub fn url_path(url: &str) -> &str {
    let Some((_, rest)) = url.split_once("://") else {
        return url;
    };
    rest.find('/').map_or("/", |start| &rest[start..])
}

fn split(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|part| !part.is_empty())
}

/// Percent-decodes `part`; malformed escapes are kept literally, and bytes
/// that do not form UTF-8 are replaced rather than rejected.
fn decode(part: &str) -> String {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3).and_then(|pair| {
                std::str::from_utf8(pair).ok().and_then(|pair| u8::from_str_radix(pair, 16).ok())
            });
            if let Some(byte) = hex {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes everything but RFC 3986's unreserved characters.
fn encode(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_matches_only_paths_of_its_shape() {
        let route = Route::parse("/users/:id").unwrap();
        let table = [
            ("/users/1", Some("1")),
            ("users/1/", Some("1")),
            ("//users//1", Some("1")),
            ("/users/1?tab=files#top", Some("1")),
            ("/users", None),
            ("/users/1/extra", None),
            ("/people/1", None),
        ];
        for (path, expected) in table {
            let matched = route.matches(path);
            assert_eq!(matched.as_ref().and_then(|params| params.get("id")), expected, "{path}");
        }
    }

    #[test]
    fn a_rest_segment_captures_everything_after_it_including_nothing() {
        let route = Route::parse("/files/*path").unwrap();
        assert_eq!(route.matches("/files/a/b/c").unwrap().get("path"), Some("a/b/c"));
        assert_eq!(route.matches("/files").unwrap().get("path"), Some(""));
    }

    #[test]
    fn literals_and_parameters_are_percent_decoded() {
        let route = Route::parse("/tags/c%2B%2B/:name").unwrap();
        let matched = route.matches("/tags/c++/%E2%9C%93%20done").unwrap();
        assert_eq!(matched.get("name"), Some("\u{2713} done"));
    }

    #[test]
    fn malformed_escapes_are_kept_rather_than_rejected() {
        let route = Route::parse("/:x").unwrap();
        assert_eq!(route.matches("/100%").unwrap().get("x"), Some("100%"));
        assert_eq!(route.matches("/%zz").unwrap().get("x"), Some("%zz"));
    }

    #[test]
    fn bad_patterns_are_refused() {
        assert_eq!(Route::parse("/*a/b"), Err(RouteError::RestNotLast("a".to_owned())));
        assert_eq!(Route::parse("/:"), Err(RouteError::UnnamedParameter));
        assert_eq!(Route::parse("/:a/:a"), Err(RouteError::DuplicateParameter("a".to_owned())));
    }

    #[test]
    fn the_root_route_matches_only_the_root() {
        let route = Route::parse("/").unwrap();
        assert!(route.matches("/").is_some());
        assert!(route.matches("").is_some());
        assert!(route.matches("/a").is_none());
        assert_eq!(route.build(&[]).as_deref(), Some("/"));
    }

    #[test]
    fn a_missing_parameter_builds_nothing() {
        let route = Route::parse("/users/:id").unwrap();
        assert_eq!(route.build(&[]), None);
        assert_eq!(route.build(&[("id", "")]), None, "an empty segment could not match");
    }

    #[test]
    fn a_url_is_routed_by_its_path() {
        assert_eq!(url_path("myapp://open/users/7?x=1"), "/users/7?x=1");
        assert_eq!(url_path("myapp://open"), "/");
        assert_eq!(url_path("/already/a/path"), "/already/a/path");
    }
}
