//! Typed per-route metadata (`C41-1`): the head — title, description,
//! canonical address, social card, structured data, and the page's language
//! alternates (`C41-2`) — rendered on the server, validated before it
//! ships, and updated in place when the client navigates; and a sitemap of
//! the routes.
//!
//! ```
//! use rustnative_web::head::{Head, Sitemap};
//!
//! let head = Head::new("Notes — Rust Native", "Write things down, find them again, on every device you use.")
//!     .canonical("https://notes.example.com/")
//!     .image("https://notes.example.com/card.png")
//!     .alternate("fr", "https://notes.example.com/fr/");
//! assert!(head.validate().is_empty());
//! let html = head.render("n0nce");
//! assert!(html.contains("<meta property=\"og:title\" content=\"Notes — Rust Native\">"));
//! assert!(html.contains("<link rel=\"alternate\" hreflang=\"fr\" href=\"https://notes.example.com/fr/\">"));
//!
//! let sitemap = Sitemap::new("https://notes.example.com").page("/", None).page("/about", Some("2026-09-25"));
//! assert!(sitemap.xml().contains("<loc>https://notes.example.com/about</loc>"));
//! ```

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::html::escape_into;

/// A page's head.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    title: String,
    description: String,
    canonical: Option<String>,
    image: Option<String>,
    kind: &'static str,
    structured_data: Option<Value>,
    alternates: Vec<(String, String)>,
}

impl Head {
    /// A head with its title and description.
    #[must_use]
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            canonical: None,
            image: None,
            kind: "website",
            structured_data: None,
            alternates: Vec::new(),
        }
    }

    /// The page's title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The page's canonical address.
    #[must_use]
    pub fn canonical(mut self, url: impl Into<String>) -> Self {
        self.canonical = Some(url.into());
        self
    }

    /// The social card image.
    #[must_use]
    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.image = Some(url.into());
        self
    }

    /// The Open Graph type (`article`, …; default `website`).
    #[must_use]
    pub const fn kind(mut self, kind: &'static str) -> Self {
        self.kind = kind;
        self
    }

    /// Schema.org structured data (JSON-LD).
    #[must_use]
    pub fn structured_data(mut self, data: Value) -> Self {
        self.structured_data = Some(data);
        self
    }

    /// The same page in language `lang` (a BCP 47 tag, or `x-default`) at
    /// `url`: search engines show each reader the page in their language.
    #[must_use]
    pub fn alternate(mut self, lang: impl Into<String>, url: impl Into<String>) -> Self {
        self.alternates.push((lang.into(), url.into()));
        self
    }

    /// What is wrong with it, for a build-time check: an empty or overlong
    /// title, a description outside what search results show, relative
    /// addresses where absolute ones are required.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let length = self.title.chars().count();
        if length == 0 || length > 60 {
            problems.push(format!("the title is {length} characters; 1 to 60 show in full"));
        }
        let length = self.description.chars().count();
        if !(50..=160).contains(&length) {
            problems
                .push(format!("the description is {length} characters; 50 to 160 show in full"));
        }
        let alternates =
            self.alternates.iter().map(|(lang, url)| (format!("{lang} alternate"), url));
        let fixed = [
            ("canonical address".to_owned(), &self.canonical),
            ("card image".to_owned(), &self.image),
        ];
        for (what, url) in fixed
            .into_iter()
            .filter_map(|(what, url)| Some((what, url.as_ref()?)))
            .chain(alternates)
        {
            if !url.starts_with("https://") {
                problems.push(format!("the {what} {url:?} is not an absolute https address"));
            }
        }
        problems
    }

    /// The head's markup; structured data carries the response's CSP
    /// `nonce` so the policy lets it through.
    #[must_use]
    pub fn render(&self, nonce: &str) -> String {
        let mut head = String::from("<title>");
        escape_into(&mut head, &self.title);
        head.push_str("</title>");
        let mut tag = |open: &str, attribute: &str, value: &str| {
            head.push_str(open);
            escape_into(&mut head, attribute);
            head.push_str("\" content=\"");
            escape_into(&mut head, value);
            head.push_str("\">");
        };
        tag("<meta name=\"", "description", &self.description);
        tag("<meta property=\"", "og:title", &self.title);
        tag("<meta property=\"", "og:description", &self.description);
        tag("<meta property=\"", "og:type", self.kind);
        if let Some(image) = &self.image {
            tag("<meta property=\"", "og:image", image);
            tag("<meta name=\"", "twitter:card", "summary_large_image");
        }
        if let Some(canonical) = &self.canonical {
            head.push_str("<link rel=\"canonical\" href=\"");
            escape_into(&mut head, canonical);
            head.push_str("\">");
        }
        for (lang, url) in &self.alternates {
            head.push_str("<link rel=\"alternate\" hreflang=\"");
            escape_into(&mut head, lang);
            head.push_str("\" href=\"");
            escape_into(&mut head, url);
            head.push_str("\">");
        }
        if let Some(data) = &self.structured_data {
            head.push_str("<script type=\"application/ld+json\" nonce=\"");
            escape_into(&mut head, nonce);
            head.push_str("\">");
            head.push_str(&crate::html::json_for_script(data));
            head.push_str("</script>");
        }
        head
    }

    /// The head as the runtime applies it after a client-side navigation
    /// (Web milestone G): the title, and each `meta` and `link` by the name
    /// it is keyed on.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "title": self.title,
            "description": self.description,
            "canonical": self.canonical,
            "image": self.image,
            "kind": self.kind,
            "alternates": self.alternates,
        })
    }
}

/// A sitemap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sitemap {
    base: String,
    pages: Vec<(String, Option<String>)>,
}

impl Sitemap {
    /// A sitemap for the site at `base`.
    #[must_use]
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_owned(), pages: Vec::new() }
    }

    /// Adds a page, with its last change (`YYYY-MM-DD`).
    #[must_use]
    pub fn page(mut self, path: &str, last_modified: Option<&str>) -> Self {
        self.pages.push((path.to_owned(), last_modified.map(str::to_owned)));
        self
    }

    /// The sitemap XML.
    #[must_use]
    pub fn xml(&self) -> String {
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
        );
        for (path, modified) in &self.pages {
            let mut location = String::new();
            escape_into(&mut location, &format!("{}{path}", self.base));
            let _ = write!(xml, "  <url><loc>{location}</loc>");
            if let Some(modified) = modified {
                let mut date = String::new();
                escape_into(&mut date, modified);
                let _ = write!(xml, "<lastmod>{date}</lastmod>");
            }
            xml.push_str("</url>\n");
        }
        xml.push_str("</urlset>\n");
        xml
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_alternate_is_a_problem() {
        let head =
            Head::new("Notes", "Write things down, find them again, on every device you use.")
                .alternate("de", "/de/");
        assert_eq!(
            head.validate(),
            vec!["the de alternate \"/de/\" is not an absolute https address"]
        );
    }

    #[test]
    fn structured_data_cannot_end_its_script() {
        let head =
            Head::new("t", "d").structured_data(json!({ "name": "</script><script>alert(1)" }));
        let html = head.render("n");
        assert_eq!(html.matches("</script>").count(), 1);
    }
}
