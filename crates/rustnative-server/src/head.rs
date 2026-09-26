//! Typed per-route web metadata (`C41-1`): the head — title, description,
//! canonical address, social card, structured data — rendered on the
//! server, validated before it ships, and a sitemap of the routes.
//!
//! ```
//! use rustnative_server::head::{Head, Sitemap};
//!
//! let head = Head::new("Notes — Rust Native", "Write things down, find them again, on every device you use.")
//!     .canonical("https://notes.example.com/")
//!     .image("https://notes.example.com/card.png");
//! assert!(head.validate().is_empty());
//! let html = head.render("n0nce");
//! assert!(html.as_str().contains("<meta property=\"og:title\" content=\"Notes — Rust Native\">"));
//!
//! let sitemap = Sitemap::new("https://notes.example.com").page("/", None).page("/about", Some("2026-09-25"));
//! assert!(sitemap.xml().contains("<loc>https://notes.example.com/about</loc>"));
//! ```

use std::fmt::Write as _;

use serde_json::Value;

use crate::response::{Html, escape_into};

/// A page's head.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    title: String,
    description: String,
    canonical: Option<String>,
    image: Option<String>,
    kind: &'static str,
    structured_data: Option<Value>,
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
        }
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
        for (what, url) in [("canonical address", &self.canonical), ("card image", &self.image)] {
            if let Some(url) = url {
                if !url.starts_with("https://") {
                    problems.push(format!("the {what} {url:?} is not an absolute https address"));
                }
            }
        }
        problems
    }

    /// The head's markup; structured data carries the response's CSP
    /// `nonce` so the policy lets it through.
    #[must_use]
    pub fn render(&self, nonce: &str) -> Html {
        let mut out = String::new();
        let mut tag = |open: &str, attribute: &str, value: &str| {
            out.push_str(open);
            escape_into(&mut out, attribute);
            out.push_str("\" content=\"");
            escape_into(&mut out, value);
            out.push_str("\">");
        };
        tag("<meta name=\"", "description", &self.description);
        tag("<meta property=\"", "og:title", &self.title);
        tag("<meta property=\"", "og:description", &self.description);
        tag("<meta property=\"", "og:type", self.kind);
        if let Some(image) = &self.image {
            tag("<meta property=\"", "og:image", image);
            tag("<meta name=\"", "twitter:card", "summary_large_image");
        }
        let mut head = String::from("<title>");
        escape_into(&mut head, &self.title);
        head.push_str("</title>");
        head.push_str(&out);
        if let Some(canonical) = &self.canonical {
            head.push_str("<link rel=\"canonical\" href=\"");
            escape_into(&mut head, canonical);
            head.push_str("\">");
        }
        if let Some(data) = &self.structured_data {
            // `</` inside a script would end it; JSON allows `<\/`.
            let json = data.to_string().replace("</", "<\\/");
            head.push_str("<script type=\"application/ld+json\" nonce=\"");
            escape_into(&mut head, nonce);
            head.push_str("\">");
            head.push_str(&json);
            head.push_str("</script>");
        }
        Html::from_escaped(head)
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
