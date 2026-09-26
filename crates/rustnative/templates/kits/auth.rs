//! Accounts: sign-up, sign-in, sign-out, and who is signed in — generated
//! by `rustnative generate kit auth` (`PLAN.md` Milestone 52, `C57-3`).
//! It is ordinary code in your project: change it as you need.
//!
//! - Passwords are hashed with Argon2id; a name is taken once.
//! - The session lives in a sealed cookie; every change needs the
//!   request-forgery token, as every unsafe request does.
//! - `/auth/me` is for signed-in accounts only.

use std::time::Duration;

use rustnative_server::auth::password;
use rustnative_server::auth::session::{Session, Sessions};
use rustnative_server::auth::{Authentication, PRINCIPAL_KEY, Principal};
use rustnative_server::config::Secret;
use rustnative_server::db::{Db, DbError};
use rustnative_server::{Json, ServerApp, ServerError, State, get, post};
use serde::{Deserialize, Serialize};

/// A signed-in account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Its id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// Whether it administers the application (the first account does;
    /// the admin kit's policy reads this).
    pub admin: bool,
}

/// What sign-up and sign-in send.
#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    /// The account's name.
    pub name: String,
    /// Its password.
    pub password: String,
}

/// Creates the accounts table if it is missing.
///
/// # Errors
///
/// The database refused.
pub fn migrate(db: &Db) -> Result<(), DbError> {
    db.get().execute_batch(
        "CREATE TABLE IF NOT EXISTS accounts (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            admin INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    Ok(())
}

async fn sign_up(
    session: Session,
    State(db): State<Db>,
    Json(credentials): Json<Credentials>,
) -> Result<Json<Account>, ServerError> {
    let name = credentials.name.trim().to_owned();
    if name.is_empty() {
        return Err(ServerError::bad_request("An account needs a name"));
    }
    if credentials.password.chars().count() < 8 {
        return Err(ServerError::bad_request("A password needs at least 8 characters"));
    }
    let hash = password::hash(&credentials.password).map_err(ServerError::internal)?;
    let stored = name.clone();
    let (id, admin) = db
        .run(move |connection| {
            // The first account administers; the rest do not.
            connection.execute(
                "INSERT INTO accounts (name, password_hash, admin) VALUES (?1, ?2, NOT EXISTS (SELECT 1 FROM accounts))",
                (&stored, &hash),
            )?;
            let id = connection.last_insert_rowid();
            let admin: bool = connection.query_row("SELECT admin FROM accounts WHERE id = ?1", [id], |row| row.get(0))?;
            Ok((id, admin))
        })
        .await
        .map_err(|_| ServerError::bad_request("That name is taken"))?;
    let account = Account { id, name, admin };
    session.set(PRINCIPAL_KEY, &account);
    Ok(Json(account))
}

async fn sign_in(
    session: Session,
    State(db): State<Db>,
    Json(credentials): Json<Credentials>,
) -> Result<Json<Account>, ServerError> {
    let name = credentials.name.trim().to_owned();
    let row: Option<(i64, String, bool)> = db
        .get()
        .query_row(
            "SELECT id, password_hash, admin FROM accounts WHERE name = ?1",
            [&name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();
    // The same answer for an unknown name and a wrong password.
    let (id, hash, admin) = row.ok_or_else(ServerError::unauthorized)?;
    if !password::verify(&credentials.password, &hash) {
        return Err(ServerError::unauthorized());
    }
    let account = Account { id, name, admin };
    session.set(PRINCIPAL_KEY, &account);
    Ok(Json(account))
}

async fn sign_out(session: Session) -> Json<bool> {
    session.clear();
    Json(true)
}

async fn me(Principal(account): Principal<Account>) -> Json<Account> {
    Json(account)
}

/// Adds sessions and the account routes to `app`, sealing sessions with
/// `key` (32 bytes from your secrets, never from source).
#[must_use]
pub fn routes(app: ServerApp, db: &Db, key: [u8; 32]) -> ServerApp {
    let (before, after) =
        Sessions::new(&Secret::new(key), Duration::from_secs(12 * 3600)).middleware();
    app.state(db.clone())
        .before(before)
        .after(after)
        .before(Authentication::<Account>::sessions().middleware())
        .route("/auth/sign-up", post(sign_up).public())
        .route("/auth/sign-in", post(sign_in).public())
        .route("/auth/sign-out", post(sign_out).public())
        .route("/auth/me", get(me).signed_in::<Account>())
}
