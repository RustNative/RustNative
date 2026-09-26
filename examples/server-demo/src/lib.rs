//! The notes server (`PLAN.md` Milestone 49's "done when"): authenticated,
//! database-backed, job-processing traffic from one codebase whose view
//! the Windows client renders natively and the server renders as a page.
//!
//! - Sign-in: a password (Argon2id) for a session in the browser, or a
//!   bearer token for the native client (`SignIn`).
//! - Data: SQLite through checked queries — a misspelt column is a compile
//!   error:
//!
//! ```compile_fail
//! let _ = rustnative_server::query!("SELECT titel FROM notes");
//! ```
//!
//! - Jobs: a new note is indexed in the background, durably.
//! - The admin surface, the API schema, the page head and sitemap, and the
//!   health and metrics endpoints are all on.

use std::time::Duration;

use notes_shared::{CreateNote, Credentials, ListNotes, NewNote, Note, NoteSummary, SignIn};
use rustnative_core::Node;
use rustnative_server::admin::Admin;
use rustnative_server::auth::password;
use rustnative_server::auth::session::{Session, Sessions};
use rustnative_server::auth::token::TokenSigner;
use rustnative_server::auth::{Authentication, PRINCIPAL_KEY, Policy, Principal};
use rustnative_server::components::server_component;
use rustnative_server::config::{Config, Secret};
use rustnative_server::db::migrate::Migrations;
use rustnative_server::db::schema::Schema;
use rustnative_server::db::{Db, DbError};
use rustnative_server::functions::{server_fn, server_fn_with};
use rustnative_server::head::{Head, Sitemap};
use rustnative_server::jobs::{Job, JobContext, Jobs};
use rustnative_server::render::page;
use rustnative_server::{
    CspNonce, CsrfToken, Form, Html, IntoResponse, Redirect, RequestContext, Response, ServerApp,
    ServerError, get, post, query,
};
use serde::{Deserialize, Serialize};

/// Who is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Their id.
    pub id: i64,
    /// Their name.
    pub name: String,
    /// Whether they administer the server.
    pub admin: bool,
}

/// Administrators.
pub struct Admins;

impl Policy<User> for Admins {
    const NAME: &'static str = "admins";
    fn allows(user: &User, _: &RequestContext) -> bool {
        user.admin
    }
}

/// The server's settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Where to listen.
    pub address: String,
    /// The database file.
    pub database: String,
    /// The key sessions and tokens are sealed with (32 bytes, hex).
    pub secret_key: Secret<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:8080".into(),
            database: "notes.db".into(),
            secret_key: Secret::new(String::new()),
        }
    }
}

/// Reads the settings: defaults, `server.toml`, `NOTES_*`, and a secrets
/// directory, in that order.
///
/// # Errors
///
/// A layer is malformed.
pub fn settings(
    directory: &std::path::Path,
) -> Result<(Settings, Vec<String>), rustnative_server::config::ConfigError> {
    let config = Config::new()
        .defaults(&Settings::default())
        .file(directory.join("server.toml"))?
        .env("NOTES_")
        .secrets_dir(directory.join("secrets"))?;
    let sources = config.sources().to_vec();
    Ok((config.build()?, sources))
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
                query!("UPDATE notes SET indexed = 1 WHERE id = ?", id).execute(connection)
            })
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// Opens (and migrates) the database, and adds `ada` (an administrator)
/// and `grace` if there are no users.
///
/// # Errors
///
/// The database cannot be opened or migrated.
pub fn database(db: &Db) -> Result<(), DbError> {
    let migrations = Migrations::from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .map_err(|error| DbError::Task(error.to_string()))?;
    let mut connection = db.get();
    migrations.apply(&mut connection)?;
    let users: i64 = connection.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
    if users == 0 {
        for (name, pass, admin) in [("ada", "analytical engine", 1), ("grace", "compiler", 0)] {
            let hash = password::hash(pass).map_err(DbError::Task)?;
            query!(
                "INSERT INTO users (name, password_hash, admin) VALUES (?, ?, ?)",
                name,
                hash,
                admin
            )
            .execute(&connection)?;
        }
    }
    Ok(())
}

fn find_user(db: &Db, name: &str, pass: &str) -> Option<User> {
    let connection = db.get();
    let row = query!("SELECT id, name, password_hash, admin FROM users WHERE name = ?", name)
        .fetch_optional(&connection)
        .ok()??;
    password::verify(pass, &row.password_hash).then_some(User {
        id: row.id,
        name: row.name,
        admin: row.admin != 0,
    })
}

fn notes_of(db: &Db, owner: i64) -> Result<Vec<Note>, DbError> {
    let connection = db.get();
    Ok(query!("SELECT id, title, indexed FROM notes WHERE owner = ? ORDER BY id", owner)
        .fetch_all(&connection)?
        .into_iter()
        .map(|row| Note { id: row.id, title: row.title, indexed: row.indexed != 0 })
        .collect())
}

fn head() -> Head {
    Head::new(
        "Notes — Rust Native",
        "Your notes, written anywhere and indexed in the background, on every device.",
    )
    .canonical("https://notes.example.com/")
}

async fn home(
    principal: Option<Principal<User>>,
    rustnative_server::State(db): rustnative_server::State<Db>,
    nonce: CspNonce,
    csrf: CsrfToken,
) -> Response {
    match principal {
        Some(Principal(user)) => match notes_of(&db, user.id) {
            // The same view the Windows client realizes, as a page.
            Ok(notes) => page(&head(), &notes_shared::notes_view(&user.name, &notes, ""), &nonce.0).into_response(),
            Err(error) => ServerError::from(error).into_response(),
        },
        None => Html::trusted("<!doctype html><title>Sign in</title><form method=\"post\" action=\"/sign-in\">")
            .and_trusted("<input type=\"hidden\" name=\"_csrf\" value=\"")
            .text(&csrf.0)
            .and_trusted("\"><input name=\"name\"><input name=\"password\" type=\"password\"><button>Sign in</button></form>")
            .into_response(),
    }
}

async fn sign_in_page(
    session: Session,
    rustnative_server::State(db): rustnative_server::State<Db>,
    Form(credentials): Form<Credentials>,
) -> Result<Redirect, ServerError> {
    let user = find_user(&db, &credentials.name, &credentials.password)
        .ok_or_else(ServerError::unauthorized)?;
    session.set(PRINCIPAL_KEY, &user);
    Ok(Redirect::see_other("/"))
}

/// The application, over `db` and `jobs`, sealing sessions and tokens with
/// `key`.
#[must_use]
pub fn app(db: &Db, jobs: &Jobs, key: [u8; 32]) -> ServerApp {
    let signer = TokenSigner::new(Secret::new(key.to_vec()));
    let (before, after) =
        Sessions::new(&Secret::new(key), Duration::from_secs(12 * 3600)).middleware();
    let schema = Schema::of(&db.get()).unwrap_or_default();
    let (sign_db, list_db, create_db, summary_db) =
        (db.clone(), db.clone(), db.clone(), db.clone());
    let create_jobs = jobs.clone();
    let token_signer = signer.clone();

    ServerApp::new()
        .state(db.clone())
        .before(before)
        .after(after)
        .before(Authentication::<User>::sessions().tokens(signer).middleware())
        .openapi("Notes", "1.0.0")
        .route("/", get(home).public())
        .route("/sign-in", post(sign_in_page).public())
        .function::<SignIn>(
            server_fn::<SignIn, _, _>(move |credentials: Credentials| {
                let (db, signer) = (sign_db.clone(), token_signer.clone());
                async move {
                    let user = find_user(&db, &credentials.name, &credentials.password).ok_or_else(ServerError::unauthorized)?;
                    let subject = serde_json::to_string(&user).map_err(|error| ServerError::internal(error.to_string()))?;
                    Ok(signer.issue(subject, Duration::from_secs(12 * 3600)))
                }
            })
            // Signing in is how a native client gets its token; it carries
            // no cookie, so there is nothing to forge.
            .csrf_exempt()
            .public(),
        )
        .function::<ListNotes>(
            server_fn_with::<ListNotes, Principal<User>, _, _>(move |(), Principal(user)| {
                let db = list_db.clone();
                async move { Ok(notes_of(&db, user.id)?) }
            })
            .signed_in::<User>(),
        )
        .function::<CreateNote>(
            server_fn_with::<CreateNote, Principal<User>, _, _>(move |new: NewNote, Principal(user)| {
                let (db, jobs) = (create_db.clone(), create_jobs.clone());
                async move {
                    if new.title.trim().is_empty() {
                        return Err(ServerError::bad_request("A note needs a title"));
                    }
                    let title = new.title.clone();
                    let id = db
                        .run(move |connection| {
                            query!("INSERT INTO notes (owner, title) VALUES (?, ?)", user.id, title).execute(connection)?;
                            Ok(connection.last_insert_rowid())
                        })
                        .await?;
                    jobs.enqueue(&IndexNote { id }, Some(&format!("index:{id}")))?;
                    Ok(Note { id, title: new.title, indexed: false })
                }
            })
            .signed_in::<User>(),
        )
        .component::<NoteSummary>(
            server_component::<NoteSummary, _, _>(move |name: String| {
                let db = summary_db.clone();
                async move {
                    let (count, indexed): (i64, i64) = db.get()
                        .query_row(
                            "SELECT COUNT(*), COALESCE(SUM(indexed), 0) FROM notes JOIN users ON users.id = notes.owner WHERE users.name = ?1",
                            [&name],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .map_err(|error| ServerError::internal(error.to_string()))?;
                    Ok(Node::label("summary", format!("{count} notes, {indexed} indexed")))
                }
            })
            .csrf_exempt()
            .signed_in::<User>(),
        )
        .admin::<User, Admins>(Admin::new(db.clone(), schema))
        .inspection::<User, Admins>(jobs.clone())
        .resource("/sitemap.xml", || {
            let mut response = Sitemap::new("https://notes.example.com").page("/", None).xml().into_response();
            response.headers_mut().insert("content-type", http::HeaderValue::from_static("application/xml"));
            response
        })
        .readiness("database", {
            let db = db.clone();
            move || db.get().query_row("SELECT 1", [], |row| row.get::<_, i64>(0)).is_ok()
        })
}
