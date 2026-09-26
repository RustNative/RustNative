//! Messaging with stated semantics: delivery guarantee per message
//! ([`QoS`]), retained values, a last will, and persistent sessions.
//!
//! [`Broker`] holds the semantics; [`LocalBus`] is a client of a broker in
//! the same process, and `mqtt` carries the same broker over MQTT 3.1.1.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::mpsc;

/// How hard a message is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QoS {
    /// Once or not at all.
    AtMostOnce,
    /// At least once; duplicates possible.
    AtLeastOnce,
    /// Exactly once.
    ExactlyOnce,
}

impl QoS {
    /// The MQTT level.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::AtMostOnce => 0,
            Self::AtLeastOnce => 1,
            Self::ExactlyOnce => 2,
        }
    }

    /// From an MQTT level.
    #[must_use]
    pub const fn from_level(level: u8) -> Option<Self> {
        match level {
            0 => Some(Self::AtMostOnce),
            1 => Some(Self::AtLeastOnce),
            2 => Some(Self::ExactlyOnce),
            _ => None,
        }
    }
}

/// A message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusMessage {
    /// Its topic.
    pub topic: String,
    /// Its payload.
    pub payload: Vec<u8>,
    /// Its delivery guarantee.
    pub qos: QoS,
    /// Whether the broker keeps it as the topic's current value.
    pub retain: bool,
}

/// What a client says when it connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connect {
    /// Its identity (the session's key).
    pub client_id: String,
    /// Whether the broker forgets its session on disconnect.
    pub clean_session: bool,
    /// Published for it if it disconnects without saying so.
    pub will: Option<BusMessage>,
}

/// Whether `filter` (with `+` and `#` wildcards) matches `topic`.
#[must_use]
pub fn topic_matches(filter: &str, topic: &str) -> bool {
    let mut filter = filter.split('/');
    let mut topic = topic.split('/');
    loop {
        match (filter.next(), topic.next()) {
            (Some("#"), _) | (None, None) => return true,
            (Some(expected), Some(actual)) if expected == "+" || expected == actual => {}
            _ => return false,
        }
    }
}

#[derive(Default)]
struct Session {
    subscriptions: BTreeMap<String, QoS>,
    queued: Vec<BusMessage>,
    online: Option<mpsc::UnboundedSender<BusMessage>>,
    will: Option<BusMessage>,
    clean: bool,
}

/// The broker: sessions, subscriptions, retained values.
#[derive(Clone, Default)]
pub struct Broker {
    inner: Arc<Mutex<BrokerState>>,
}

#[derive(Default)]
struct BrokerState {
    sessions: HashMap<String, Session>,
    retained: BTreeMap<String, BusMessage>,
    delivered_once: BTreeSet<(String, u16)>,
}

impl Broker {
    /// An empty broker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Connects a client; its deliveries arrive on the returned receiver.
    /// A persistent session gets what was queued while it was away.
    pub fn connect(&self, connect: Connect) -> mpsc::UnboundedReceiver<BusMessage> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if connect.clean_session {
            state.sessions.remove(&connect.client_id);
        }
        let session = state.sessions.entry(connect.client_id).or_default();
        session.clean = connect.clean_session;
        session.will = connect.will;
        for message in session.queued.drain(..) {
            let _ = sender.send(message);
        }
        session.online = Some(sender);
        receiver
    }

    /// Subscribes; the topic's retained values are delivered at once.
    pub fn subscribe(&self, client_id: &str, filter: &str, qos: QoS) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let retained: Vec<BusMessage> = state
            .retained
            .values()
            .filter(|message| topic_matches(filter, &message.topic))
            .cloned()
            .collect();
        if let Some(session) = state.sessions.get_mut(client_id) {
            session.subscriptions.insert(filter.to_owned(), qos);
            if let Some(online) = &session.online {
                for message in retained {
                    let _ = online.send(BusMessage { qos: message.qos.min(qos), ..message });
                }
            }
        }
    }

    /// Publishes. `once` is a sender-scoped packet id for exactly-once: a
    /// retransmission with the same id is not delivered again.
    pub fn publish(&self, message: &BusMessage, once: Option<(&str, u16)>) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let (QoS::ExactlyOnce, Some((client, packet))) = (message.qos, once) {
            if !state.delivered_once.insert((client.to_owned(), packet)) {
                return;
            }
        }
        if message.retain {
            if message.payload.is_empty() {
                state.retained.remove(&message.topic);
            } else {
                state.retained.insert(message.topic.clone(), message.clone());
            }
        }
        for session in state.sessions.values_mut() {
            let Some(qos) = session
                .subscriptions
                .iter()
                .filter(|(filter, _)| topic_matches(filter, &message.topic))
                .map(|(_, qos)| *qos)
                .max()
            else {
                continue;
            };
            let delivered =
                BusMessage { qos: message.qos.min(qos), retain: false, ..message.clone() };
            let sent = session
                .online
                .as_ref()
                .is_some_and(|online| online.send(delivered.clone()).is_ok());
            if !sent && !session.clean && delivered.qos != QoS::AtMostOnce {
                session.queued.push(delivered);
            }
        }
    }

    /// Puts deliveries a connection did not get acknowledged back in a
    /// persistent session's queue.
    pub fn requeue(&self, client_id: &str, messages: Vec<BusMessage>) {
        let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(session) = state.sessions.get_mut(client_id) {
            if !session.clean {
                session.queued.extend(messages);
            }
        }
    }

    /// The client said goodbye: no will.
    pub fn disconnect(&self, client_id: &str) {
        self.drop_client(client_id, false);
    }

    /// The client vanished: its will is published.
    pub fn lost(&self, client_id: &str) {
        self.drop_client(client_id, true);
    }

    fn drop_client(&self, client_id: &str, publish_will: bool) {
        let will = {
            let mut state = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(session) = state.sessions.get_mut(client_id) else { return };
            session.online = None;
            let will = session.will.take();
            if session.clean {
                state.sessions.remove(client_id);
            }
            will
        };
        if publish_will {
            if let Some(will) = will {
                self.publish(&will, None);
            }
        }
    }
}

/// A client of a broker in the same process.
pub struct LocalBus {
    broker: Broker,
    client_id: String,
    /// What arrives.
    pub incoming: mpsc::UnboundedReceiver<BusMessage>,
}

impl LocalBus {
    /// Connects to `broker`.
    #[must_use]
    pub fn connect(broker: &Broker, connect: Connect) -> Self {
        let client_id = connect.client_id.clone();
        let incoming = broker.connect(connect);
        Self { broker: broker.clone(), client_id, incoming }
    }

    /// Subscribes.
    pub fn subscribe(&self, filter: &str, qos: QoS) {
        self.broker.subscribe(&self.client_id, filter, qos);
    }

    /// Publishes.
    pub fn publish(&self, message: &BusMessage) {
        self.broker.publish(message, None);
    }

    /// Disconnects cleanly.
    pub fn disconnect(self) {
        self.broker.disconnect(&self.client_id);
    }

    /// Vanishes (the network went away): the will is published.
    pub fn vanish(self) {
        self.broker.lost(&self.client_id);
    }
}
