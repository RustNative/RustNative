//! The generated administrative surface: list, view, create, edit, and
//! delete for every table of the model (`schema.toml`), served under
//! `/admin` to principals a policy allows.
//!
//! Table and column names come from the model, never from the request;
//! values are bound parameters; every page is escaping [`Html`] with the
//! request-forgery token in its forms.

use std::sync::Arc;

use rusqlite::types::Value;

use crate::auth::Policy;
use crate::db::Db;
use crate::db::schema::{Column, Schema, Table};
use crate::request::{FromRequest, Path, RequestContext};
use crate::response::{Html, IntoResponse, Redirect, Response, ServerError};
use crate::security::CsrfToken;
use crate::{ServerApp, get};

/// The admin surface over `db`, described by `schema`.
#[derive(Clone)]
pub struct Admin {
    db: Db,
    schema: Arc<Schema>,
}

fn page(title: &str, body: &Html) -> Response {
    Html::trusted("<!doctype html><meta charset=\"utf-8\"><title>")
        .text(title)
        .and_trusted("</title><h1>")
        .text(title)
        .and_trusted("</h1>")
        .html(body)
        .into_response()
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Integer(integer) => integer.to_string(),
        Value::Real(real) => real.to_string(),
        Value::Text(text) => text.clone(),
        Value::Blob(blob) => format!("({} bytes)", blob.len()),
    }
}

fn input_type(column: &Column) -> &'static str {
    let kind = column.kind.to_ascii_uppercase();
    if kind.contains("INT") || kind.contains("REAL") || kind.contains("NUM") {
        "number"
    } else {
        "text"
    }
}

fn parse(column: &Column, raw: &str) -> Value {
    if raw.is_empty() && !column.not_null {
        return Value::Null;
    }
    let kind = column.kind.to_ascii_uppercase();
    if kind.contains("INT") {
        raw.parse().map_or_else(|_| Value::Text(raw.to_owned()), Value::Integer)
    } else if kind.contains("REAL") || kind.contains("FLOA") || kind.contains("DOUB") {
        raw.parse().map_or_else(|_| Value::Text(raw.to_owned()), Value::Real)
    } else {
        Value::Text(raw.to_owned())
    }
}

impl Admin {
    /// The surface over `db` and its model.
    #[must_use]
    pub fn new(db: Db, schema: Schema) -> Self {
        Self { db, schema: Arc::new(schema) }
    }

    fn table(&self, name: &str) -> Result<(&str, &Table), ServerError> {
        self.schema
            .tables
            .get_key_value(name)
            .map(|(name, table)| (name.as_str(), table))
            .ok_or_else(ServerError::not_found)
    }

    fn key(table: &Table) -> &str {
        table
            .columns
            .iter()
            .find(|column| column.primary_key)
            .map_or("rowid", |column| column.name.as_str())
    }

    fn index(&self) -> Response {
        let mut body = Html::trusted("<ul>");
        for name in self.schema.tables.keys() {
            body = body
                .and_trusted("<li><a href=\"/admin/")
                .text(name)
                .and_trusted("\">")
                .text(name)
                .and_trusted("</a></li>");
        }
        page("Administration", &body.and_trusted("</ul>"))
    }

    fn list(&self, name: &str) -> Result<Response, ServerError> {
        let (name, table) = self.table(name)?;
        let key = Self::key(table);
        let columns = table.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>();
        let connection = self.db.get();
        let mut statement = connection
            .prepare(&format!(
                "SELECT {key}, {} FROM {name} ORDER BY {key} LIMIT 200",
                columns.join(", ")
            ))
            .map_err(|error| ServerError::internal(error.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                (0..=columns.len())
                    .map(|index| row.get::<_, Value>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .map_err(|error| ServerError::internal(error.to_string()))?;
        let mut body = Html::trusted("<p><a href=\"/admin/")
            .text(name)
            .and_trusted("/new\">New</a></p><table><tr>");
        for column in &columns {
            body = body.and_trusted("<th>").text(column).and_trusted("</th>");
        }
        body = body.and_trusted("</tr>");
        for row in rows {
            let id = text(&row[0]);
            body = body.and_trusted("<tr>");
            for (index, value) in row.iter().skip(1).enumerate() {
                body = if index == 0 {
                    body.and_trusted("<td><a href=\"/admin/")
                        .text(name)
                        .and_trusted("/")
                        .text(&id)
                        .and_trusted("\">")
                        .text(&text(value))
                        .and_trusted("</a></td>")
                } else {
                    body.and_trusted("<td>").text(&text(value)).and_trusted("</td>")
                };
            }
            body = body.and_trusted("</tr>");
        }
        Ok(page(name, &body.and_trusted("</table>")))
    }

    fn form(&self, name: &str, id: Option<&str>, csrf: &str) -> Result<Response, ServerError> {
        let (name, table) = self.table(name)?;
        let key = Self::key(table);
        let values: Vec<Value> = match id {
            Some(id) => {
                let columns = table
                    .columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let connection = self.db.get();
                connection
                    .query_row(
                        &format!("SELECT {columns} FROM {name} WHERE {key} = ?1"),
                        [id],
                        |row| {
                            (0..table.columns.len())
                                .map(|index| row.get::<_, Value>(index))
                                .collect()
                        },
                    )
                    .map_err(|_| ServerError::not_found())?
            }
            None => vec![Value::Null; table.columns.len()],
        };
        let mut body =
            Html::trusted("<form method=\"post\"><input type=\"hidden\" name=\"_csrf\" value=\"")
                .text(csrf)
                .and_trusted("\">");
        for (column, value) in table.columns.iter().zip(&values) {
            if column.primary_key && column.kind.eq_ignore_ascii_case("INTEGER") {
                continue;
            }
            body = body
                .and_trusted("<p><label>")
                .text(&column.name)
                .and_trusted(" <input name=\"")
                .text(&column.name)
                .and_trusted("\" type=\"")
                .text(input_type(column))
                .and_trusted("\" value=\"")
                .text(&text(value))
                .and_trusted(if column.not_null {
                    "\" required></label></p>"
                } else {
                    "\"></label></p>"
                });
        }
        body = body.and_trusted("<button>Save</button></form>");
        if let Some(id) = id {
            body = body
                .and_trusted("<form method=\"post\" action=\"/admin/")
                .text(name)
                .and_trusted("/")
                .text(id)
                .and_trusted("/delete\"><input type=\"hidden\" name=\"_csrf\" value=\"")
                .text(csrf)
                .and_trusted("\"><button>Delete</button></form>");
        }
        Ok(page(name, &body))
    }

    fn save(
        &self,
        name: &str,
        id: Option<&str>,
        fields: &[(String, String)],
    ) -> Result<Response, ServerError> {
        let (name, table) = self.table(name)?;
        let key = Self::key(table);
        let mut columns = Vec::new();
        let mut values = Vec::new();
        for column in &table.columns {
            if let Some((_, raw)) = fields.iter().find(|(field, _)| *field == column.name) {
                columns.push(column.name.as_str());
                values.push(parse(column, raw));
            }
        }
        let connection = self.db.get();
        let result = if let Some(id) = id {
            let assignments = columns
                .iter()
                .enumerate()
                .map(|(index, column)| format!("{column} = ?{}", index + 1))
                .collect::<Vec<_>>();
            values.push(Value::Text(id.to_owned()));
            connection.execute(
                &format!(
                    "UPDATE {name} SET {} WHERE {key} = ?{}",
                    assignments.join(", "),
                    values.len()
                ),
                rusqlite::params_from_iter(values.iter()),
            )
        } else {
            let placeholders =
                (1..=columns.len()).map(|index| format!("?{index}")).collect::<Vec<_>>();
            connection.execute(
                &format!(
                    "INSERT INTO {name} ({}) VALUES ({})",
                    columns.join(", "),
                    placeholders.join(", ")
                ),
                rusqlite::params_from_iter(values.iter()),
            )
        };
        result.map_err(|error| ServerError::bad_request(error.to_string()))?;
        Ok(Redirect::see_other(format!("/admin/{name}")).into_response())
    }

    fn delete(&self, name: &str, id: &str) -> Result<Response, ServerError> {
        let (name, table) = self.table(name)?;
        let key = Self::key(table);
        self.db
            .get()
            .execute(&format!("DELETE FROM {name} WHERE {key} = ?1"), [id])
            .map_err(|error| ServerError::bad_request(error.to_string()))?;
        Ok(Redirect::see_other(format!("/admin/{name}")).into_response())
    }
}

/// The form's fields, in order.
struct Fields(Vec<(String, String)>);

impl FromRequest for Fields {
    fn from_request(request: &RequestContext) -> Result<Self, ServerError> {
        let text = std::str::from_utf8(request.body())
            .map_err(|_| ServerError::bad_request("The form is not UTF-8"))?;
        Ok(Self(
            crate::request::form_pairs(text)
                .into_iter()
                .filter(|(key, _)| key != "_csrf")
                .collect(),
        ))
    }
}

impl ServerApp {
    /// Serves `admin` under `/admin` to principals `Pol` allows.
    #[must_use]
    pub fn admin<P, Pol>(self, admin: Admin) -> Self
    where
        P: Clone + Send + Sync + 'static,
        Pol: Policy<P>,
    {
        let (index, listing, creating, editing, deleting) =
            (admin.clone(), admin.clone(), admin.clone(), admin.clone(), admin);
        self.route(
            "/admin",
            get(move || {
                let admin = index.clone();
                async move { admin.index() }
            })
            .authorized::<P, Pol>(),
        )
        .route(
            "/admin/:table",
            get(move |Path(table): Path<String>| {
                let admin = listing.clone();
                async move { admin.list(&table) }
            })
            .authorized::<P, Pol>(),
        )
        .route(
            "/admin/:table/new",
            get({
                let creating = creating.clone();
                move |Path(table): Path<String>, token: CsrfToken| {
                    let admin = creating.clone();
                    async move { admin.form(&table, None, &token.0) }
                }
            })
            .post(move |Path(table): Path<String>, Fields(fields): Fields| {
                let admin = creating.clone();
                async move { admin.save(&table, None, &fields) }
            })
            .authorized::<P, Pol>(),
        )
        .route(
            "/admin/:table/:id",
            get({
                let editing = editing.clone();
                move |Path((table, id)): Path<(String, String)>, token: CsrfToken| {
                    let admin = editing.clone();
                    async move { admin.form(&table, Some(&id), &token.0) }
                }
            })
            .post(move |Path((table, id)): Path<(String, String)>, Fields(fields): Fields| {
                let admin = editing.clone();
                async move { admin.save(&table, Some(&id), &fields) }
            })
            .authorized::<P, Pol>(),
        )
        .route(
            "/admin/:table/:id/delete",
            crate::post(move |Path((table, id)): Path<(String, String)>| {
                let admin = deleting.clone();
                async move { admin.delete(&table, &id) }
            })
            .authorized::<P, Pol>(),
        )
    }
}
