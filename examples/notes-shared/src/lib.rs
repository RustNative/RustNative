//! The notes application's shared half (`PLAN.md` Milestone 49): typed
//! server functions (one definition, checked where the server answers and
//! where the client calls), the server-only component's definition, and the
//! view — rendered natively by the Windows client and as a page by the
//! server, from the same code.
//!
//! Server-only items are behind the `server` feature, which only the server
//! enables; the rendering of server-only components and everything they
//! reach live in the server crate. Neither this crate nor the client
//! depends on it, so shared or client code that reaches for the server is
//! a compile error, not a runtime one:
//!
//! ```compile_fail
//! use rustnative_server::db::Db;
//! ```

use rustnative_core::api_schema::{ApiSchema, object};
use rustnative_core::server_fn::{ServerComponentDef, ServerFn};
use rustnative_core::{AccessibilityInfo, AccessibilityRole, Node};
use serde::{Deserialize, Serialize};

/// A note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Its id.
    pub id: i64,
    /// Its title.
    pub title: String,
    /// Whether the indexing job has processed it.
    pub indexed: bool,
}

impl ApiSchema for Note {
    fn schema() -> serde_json::Value {
        object(
            "Note",
            [
                ("id", i64::schema(), true),
                ("title", String::schema(), true),
                ("indexed", bool::schema(), true),
            ],
        )
    }
}

/// Signing in: a name and a password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    /// Who.
    pub name: String,
    /// Their password.
    pub password: String,
}

impl ApiSchema for Credentials {
    fn schema() -> serde_json::Value {
        object(
            "Credentials",
            [("name", String::schema(), true), ("password", String::schema(), true)],
        )
    }
}

/// A new note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewNote {
    /// Its title.
    pub title: String,
}

impl ApiSchema for NewNote {
    fn schema() -> serde_json::Value {
        object("NewNote", [("title", String::schema(), true)])
    }
}

/// Signs in and returns a bearer token for the native client.
pub struct SignIn;
impl ServerFn for SignIn {
    const PATH: &'static str = "auth/sign-in";
    type Input = Credentials;
    type Output = String;
}

/// The signed-in person's notes.
pub struct ListNotes;
impl ServerFn for ListNotes {
    const PATH: &'static str = "notes/list";
    type Input = ();
    type Output = Vec<Note>;
}

/// Adds a note; the server indexes it in the background.
pub struct CreateNote;
impl ServerFn for CreateNote {
    const PATH: &'static str = "notes/create";
    type Input = NewNote;
    type Output = Note;
}

/// A summary of the signed-in person's notes, rendered on the server
/// (where the counts are) and merged into the client's tree.
pub struct NoteSummary;
impl ServerComponentDef for NoteSummary {
    const NAME: &'static str = "note-summary";
    type Props = String;
}

/// The notes view: the same function builds the Windows client's tree and
/// the server's page.
#[must_use]
pub fn notes_view(owner: &str, notes: &[Note], draft: &str) -> Node {
    Node::column(
        "notes",
        [
            Node::label("heading", format!("{owner}'s notes")).with_accessibility(
                AccessibilityInfo::new(AccessibilityRole::Heading { level: 1 }),
            ),
            Node::row(
                "compose",
                [Node::text_input("draft", draft), Node::button("add", "Add note")],
            ),
            Node::column(
                "list",
                notes.iter().map(|note| {
                    let text = if note.indexed {
                        note.title.clone()
                    } else {
                        format!("{} (indexing…)", note.title)
                    };
                    Node::label(format!("note-{}", note.id), text).with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::ListItem)
                            .name(note.title.clone()),
                    )
                }),
            )
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List)),
        ],
    )
}

/// What only the server may use.
#[cfg(feature = "server")]
pub mod server_only {
    /// How long a sign-in lasts.
    pub const SESSION_LIFETIME_HOURS: u64 = 12;
}
