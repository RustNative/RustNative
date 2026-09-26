//! The notes client (`PLAN.md` Milestone 49): a Windows application that
//! signs in to the notes server and reads and writes notes through the
//! typed server functions both share. The view is `notes_shared`'s — the
//! same one the server renders as a page.
//!
//! It depends on `notes-shared`, never on the server crate, so nothing
//! server-only is reachable from here (see `notes_shared`).

use std::sync::Arc;

use notes_shared::{
    CreateNote, Credentials, ListNotes, NewNote, Note, NoteSummary, SignIn, notes_view,
};
use rustnative_core::server_fn::{ServerComponentDef, call};
use rustnative_core::wire::WireNode;
use rustnative_core::{
    Component, ComponentContext, Event, HttpRequest, HttpService, Method, Node, NodeId,
};

/// Where the server is, and who signs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The server's base URL.
    pub base: String,
    /// The account.
    pub name: String,
    /// Its password.
    pub password: String,
}

/// What a finished call reports.
#[derive(Debug)]
pub enum Reply {
    /// Signed in, with the bearer header.
    SignedIn(Result<String, String>),
    /// The notes.
    Listed(Result<Vec<Note>, String>),
    /// A note was added.
    Created(Result<Note, String>),
    /// The server-rendered summary's tree.
    Summary(Result<Box<WireNode>, String>),
}

/// The client's root component.
pub struct NotesClient {
    connection: Connection,
    bearer: Option<String>,
    notes: Vec<Note>,
    summary: Option<Node>,
    draft: String,
    status: String,
    pending_add: Option<String>,
    started: bool,
    refresh: bool,
}

/// Fetches a server component's tree payload with the bearer.
async fn summary(
    http: Arc<dyn HttpService>,
    base: String,
    bearer: String,
    name: String,
) -> Result<Box<WireNode>, String> {
    let body = serde_json::to_vec(&name).map_err(|error| error.to_string())?;
    let request = HttpRequest::new(Method::Post, NoteSummary::url(&base))
        .header("content-type", "application/json")
        .header("authorization", bearer)
        .body(body);
    let response = http.execute(request).await.map_err(|error| error.to_string())?;
    serde_json::from_slice(response.body_bytes()).map(Box::new).map_err(|error| error.to_string())
}

impl Component for NotesClient {
    type Props = Connection;
    type Message = Reply;

    fn new(connection: Connection) -> Self {
        Self {
            connection,
            bearer: None,
            notes: Vec::new(),
            summary: None,
            draft: String::new(),
            status: "Signing in…".into(),
            pending_add: None,
            started: false,
            refresh: false,
        }
    }
    fn props(&self) -> &Connection {
        &self.connection
    }
    fn set_props(&mut self, connection: Connection) {
        self.connection = connection;
    }
    fn view(&self) -> Node {
        Node::column("client", [])
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { target, value } if target == NodeId::from_key("draft") => {
                self.draft = value;
            }
            Event::Click { target }
                if target == NodeId::from_key("add") && !self.draft.trim().is_empty() =>
            {
                self.pending_add = Some(std::mem::take(&mut self.draft));
            }
            _ => {}
        }
    }

    fn message(&mut self, reply: Reply) {
        match reply {
            Reply::SignedIn(Ok(bearer)) => {
                self.bearer = Some(bearer);
                self.status = format!("Signed in as {}", self.connection.name);
                self.refresh = true;
            }
            Reply::Listed(Ok(notes)) => self.notes = notes,
            Reply::Created(Ok(note)) => {
                self.notes.push(note);
                self.refresh = true;
            }
            Reply::Summary(Ok(tree)) => self.summary = Some((*tree).into_node()),
            Reply::SignedIn(Err(error))
            | Reply::Listed(Err(error))
            | Reply::Created(Err(error))
            | Reply::Summary(Err(error)) => self.status = error,
        }
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Reply>) -> Node {
        if let Some(http) = context.services().http().cloned() {
            let base = self.connection.base.clone();
            if !self.started {
                self.started = true;
                let credentials = Credentials {
                    name: self.connection.name.clone(),
                    password: self.connection.password.clone(),
                };
                let (http, base) = (Arc::clone(&http), base.clone());
                context.spawn(async move {
                    let token = call::<SignIn>(&*http, &base, &credentials, &[]).await;
                    Reply::SignedIn(
                        token
                            .map(|token| format!("Bearer {token}"))
                            .map_err(|error| error.to_string()),
                    )
                });
            }
            if let Some(bearer) = self.bearer.clone() {
                if let Some(title) = self.pending_add.take() {
                    let (http, base, bearer) = (Arc::clone(&http), base.clone(), bearer.clone());
                    context.spawn(async move {
                        let auth = [("authorization", bearer.as_str())];
                        let note =
                            call::<CreateNote>(&*http, &base, &NewNote { title }, &auth).await;
                        Reply::Created(note.map_err(|error| error.to_string()))
                    });
                }
                if std::mem::take(&mut self.refresh) {
                    let (list_http, list_base, list_bearer) =
                        (Arc::clone(&http), base.clone(), bearer.clone());
                    context.spawn(async move {
                        let auth = [("authorization", list_bearer.as_str())];
                        let notes = call::<ListNotes>(&*list_http, &list_base, &(), &auth).await;
                        Reply::Listed(notes.map_err(|error| error.to_string()))
                    });
                    let name = self.connection.name.clone();
                    context.spawn(async move {
                        Reply::Summary(summary(http, base, bearer, name).await)
                    });
                }
            }
        }
        Node::column(
            "client",
            [
                Node::label("status", self.status.clone()),
                self.summary.clone().unwrap_or_else(|| Node::label("summary", "")),
                notes_view(&self.connection.name, &self.notes, &self.draft),
            ],
        )
    }
}
