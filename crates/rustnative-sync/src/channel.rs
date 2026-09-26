//! Channels and presence, as service contracts every mode uses — a local
//! component, a server-interactive screen, a device.
//!
//! A [`Channel`] carries messages on topics. [`Presence`] says who is on a
//! topic: members join with their metadata, heartbeat to stay, and are
//! dropped when their heartbeats stop (a closed laptop leaves without
//! saying so). [`LocalHub`] implements both in process.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::broadcast;

/// Publish and subscribe on named topics.
pub trait Channel: Send + Sync {
    /// Sends `message` to everyone subscribed to `topic`; how many received it.
    fn publish(&self, topic: &str, message: Value) -> usize;
    /// Subscribes to `topic`.
    fn subscribe(&self, topic: &str) -> broadcast::Receiver<Value>;
}

/// Who is on a topic.
pub trait Presence: Send + Sync {
    /// `member` joins `topic` with `meta`.
    fn join(&self, topic: &str, member: &str, meta: Value);
    /// `member` is still there.
    fn heartbeat(&self, topic: &str, member: &str);
    /// `member` leaves.
    fn leave(&self, topic: &str, member: &str);
    /// The members present now, with their metadata.
    fn members(&self, topic: &str) -> BTreeMap<String, Value>;
}

/// Each topic's members: their metadata and when they were last seen.
type Members = HashMap<String, BTreeMap<String, (Value, Instant)>>;

/// An in-process hub implementing [`Channel`] and [`Presence`].
#[derive(Clone)]
pub struct LocalHub {
    topics: Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>,
    present: Arc<Mutex<Members>>,
    timeout: Duration,
}

impl LocalHub {
    /// A hub dropping members `timeout` after their last heartbeat.
    #[must_use]
    pub fn new(timeout: Duration) -> Self {
        Self { topics: Arc::default(), present: Arc::default(), timeout }
    }

    fn sender(&self, topic: &str) -> broadcast::Sender<Value> {
        self.topics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(topic.to_owned())
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }

    fn announce(&self, topic: &str) {
        let members = self.members(topic);
        let _ = self
            .sender(&format!("presence:{topic}"))
            .send(serde_json::to_value(members).unwrap_or(Value::Null));
    }
}

impl Channel for LocalHub {
    fn publish(&self, topic: &str, message: Value) -> usize {
        self.sender(topic).send(message).unwrap_or(0)
    }

    fn subscribe(&self, topic: &str) -> broadcast::Receiver<Value> {
        self.sender(topic).subscribe()
    }
}

impl Presence for LocalHub {
    fn join(&self, topic: &str, member: &str, meta: Value) {
        self.present
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(topic.to_owned())
            .or_default()
            .insert(member.to_owned(), (meta, Instant::now()));
        self.announce(topic);
    }

    fn heartbeat(&self, topic: &str, member: &str) {
        if let Some(entry) = self
            .present
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(topic)
            .and_then(|members| members.get_mut(member))
        {
            entry.1 = Instant::now();
        }
    }

    fn leave(&self, topic: &str, member: &str) {
        if let Some(members) =
            self.present.lock().unwrap_or_else(PoisonError::into_inner).get_mut(topic)
        {
            members.remove(member);
        }
        self.announce(topic);
    }

    fn members(&self, topic: &str) -> BTreeMap<String, Value> {
        let mut present = self.present.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(members) = present.get_mut(topic) else { return BTreeMap::new() };
        let timeout = self.timeout;
        members.retain(|_, (_, seen)| seen.elapsed() < timeout);
        members.iter().map(|(member, (meta, _))| (member.clone(), meta.clone())).collect()
    }
}
