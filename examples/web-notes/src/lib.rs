//! Notes on the Web (`PLAN.md` Milestone 49's done-when, in the Web track's
//! shapes): one application whose UI runs client-side, server-rendered,
//! and serverless without modification.
//!
//! - [`editor`] is the UI's interactive part: a client component (an
//!   island) that lists the notes and adds one through a server function.
//! - [`app`] is the whole application over a [`Data`]: sign-in (sessions
//!   sealed in a cookie, so any instance can read them), the notes page,
//!   and the `add_note` server function.
//! - [`Data::Local`] is SQLite with a job that indexes each new note: the
//!   long-lived server (`src/main.rs`), which also serves the data to the
//!   other shapes at `/data/*`. [`Data::Remote`] is that data service over
//!   HTTP: the Lambda function (`src/bin/lambda.rs`) and the edge module
//!   (`src/bin/edge.rs`), which keep nothing between invocations.
//! - [`site`] is the static export: the same page, its island calling the
//!   server's functions.

#![allow(
    clippy::needless_pass_by_value,
    clippy::semicolon_if_nothing_returned,
    reason = "client logic written as an application would"
)]

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::auth::session::{Session, Sessions};
use rustnative_server::auth::{Authentication, PRINCIPAL_KEY, Principal};
use rustnative_server::config::Secret;
use rustnative_server::functions::server_fn_with;
use rustnative_server::{
    Form, IntoResponse, Redirect, Response, ServerApp, ServerError, State, get, post,
};
use rustnative_web::form::form;
use rustnative_web::{Client, Head, Page};
use serde::{Deserialize, Serialize};

pub use editor::{AddNote, Note};

#[rustnative_web::client]
pub mod editor {
    use rustnative_core::server_fn::{ServerFn, ServerFnError};
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    /// A note.
    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Note {
        /// Its id.
        pub id: i64,
        /// Its title.
        pub title: String,
        /// Whether the background job has indexed it.
        pub indexed: bool,
    }

    /// Adds a note for whoever is signed in.
    pub struct AddNote;

    impl ServerFn for AddNote {
        const PATH: &'static str = "add_note";
        type Input = String;
        type Output = Note;
    }

    /// The notes, and the one being written.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Editor {
        /// The notes, oldest first.
        pub notes: Vec<Note>,
        /// The title being written.
        pub draft: String,
        /// Why the last add failed.
        pub error: String,
    }

    /// What the server answered.
    pub enum Msg {
        /// `add_note`'s answer.
        Added(Result<Note, ServerFnError>),
    }

    impl Editor {
        /// Writing and adding.
        pub fn update(&mut self, event: Event, fx: &mut Effects<Msg>) {
            match event {
                Event::TextChanged { target, value } if target == NodeId::from_key("draft") => {
                    self.draft = value;
                }
                Event::Click { target } if target == NodeId::from_key("add") => {
                    if self.draft.trim().is_empty() {
                        self.error = String::from("A note needs a title");
                    } else {
                        fx.call::<AddNote>(self.draft.trim().to_owned(), Msg::Added);
                    }
                }
                _ => {}
            }
        }

        /// A note added, or why not.
        pub fn message(&mut self, message: Msg, fx: &mut Effects<Msg>) {
            let _ = fx;
            match message {
                Msg::Added(Ok(note)) => {
                    self.notes.push(note);
                    self.draft = String::new();
                    self.error = String::new();
                }
                Msg::Added(Err(error)) => self.error = format!("Not added: {error}"),
            }
        }

        /// The list, the field, and the button.
        #[must_use]
        pub fn view(&self) -> Node {
            let count = self.notes.len();
            Node::column(
                "editor",
                [
                    Node::label("count", format!("{count} notes")),
                    Node::column(
                        "list",
                        self.notes.iter().map(|note| {
                            let mark = if note.indexed { "" } else { " (indexing)" };
                            Node::label(format!("n{}", note.id), format!("{}{mark}", note.title))
                        }),
                    ),
                    Node::text_input("draft", self.draft.clone()),
                    Node::button("add", "Add"),
                    Node::label("error", self.error.clone()),
                ],
            )
        }
    }
}

impl rustnative_core::api_schema::ApiSchema for Note {
    fn schema() -> serde_json::Value {
        rustnative_core::api_schema::object(
            "Note",
            [
                ("id", i64::schema(), true),
                ("title", String::schema(), true),
                ("indexed", bool::schema(), true),
            ],
        )
    }
}

/// Who is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Their id.
    pub id: i64,
    /// Their name.
    pub name: String,
}

/// A name and a password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// The name.
    pub name: String,
    /// The password.
    pub password: String,
}

/// Where the notes live.
#[derive(Clone)]
pub enum Data {
    /// SQLite, and the jobs that index notes: the long-lived server.
    #[cfg(feature = "server")]
    Local(local::Local),
    /// The server's data service, over HTTP: the serverless shapes.
    Remote {
        /// Its address (`http://host:port`).
        base: String,
        /// The key it asks for (`x-data-key`).
        key: String,
    },
}

impl Data {
    /// The data service named by `NOTES_DATA_URL` and `NOTES_DATA_KEY`: how
    /// the serverless shapes are configured, per invocation.
    #[must_use]
    pub fn from_env() -> Self {
        Self::Remote {
            base: std::env::var("NOTES_DATA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            key: std::env::var("NOTES_DATA_KEY").unwrap_or_default(),
        }
    }

    fn remote<T: serde::de::DeserializeOwned>(
        base: &str,
        key: &str,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, ServerError> {
        let body = if body.is_null() { String::new() } else { body.to_string() };
        let reply = rustnative_server::serverless::outbound::send(
            method,
            &format!("{base}{path}"),
            &[
                ("x-data-key", key),
                ("content-type", "application/json"),
                ("accept", "application/json"),
            ],
            &body,
        )
        .map_err(|error| ServerError::internal(format!("the data service: {error}")))?;
        match reply.status {
            200 => serde_json::from_str(&reply.body)
                .map_err(|error| ServerError::internal(error.to_string())),
            401 => Err(ServerError::unauthorized()),
            400 => Err(ServerError::bad_request(reply.body)),
            status => Err(ServerError::internal(format!("the data service answered {status}"))),
        }
    }

    /// The user `credentials` name, if the password is theirs.
    ///
    /// # Errors
    ///
    /// The data could not be reached.
    pub fn sign_in(&self, credentials: &Credentials) -> Result<Option<User>, ServerError> {
        match self {
            #[cfg(feature = "server")]
            Self::Local(local) => Ok(local.sign_in(credentials)),
            Self::Remote { base, key } => {
                match Self::remote(
                    base,
                    key,
                    "POST",
                    "/data/sign-in",
                    &serde_json::to_value(credentials).unwrap_or_default(),
                ) {
                    Ok(user) => Ok(Some(user)),
                    Err(error) if error.status().as_u16() == 401 => Ok(None),
                    Err(error) => Err(error),
                }
            }
        }
    }

    /// `owner`'s notes, oldest first.
    ///
    /// # Errors
    ///
    /// The data could not be reached.
    pub fn notes(&self, owner: i64) -> Result<Vec<Note>, ServerError> {
        match self {
            #[cfg(feature = "server")]
            Self::Local(local) => local.notes(owner),
            Self::Remote { base, key } => Self::remote(
                base,
                key,
                "GET",
                &format!("/data/notes?owner={owner}"),
                &serde_json::Value::Null,
            ),
        }
    }

    /// Adds a note titled `title` for `owner`; the job indexes it.
    ///
    /// # Errors
    ///
    /// The title is empty, or the data could not be reached.
    pub fn add(&self, owner: i64, title: &str) -> Result<Note, ServerError> {
        if title.trim().is_empty() {
            return Err(ServerError::bad_request("A note needs a title"));
        }
        match self {
            #[cfg(feature = "server")]
            Self::Local(local) => local.add(owner, title),
            Self::Remote { base, key } => Self::remote(
                base,
                key,
                "POST",
                "/data/notes",
                &serde_json::json!({ "owner": owner, "title": title }),
            ),
        }
    }
}

/// The notes page's props.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotesProps {
    /// Who is signed in.
    pub name: String,
    /// Their notes.
    pub notes: Vec<Note>,
}

/// The signed-in page: a greeting, and the editor island.
pub struct NotesPage(NotesProps);

impl Component for NotesPage {
    type Props = NotesProps;
    type Message = ();
    fn new(props: NotesProps) -> Self {
        Self(props)
    }
    fn props(&self) -> &NotesProps {
        &self.0
    }
    fn set_props(&mut self, props: NotesProps) {
        self.0 = props;
    }
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let editor = context.child_with_props::<Client<editor::Editor>, _>(
            "editor",
            editor::Editor { notes: self.0.notes.clone(), ..editor::Editor::default() },
            Client::new,
        );
        Node::column(
            "page",
            [
                Node::label("title", "Notes"),
                Node::label("hello", format!("Signed in as {}", self.0.name)),
                editor,
            ],
        )
    }
}

/// The sign-in page: a form that works with scripts off.
pub struct SignInPage(bool);

impl Component for SignInPage {
    type Props = bool;
    type Message = ();
    fn new(failed: bool) -> Self {
        Self(failed)
    }
    fn props(&self) -> &bool {
        &self.0
    }
    fn set_props(&mut self, failed: bool) {
        self.0 = failed;
    }
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let message = if self.0 { "That name and password do not match." } else { "" };
        Node::column(
            "page",
            [
                Node::label("title", "Sign in"),
                Node::label("message", message),
                form(
                    context,
                    "sign-in",
                    "/sign-in",
                    [
                        Node::text_input("name", ""),
                        Node::text_input("password", ""),
                        Node::button("go", "Sign in"),
                    ],
                ),
            ],
        )
    }
}

fn head() -> Head {
    Head::new("Notes", "Your notes, written anywhere and indexed in the background.")
}

/// The notes page for `props`: what every shape renders.
#[must_use]
pub fn notes_page(props: NotesProps) -> Page {
    Page::new::<NotesPage>(head(), props).lang("en")
}

async fn home(
    principal: Option<Principal<User>>,
    State(data): State<Data>,
    failed: Option<rustnative_server::Query<Failed>>,
) -> Response {
    let Some(Principal(user)) = principal else {
        let failed = failed.is_some_and(|query| query.0.failed.is_some());
        return Page::new::<SignInPage>(head(), failed).lang("en").into_response();
    };
    match data.notes(user.id) {
        Ok(notes) => notes_page(NotesProps { name: user.name, notes }).into_response(),
        Err(error) => error.into_response(),
    }
}

/// `?failed`: the last sign-in did not match.
#[derive(Debug, Deserialize)]
pub struct Failed {
    failed: Option<String>,
}

async fn sign_in(
    session: Session,
    State(data): State<Data>,
    Form(credentials): Form<Credentials>,
) -> Result<Redirect, ServerError> {
    Ok(match data.sign_in(&credentials)? {
        Some(user) => {
            session.set(PRINCIPAL_KEY, &user);
            Redirect::see_other("/")
        }
        None => Redirect::see_other("/?failed=1"),
    })
}

/// The application over `data`, sealing sessions with `key`. The same in
/// every shape; only `data` differs.
#[must_use]
pub fn app(data: Data, key: [u8; 32]) -> ServerApp {
    let (before, after) =
        Sessions::new(&Secret::new(key), std::time::Duration::from_secs(12 * 3600)).middleware();
    let add_data = data.clone();
    ServerApp::new()
        .state(data)
        .before(before)
        .after(after)
        .before(Authentication::<User>::sessions().middleware())
        .route("/", get(home).public())
        .route("/sign-in", post(sign_in).public())
        .function::<AddNote>(
            server_fn_with::<AddNote, Principal<User>, _, _>(
                move |title: String, Principal(user)| {
                    let data = add_data.clone();
                    async move { data.add(user.id, &title) }
                },
            )
            .signed_in::<User>(),
        )
}

/// The session key named by `NOTES_SECRET_KEY` (64 hex digits), so every
/// instance of every shape reads the same sessions.
#[must_use]
pub fn key_from_env() -> [u8; 32] {
    let hex = std::env::var("NOTES_SECRET_KEY").unwrap_or_default();
    let mut key = [0u8; 32];
    if hex.len() == 64 {
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap_or(0);
        }
    }
    key
}

/// The static export: the notes page with `notes`, its island calling the
/// server's functions.
#[must_use]
pub fn site(notes: Vec<Note>) -> rustnative_web::export::Site {
    rustnative_web::export::Site::new()
        .page("/", move || notes_page(NotesProps { name: "ada".into(), notes: notes.clone() }))
}

/// The long-lived server's data: SQLite, the indexing job, and the data
/// service the serverless shapes use.
#[cfg(feature = "server")]
pub mod local {
    use http::HeaderMap;
    use rustnative_server::auth::password;
    use rustnative_server::db::{Db, DbError};
    use rustnative_server::jobs::{Job, JobContext, Jobs};
    use rustnative_server::{Json, Query, ServerApp, ServerError, constant_time_eq, get, post};
    use serde::{Deserialize, Serialize};

    use super::{Credentials, Note, User};

    /// SQLite and its jobs.
    #[derive(Clone)]
    pub struct Local {
        /// The database.
        pub db: Db,
        /// The job queue.
        pub jobs: Jobs,
    }

    /// Indexes a note (the background job).
    #[derive(Debug, Serialize, Deserialize)]
    pub struct IndexNote {
        /// The note.
        pub id: i64,
    }

    #[async_trait::async_trait]
    impl Job for IndexNote {
        const KIND: &'static str = "index-note";
        async fn run(&self, context: &JobContext) -> Result<(), String> {
            let id = self.id;
            context
                .db
                .run(move |connection| {
                    connection
                        .execute("UPDATE notes SET indexed = 1 WHERE id = ?1", [id])
                        .map_err(DbError::from)
                })
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    fn internal(error: impl std::fmt::Display) -> ServerError {
        ServerError::internal(error.to_string())
    }

    impl Local {
        /// Opens `path`, creating the tables and the users `ada` and
        /// `grace` if there are none.
        ///
        /// # Errors
        ///
        /// SQLite refused.
        pub fn open(path: &str) -> Result<Self, DbError> {
            let db = Db::open(path, 4)?;
            let connection = db.get();
            connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT UNIQUE NOT NULL, password_hash TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, owner INTEGER NOT NULL, title TEXT NOT NULL, indexed INTEGER NOT NULL DEFAULT 0);",
            )?;
            let users: i64 =
                connection.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
            if users == 0 {
                for (name, pass) in [("ada", "analytical engine"), ("grace", "compiler")] {
                    let hash = password::hash(pass).map_err(DbError::Task)?;
                    connection.execute(
                        "INSERT INTO users (name, password_hash) VALUES (?1, ?2)",
                        [name, hash.as_str()],
                    )?;
                }
            }
            drop(connection);
            let jobs = Jobs::new(db.clone())?.register::<IndexNote>();
            Ok(Self { db, jobs })
        }

        pub(crate) fn sign_in(&self, credentials: &Credentials) -> Option<User> {
            let connection = self.db.get();
            let row: Option<(i64, String, String)> = connection
                .query_row(
                    "SELECT id, name, password_hash FROM users WHERE name = ?1",
                    [&credentials.name],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .ok();
            row.and_then(|(id, name, hash)| {
                password::verify(&credentials.password, &hash).then_some(User { id, name })
            })
        }

        pub(crate) fn notes(&self, owner: i64) -> Result<Vec<Note>, ServerError> {
            let connection = self.db.get();
            let mut statement = connection
                .prepare("SELECT id, title, indexed FROM notes WHERE owner = ?1 ORDER BY id")
                .map_err(internal)?;
            let rows = statement
                .query_map([owner], |row| {
                    Ok(Note {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        indexed: row.get::<_, i64>(2)? != 0,
                    })
                })
                .map_err(internal)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(internal)
        }

        pub(crate) fn add(&self, owner: i64, title: &str) -> Result<Note, ServerError> {
            let connection = self.db.get();
            connection
                .execute("INSERT INTO notes (owner, title) VALUES (?1, ?2)", (owner, title))
                .map_err(internal)?;
            let id = connection.last_insert_rowid();
            drop(connection);
            self.jobs.enqueue(&IndexNote { id }, Some(&format!("index:{id}"))).map_err(internal)?;
            Ok(Note { id, title: title.to_owned(), indexed: false })
        }
    }

    /// `?owner=`.
    #[derive(Debug, Deserialize)]
    struct Owner {
        owner: i64,
    }

    /// A new note for `owner`.
    #[derive(Debug, Deserialize)]
    struct NewNote {
        owner: i64,
        title: String,
    }

    fn check(headers: &HeaderMap, key: &str) -> Result<(), ServerError> {
        let given =
            headers.get("x-data-key").and_then(|value| value.to_str().ok()).unwrap_or_default();
        if key.is_empty() || !constant_time_eq(given.as_bytes(), key.as_bytes()) {
            return Err(ServerError::unauthorized());
        }
        Ok(())
    }

    /// The data service at `/data/*`, for callers that present `key`.
    #[must_use]
    pub fn data_service(app: ServerApp, local: &Local, key: String) -> ServerApp {
        let (sign, list, add) = (local.clone(), local.clone(), local.clone());
        let (sign_key, list_key, add_key) = (key.clone(), key.clone(), key);
        app.route(
            "/data/sign-in",
            post(move |headers: HeaderMap, Json(credentials): Json<Credentials>| {
                let (local, key) = (sign.clone(), sign_key.clone());
                async move {
                    check(&headers, &key)?;
                    local.sign_in(&credentials).map(Json).ok_or_else(ServerError::unauthorized)
                }
            })
            .csrf_exempt()
            .public(),
        )
        .route(
            "/data/notes",
            get(move |headers: HeaderMap, Query(owner): Query<Owner>| {
                let (local, key) = (list.clone(), list_key.clone());
                async move {
                    check(&headers, &key)?;
                    local.notes(owner.owner).map(Json)
                }
            })
            .post(move |headers: HeaderMap, Json(new): Json<NewNote>| {
                let (local, key) = (add.clone(), add_key.clone());
                async move {
                    check(&headers, &key)?;
                    local.add(new.owner, &new.title).map(Json)
                }
            })
            .csrf_exempt()
            .public(),
        )
    }
}
