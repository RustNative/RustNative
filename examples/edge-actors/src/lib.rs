//! A collaborative document as an actor (`C46`): editors join, append
//! lines, and read; an alarm counts reminders. Its code does not know where
//! it runs — `LocalActorSystem` in a process (`tests/local.rs`) or an edge
//! host, one request per message (`src/main.rs`, built for
//! `wasm32-wasip1`).

use std::time::Duration;

use rustnative_durable::{Actor, ActorContext, Storage};
use serde::{Deserialize, Serialize};

/// The document.
pub struct Document {
    text: String,
    editors: Vec<String>,
    reminders: u32,
}

/// What an editor does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Edit {
    /// Joins the session (and sets the reminder alarm).
    Join(String),
    /// Appends a line: who, and what.
    Append(String, String),
    /// Reads.
    Read,
}

/// The document's state after a message: its text, its editors, and how
/// many reminders have fired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The text.
    pub text: String,
    /// The editors, in joining order.
    pub editors: Vec<String>,
    /// Reminders fired.
    pub reminders: u32,
}

#[async_trait::async_trait]
impl Actor for Document {
    type Message = Edit;
    type Reply = Result<Snapshot, String>;

    fn start(_: &str, storage: &Storage) -> Self {
        Self {
            text: storage.get("text").unwrap_or_default(),
            editors: storage.get("editors").unwrap_or_default(),
            reminders: storage.get("reminders").unwrap_or_default(),
        }
    }

    async fn handle(&mut self, edit: Edit, context: &ActorContext) -> Self::Reply {
        match edit {
            Edit::Join(who) => {
                if !self.editors.contains(&who) {
                    self.editors.push(who);
                }
                context.storage().put("editors", &self.editors).map_err(|e| e.to_string())?;
                context.set_alarm(Duration::ZERO).map_err(|e| e.to_string())?;
            }
            Edit::Append(who, words) => {
                // Serialized: a read-modify-write that cannot lose an update.
                let current = self.text.clone();
                tokio::task::yield_now().await;
                self.text = format!("{current}{who}: {words}\n");
                context.storage().put("text", &self.text).map_err(|e| e.to_string())?;
            }
            Edit::Read => {}
        }
        Ok(Snapshot {
            text: self.text.clone(),
            editors: self.editors.clone(),
            reminders: self.reminders,
        })
    }

    async fn alarm(&mut self, context: &ActorContext) {
        self.reminders += 1;
        let _ = context.storage().put("reminders", &self.reminders);
    }
}
