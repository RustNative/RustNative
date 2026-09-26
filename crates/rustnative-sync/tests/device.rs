//! Device desired state and messaging (`PLAN.md` Milestone 55): a device
//! converges to its desired configuration after a period offline; the bus
//! keeps its stated delivery guarantees, retained values, last will, and
//! persistent sessions — in process and over MQTT 3.1.1.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::collections::BTreeMap;
use std::time::Duration;

use rustnative_sync::bus::{Broker, BusMessage, Connect, LocalBus, QoS, topic_matches};
use rustnative_sync::device::{
    Actuate, DeviceAgent, DeviceModel, DevicePolicy, Twin, decode, encode,
};
use rustnative_sync::mqtt::{MqttClient, Packet, serve};
use rustnative_sync::{Clock, Hlc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Thermostat {
    target: i32,
}

struct Heater {
    applied: Vec<i32>,
}

impl Actuate<Thermostat> for Heater {
    fn apply(&mut self, desired: &Thermostat) -> Result<Thermostat, String> {
        // The hardware clamps to what it can do.
        let target = desired.target.clamp(5, 28);
        self.applied.push(target);
        Ok(Thermostat { target })
    }
}

impl DeviceModel for Thermostat {
    fn to_resources(&self) -> BTreeMap<String, Value> {
        // IPSO 3308 (set point), instance 0, resource 5900 (set point value).
        BTreeMap::from([("3308/0/5900".to_owned(), json!(self.target))])
    }
    fn from_resources(resources: &BTreeMap<String, Value>) -> Result<Self, String> {
        let target = resources.get("3308/0/5900").and_then(Value::as_i64).ok_or("3308/0/5900")?;
        Ok(Self { target: i32::try_from(target).map_err(|error| error.to_string())? })
    }
}

fn topic(message: &str) -> String {
    format!("devices/thermostat-1/{message}")
}

#[test]
fn a_device_converges_after_a_period_offline() {
    let cloud = Clock::new(1);
    let device = Clock::new(2);
    let broker = Broker::new();
    let mut twin = Twin::new(Thermostat { target: 20 }, cloud.now());
    let mut agent = DeviceAgent::new(Thermostat { target: 16 }, device.now());
    let mut heater = Heater { applied: Vec::new() };

    // The device's persistent session outlives its connection.
    let session = |broker: &Broker| {
        let bus = LocalBus::connect(
            broker,
            Connect {
                client_id: "thermostat-1".into(),
                clean_session: false,
                will: Some(BusMessage {
                    topic: topic("status"),
                    payload: b"offline".to_vec(),
                    qos: QoS::AtLeastOnce,
                    retain: true,
                }),
            },
        );
        bus.subscribe(&topic("desired"), QoS::AtLeastOnce);
        bus
    };
    let publish_desire = |twin: &Twin<Thermostat>| {
        broker.publish(
            &BusMessage {
                topic: topic("desired"),
                payload: encode(&twin.desired).unwrap(),
                qos: QoS::AtLeastOnce,
                retain: true,
            },
            None,
        );
    };
    let bus = session(&broker);
    publish_desire(&twin);

    let reconcile = |bus: &mut LocalBus,
                     agent: &mut DeviceAgent<Thermostat>,
                     heater: &mut Heater,
                     twin: &mut Twin<Thermostat>| {
        let mut newest = None;
        while let Ok(message) = bus.incoming.try_recv() {
            newest = Some(decode::<Thermostat>(&message.payload).unwrap());
        }
        if let Some(desired) = newest {
            agent.reconcile(&desired, DevicePolicy::DesiredWins, heater, device.now()).unwrap();
        }
        if let Some(report) = agent.take_reports() {
            twin.report(report);
        }
    };
    let mut bus = bus;
    reconcile(&mut bus, &mut agent, &mut heater, &mut twin);
    assert!(twin.converged());

    // The device drops off the network; the cloud's will says so.
    let observer = LocalBus::connect(
        &broker,
        Connect { client_id: "dashboard".into(), clean_session: true, will: None },
    );
    bus.vanish();
    observer.subscribe(&topic("status"), QoS::AtLeastOnce);
    let mut observer = observer;
    assert_eq!(observer.incoming.try_recv().unwrap().payload, b"offline", "the retained will");

    // Three changes while it is away; the device needs only the last.
    for target in [18, 22, 35] {
        twin.desire(Thermostat { target }, cloud.now());
        publish_desire(&twin);
    }
    assert!(!twin.converged());

    let mut bus = session(&broker);
    reconcile(&mut bus, &mut agent, &mut heater, &mut twin);
    assert_eq!(heater.applied, [20, 28], "one application per convergence, not per missed desire");
    assert_eq!(agent.current().value, Thermostat { target: 28 }, "clamped by the hardware");
    assert_eq!(twin.reported.as_ref().unwrap().version, twin.desired.version);
    assert_eq!(
        Thermostat::from_resources(&agent.current().value.to_resources()).unwrap(),
        agent.current().value
    );
}

#[test]
fn a_local_change_is_settled_by_the_policy() {
    let clock = Clock::new(7);
    let mut heater = Heater { applied: Vec::new() };
    let mut agent = DeviceAgent::new(Thermostat { target: 20 }, clock.now());
    let desire = |version, stamp: Hlc| rustnative_sync::device::Versioned {
        value: Thermostat { target: 25 },
        version,
        stamp,
    };
    let earlier = clock.now();
    agent.local_change(Thermostat { target: 18 }, clock.now());
    agent
        .reconcile(&desire(1, earlier), DevicePolicy::LastWriterWins, &mut heater, clock.now())
        .unwrap();
    assert_eq!(agent.current().value.target, 18, "the knob was turned after the desire was made");
    agent
        .reconcile(&desire(2, clock.now()), DevicePolicy::DesiredWins, &mut heater, clock.now())
        .unwrap();
    assert_eq!(agent.current().value.target, 25);
}

#[test]
fn topics_match_with_wildcards() {
    assert!(topic_matches("devices/+/desired", "devices/t-1/desired"));
    assert!(topic_matches("devices/#", "devices/t-1/status/battery"));
    assert!(!topic_matches("devices/+/desired", "devices/t-1/reported"));
}

#[test]
fn packets_round_trip() {
    let will = BusMessage {
        topic: "a/b".into(),
        payload: b"bye".to_vec(),
        qos: QoS::AtLeastOnce,
        retain: true,
    };
    for packet in [
        Packet::Connect {
            client_id: "c".into(),
            clean_session: false,
            keep_alive: 30,
            will: Some(will),
        },
        Packet::ConnAck { session_present: true, code: 0 },
        Packet::Publish {
            message: BusMessage {
                topic: "t".into(),
                payload: vec![1, 2],
                qos: QoS::ExactlyOnce,
                retain: false,
            },
            id: Some(7),
            duplicate: false,
        },
        Packet::Subscribe { id: 3, filters: vec![("x/#".into(), QoS::AtLeastOnce)] },
        Packet::SubAck { id: 3, granted: vec![1] },
        Packet::PubRel(9),
        Packet::PingReq,
        Packet::Disconnect,
    ] {
        let bytes = packet.encode();
        let body_start = 1 + bytes[1..].iter().position(|byte| byte & 0x80 == 0).unwrap() + 1;
        assert_eq!(Packet::decode(bytes[0], &bytes[body_start..]).unwrap(), packet);
    }
}

#[tokio::test]
async fn mqtt_keeps_its_guarantees_over_tcp() {
    let broker = Broker::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(serve(broker, listener));
    let connect = |id: &str, clean: bool, will: Option<BusMessage>| Connect {
        client_id: id.into(),
        clean_session: clean,
        will,
    };

    // A persistent subscriber.
    let mut device = MqttClient::connect(&address, connect("device", false, None)).await.unwrap();
    device.subscribe("config/#", QoS::AtLeastOnce).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    device.disconnect().await;

    // While it is away: an at-least-once message is queued for it, an
    // at-most-once one is not; a retained value is kept.
    let mut cloud = MqttClient::connect(&address, connect("cloud", true, None)).await.unwrap();
    cloud
        .publish(BusMessage {
            topic: "config/rate".into(),
            payload: b"10".to_vec(),
            qos: QoS::AtLeastOnce,
            retain: false,
        })
        .await
        .unwrap();
    cloud
        .publish(BusMessage {
            topic: "config/noise".into(),
            payload: b"x".to_vec(),
            qos: QoS::AtMostOnce,
            retain: false,
        })
        .await
        .unwrap();
    cloud
        .publish(BusMessage {
            topic: "status/fw".into(),
            payload: b"1.2".to_vec(),
            qos: QoS::AtLeastOnce,
            retain: true,
        })
        .await
        .unwrap();
    // Exactly once, even when the packet is sent twice.
    for _ in 0..1 {
        cloud
            .publish(BusMessage {
                topic: "config/mode".into(),
                payload: b"eco".to_vec(),
                qos: QoS::ExactlyOnce,
                retain: false,
            })
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut device = MqttClient::connect(&address, connect("device", false, None)).await.unwrap();
    let mut received = Vec::new();
    while let Ok(Some(message)) =
        tokio::time::timeout(Duration::from_millis(200), device.incoming.recv()).await
    {
        received.push(String::from_utf8(message.payload).unwrap());
    }
    assert_eq!(
        received,
        ["10", "eco"],
        "queued at-least-once and exactly-once; the at-most-once one dropped"
    );

    // The last will, when a client vanishes.
    let mut watcher = MqttClient::connect(&address, connect("watcher", true, None)).await.unwrap();
    watcher.subscribe("status/#", QoS::AtLeastOnce).await.unwrap();
    let retained = tokio::time::timeout(Duration::from_secs(1), watcher.incoming.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.payload, b"1.2", "the retained value, at subscription");
    let will = BusMessage {
        topic: "status/sensor".into(),
        payload: b"gone".to_vec(),
        qos: QoS::AtLeastOnce,
        retain: false,
    };
    let sensor = MqttClient::connect(&address, connect("sensor", true, Some(will))).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    sensor.vanish();
    let message = tokio::time::timeout(Duration::from_secs(2), watcher.incoming.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.payload, b"gone");
    cloud.disconnect().await;
}
