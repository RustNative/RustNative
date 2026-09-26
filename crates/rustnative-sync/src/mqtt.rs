//! MQTT 3.1.1 (OASIS standard): the packet codec, a broker serving
//! [`crate::bus::Broker`] over TCP, and a client. Enough of the protocol
//! for devices: connect with a will and a persistent session, publish and
//! subscribe at all three `QoS` levels (with acknowledgement and
//! redelivery), retained messages, keep-alive, and disconnect.

use std::collections::BTreeMap;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::bus::{Broker, BusMessage, Connect, QoS};

/// A control packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// CONNECT.
    Connect {
        /// The client's id.
        client_id: String,
        /// Clean session.
        clean_session: bool,
        /// Keep-alive, seconds.
        keep_alive: u16,
        /// The will.
        will: Option<BusMessage>,
    },
    /// CONNACK.
    ConnAck {
        /// Whether a session was resumed.
        session_present: bool,
        /// 0 is accepted.
        code: u8,
    },
    /// PUBLISH.
    Publish {
        /// The message.
        message: BusMessage,
        /// The packet id (`QoS` 1 and 2).
        id: Option<u16>,
        /// A redelivery.
        duplicate: bool,
    },
    /// `PUBACK` (`QoS` 1).
    PubAck(u16),
    /// `PUBREC` (`QoS` 2, step 1).
    PubRec(u16),
    /// `PUBREL` (`QoS` 2, step 2).
    PubRel(u16),
    /// `PUBCOMP` (`QoS` 2, step 3).
    PubComp(u16),
    /// SUBSCRIBE.
    Subscribe {
        /// The packet id.
        id: u16,
        /// Filters and their maximum `QoS`.
        filters: Vec<(String, QoS)>,
    },
    /// SUBACK.
    SubAck {
        /// The packet id.
        id: u16,
        /// The granted `QoS` per filter.
        granted: Vec<u8>,
    },
    /// PINGREQ.
    PingReq,
    /// PINGRESP.
    PingResp,
    /// DISCONNECT.
    Disconnect,
}

fn string(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    out.extend_from_slice(&u16::try_from(bytes.len()).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(&bytes[..bytes.len().min(usize::from(u16::MAX))]);
}

fn remaining_length(out: &mut Vec<u8>, mut length: usize) {
    loop {
        let mut byte = u8::try_from(length % 128).unwrap_or(0);
        length /= 128;
        if length > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if length == 0 {
            break;
        }
    }
}

impl Packet {
    /// The packet's bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let (header, body) = match self {
            Self::Connect { client_id, clean_session, keep_alive, will } => {
                let mut body = Vec::new();
                string(&mut body, "MQTT");
                body.push(4);
                let mut flags = 0u8;
                if *clean_session {
                    flags |= 0x02;
                }
                if let Some(will) = will {
                    flags |= 0x04 | (will.qos.level() << 3);
                    if will.retain {
                        flags |= 0x20;
                    }
                }
                body.push(flags);
                body.extend_from_slice(&keep_alive.to_be_bytes());
                string(&mut body, client_id);
                if let Some(will) = will {
                    string(&mut body, &will.topic);
                    body.extend_from_slice(
                        &u16::try_from(will.payload.len()).unwrap_or(0).to_be_bytes(),
                    );
                    body.extend_from_slice(&will.payload);
                }
                (0x10, body)
            }
            Self::ConnAck { session_present, code } => {
                (0x20, vec![u8::from(*session_present), *code])
            }
            Self::Publish { message, id, duplicate } => {
                let mut header = 0x30 | (message.qos.level() << 1);
                if message.retain {
                    header |= 0x01;
                }
                if *duplicate {
                    header |= 0x08;
                }
                let mut body = Vec::new();
                string(&mut body, &message.topic);
                if let Some(id) = id {
                    body.extend_from_slice(&id.to_be_bytes());
                }
                body.extend_from_slice(&message.payload);
                (header, body)
            }
            Self::PubAck(id) => (0x40, id.to_be_bytes().to_vec()),
            Self::PubRec(id) => (0x50, id.to_be_bytes().to_vec()),
            Self::PubRel(id) => (0x62, id.to_be_bytes().to_vec()),
            Self::PubComp(id) => (0x70, id.to_be_bytes().to_vec()),
            Self::Subscribe { id, filters } => {
                let mut body = id.to_be_bytes().to_vec();
                for (filter, qos) in filters {
                    string(&mut body, filter);
                    body.push(qos.level());
                }
                (0x82, body)
            }
            Self::SubAck { id, granted } => {
                let mut body = id.to_be_bytes().to_vec();
                body.extend_from_slice(granted);
                (0x90, body)
            }
            Self::PingReq => (0xC0, Vec::new()),
            Self::PingResp => (0xD0, Vec::new()),
            Self::Disconnect => (0xE0, Vec::new()),
        };
        let mut out = vec![header];
        remaining_length(&mut out, body.len());
        out.extend(body);
        out
    }

    /// Decodes a packet from its fixed-header byte and body.
    ///
    /// # Errors
    ///
    /// A malformed packet.
    pub fn decode(header: u8, body: &[u8]) -> Result<Self, String> {
        struct Cursor<'a> {
            body: &'a [u8],
            at: usize,
        }
        impl<'a> Cursor<'a> {
            fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
                let slice = self.body.get(self.at..self.at + count).ok_or("truncated packet")?;
                self.at += count;
                Ok(slice)
            }
            fn rest(&self) -> &'a [u8] {
                &self.body[self.at.min(self.body.len())..]
            }
        }
        fn u16_of(bytes: &[u8]) -> u16 {
            u16::from_be_bytes([bytes[0], bytes[1]])
        }
        let mut cursor = Cursor { body, at: 0 };
        let kind = header >> 4;
        Ok(match kind {
            1 => {
                let name_length = usize::from(u16_of(cursor.take(2)?));
                let _name = cursor.take(name_length)?;
                let _level = cursor.take(1)?[0];
                let flags = cursor.take(1)?[0];
                let keep_alive = u16_of(cursor.take(2)?);
                let id_length = usize::from(u16_of(cursor.take(2)?));
                let client_id = String::from_utf8_lossy(cursor.take(id_length)?).into_owned();
                let will = if flags & 0x04 != 0 {
                    let topic_length = usize::from(u16_of(cursor.take(2)?));
                    let topic = String::from_utf8_lossy(cursor.take(topic_length)?).into_owned();
                    let payload_length = usize::from(u16_of(cursor.take(2)?));
                    let payload = cursor.take(payload_length)?.to_vec();
                    Some(BusMessage {
                        topic,
                        payload,
                        qos: QoS::from_level((flags >> 3) & 0x03).ok_or("will QoS")?,
                        retain: flags & 0x20 != 0,
                    })
                } else {
                    None
                };
                Self::Connect { client_id, clean_session: flags & 0x02 != 0, keep_alive, will }
            }
            2 => {
                let bytes = cursor.take(2)?;
                Self::ConnAck { session_present: bytes[0] & 1 != 0, code: bytes[1] }
            }
            3 => {
                let qos = QoS::from_level((header >> 1) & 0x03).ok_or("QoS")?;
                let topic_length = usize::from(u16_of(cursor.take(2)?));
                let topic = String::from_utf8_lossy(cursor.take(topic_length)?).into_owned();
                let id = if qos == QoS::AtMostOnce { None } else { Some(u16_of(cursor.take(2)?)) };
                let payload = cursor.rest().to_vec();
                Self::Publish {
                    message: BusMessage { topic, payload, qos, retain: header & 1 != 0 },
                    id,
                    duplicate: header & 0x08 != 0,
                }
            }
            4 => Self::PubAck(u16_of(cursor.take(2)?)),
            5 => Self::PubRec(u16_of(cursor.take(2)?)),
            6 => Self::PubRel(u16_of(cursor.take(2)?)),
            7 => Self::PubComp(u16_of(cursor.take(2)?)),
            8 => {
                let id = u16_of(cursor.take(2)?);
                let mut filters = Vec::new();
                while cursor.at < body.len() {
                    let length = usize::from(u16_of(cursor.take(2)?));
                    let filter = String::from_utf8_lossy(cursor.take(length)?).into_owned();
                    let qos = QoS::from_level(cursor.take(1)?[0] & 0x03).ok_or("QoS")?;
                    filters.push((filter, qos));
                }
                Self::Subscribe { id, filters }
            }
            9 => {
                let id = u16_of(cursor.take(2)?);
                Self::SubAck { id, granted: cursor.rest().to_vec() }
            }
            12 => Self::PingReq,
            13 => Self::PingResp,
            14 => Self::Disconnect,
            other => return Err(format!("unsupported packet type {other}")),
        })
    }

    /// Reads one packet.
    ///
    /// # Errors
    ///
    /// The stream ended or the packet was malformed.
    pub async fn read(stream: &mut (impl AsyncReadExt + Unpin)) -> Result<Self, String> {
        let header = stream.read_u8().await.map_err(|error| error.to_string())?;
        let mut length = 0usize;
        let mut shift = 0;
        loop {
            let byte = stream.read_u8().await.map_err(|error| error.to_string())?;
            length |= usize::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 21 {
                return Err("remaining length too long".into());
            }
        }
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body).await.map_err(|error| error.to_string())?;
        Self::decode(header, &body)
    }
}

/// Serves `broker` over MQTT on `listener`.
pub async fn serve(broker: Broker, listener: tokio::net::TcpListener) {
    while let Ok((stream, _)) = listener.accept().await {
        let broker = broker.clone();
        tokio::spawn(async move { connection(broker, stream).await });
    }
}

async fn connection(broker: Broker, stream: TcpStream) {
    let (mut reader, mut writer) = stream.into_split();
    let Ok(Packet::Connect { client_id, clean_session, keep_alive, will }) =
        Packet::read(&mut reader).await
    else {
        return;
    };
    let mut deliveries =
        broker.connect(Connect { client_id: client_id.clone(), clean_session, will });
    if writer
        .write_all(&Packet::ConnAck { session_present: !clean_session, code: 0 }.encode())
        .await
        .is_err()
    {
        broker.lost(&client_id);
        return;
    }
    let timeout = if keep_alive == 0 {
        Duration::from_secs(86_400)
    } else {
        Duration::from_secs(u64::from(keep_alive)) * 3 / 2
    };
    let mut next_id: u16 = 0;
    let mut inflight: BTreeMap<u16, BusMessage> = BTreeMap::new();
    let mut graceful = false;
    loop {
        tokio::select! {
            delivery = deliveries.recv() => {
                let Some(message) = delivery else { break };
                let id = if message.qos == QoS::AtMostOnce {
                    None
                } else {
                    next_id = next_id.wrapping_add(1).max(1);
                    inflight.insert(next_id, message.clone());
                    Some(next_id)
                };
                if writer.write_all(&Packet::Publish { message, id, duplicate: false }.encode()).await.is_err() {
                    break;
                }
            }
            packet = tokio::time::timeout(timeout, Packet::read(&mut reader)) => {
                let Ok(Ok(packet)) = packet else { break };
                let reply = match packet {
                    Packet::Publish { message, id, .. } => match (message.qos, id) {
                        (QoS::AtMostOnce, _) => { broker.publish(&message, None); None }
                        (QoS::AtLeastOnce, Some(id)) => { broker.publish(&message, None); Some(Packet::PubAck(id)) }
                        (QoS::ExactlyOnce, Some(id)) => { broker.publish(&message, Some((&client_id, id))); Some(Packet::PubRec(id)) }
                        _ => None,
                    },
                    Packet::PubRel(id) => Some(Packet::PubComp(id)),
                    Packet::PubAck(id) | Packet::PubComp(id) => { inflight.remove(&id); None }
                    Packet::PubRec(id) => Some(Packet::PubRel(id)),
                    Packet::Subscribe { id, filters } => {
                        let granted = filters.iter().map(|(_, qos)| qos.level()).collect();
                        for (filter, qos) in filters {
                            broker.subscribe(&client_id, &filter, qos);
                        }
                        Some(Packet::SubAck { id, granted })
                    }
                    Packet::PingReq => Some(Packet::PingResp),
                    Packet::Disconnect => { graceful = true; break }
                    _ => None,
                };
                if let Some(reply) = reply {
                    if writer.write_all(&reply.encode()).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
    // Unacknowledged deliveries wait for a persistent session's return.
    broker.requeue(&client_id, inflight.into_values().collect());
    if graceful {
        broker.disconnect(&client_id);
    } else {
        broker.lost(&client_id);
    }
}

/// An MQTT client.
pub struct MqttClient {
    writer: std::sync::Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    /// What arrives (acknowledged for you).
    pub incoming: mpsc::UnboundedReceiver<BusMessage>,
    next_id: u16,
    reader: tokio::task::JoinHandle<()>,
}

impl MqttClient {
    /// Connects to the broker at `address`.
    ///
    /// # Errors
    ///
    /// The connection or handshake failed.
    pub async fn connect(address: &str, connect: Connect) -> Result<Self, String> {
        let stream = TcpStream::connect(address).await.map_err(|error| error.to_string())?;
        let (mut reader, mut writer) = stream.into_split();
        let packet = Packet::Connect {
            client_id: connect.client_id,
            clean_session: connect.clean_session,
            keep_alive: 30,
            will: connect.will,
        };
        writer.write_all(&packet.encode()).await.map_err(|error| error.to_string())?;
        match Packet::read(&mut reader).await? {
            Packet::ConnAck { code: 0, .. } => {}
            other => return Err(format!("refused: {other:?}")),
        }
        let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer));
        let (deliver, incoming) = mpsc::unbounded_channel();
        let responder = std::sync::Arc::clone(&writer);
        let reader = tokio::spawn(async move {
            let mut seen_exactly_once = std::collections::BTreeSet::new();
            while let Ok(packet) = Packet::read(&mut reader).await {
                let reply = match packet {
                    Packet::Publish { message, id, .. } => match (message.qos, id) {
                        (QoS::AtLeastOnce, Some(id)) => {
                            let _ = deliver.send(message);
                            Some(Packet::PubAck(id))
                        }
                        (QoS::ExactlyOnce, Some(id)) => {
                            if seen_exactly_once.insert(id) {
                                let _ = deliver.send(message);
                            }
                            Some(Packet::PubRec(id))
                        }
                        _ => {
                            let _ = deliver.send(message);
                            None
                        }
                    },
                    Packet::PubRel(id) => {
                        seen_exactly_once.remove(&id);
                        Some(Packet::PubComp(id))
                    }
                    Packet::PubRec(id) => Some(Packet::PubRel(id)),
                    _ => None,
                };
                if let Some(reply) = reply {
                    if responder.lock().await.write_all(&reply.encode()).await.is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Self { writer, incoming, next_id: 0, reader })
    }

    fn id(&mut self) -> u16 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    async fn write(&self, packet: &Packet) -> Result<(), String> {
        self.writer
            .lock()
            .await
            .write_all(&packet.encode())
            .await
            .map_err(|error| error.to_string())
    }

    /// Subscribes.
    ///
    /// # Errors
    ///
    /// The connection failed.
    pub async fn subscribe(&mut self, filter: &str, qos: QoS) -> Result<(), String> {
        let id = self.id();
        self.write(&Packet::Subscribe { id, filters: vec![(filter.to_owned(), qos)] }).await
    }

    /// Publishes.
    ///
    /// # Errors
    ///
    /// The connection failed.
    pub async fn publish(&mut self, message: BusMessage) -> Result<(), String> {
        let id = (message.qos != QoS::AtMostOnce).then(|| self.id());
        self.write(&Packet::Publish { message, id, duplicate: false }).await
    }

    /// Says goodbye (no will).
    pub async fn disconnect(self) {
        let _ = self.write(&Packet::Disconnect).await;
        self.reader.abort();
    }

    /// Drops the connection without a word (the will is published).
    pub fn vanish(self) {
        self.reader.abort();
    }
}
