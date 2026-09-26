//! A device converging on its desired configuration (`PLAN.md` Milestone
//! 55's third "done when"), over MQTT 3.1.1.
//!
//! - The cloud keeps a [`Twin`] and publishes its desired state, retained,
//!   on `devices/<id>/desired`.
//! - The device keeps a persistent session, so desires published while it
//!   is offline wait for it; it applies the newest (once) and reports on
//!   `devices/<id>/reported`. Its last will marks it offline.

use std::time::Duration;

use rustnative_sync::Clock;
use rustnative_sync::bus::{BusMessage, Connect, QoS};
use rustnative_sync::device::{
    Actuate, DeviceAgent, DevicePolicy, Twin, Versioned, decode, encode,
};
use rustnative_sync::mqtt::MqttClient;
use serde::{Deserialize, Serialize};

/// The configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Reporting interval, seconds.
    pub interval: u32,
    /// Firmware channel.
    pub channel: String,
}

/// The device's hardware.
#[derive(Debug, Default)]
pub struct Hardware {
    /// Every configuration it was told to apply.
    pub applied: Vec<Config>,
}

impl Actuate<Config> for Hardware {
    fn apply(&mut self, desired: &Config) -> Result<Config, String> {
        // The radio cannot report faster than every 5 seconds.
        let actual = Config { interval: desired.interval.max(5), channel: desired.channel.clone() };
        self.applied.push(actual.clone());
        Ok(actual)
    }
}

fn topic(device: &str, kind: &str) -> String {
    format!("devices/{device}/{kind}")
}

/// The device side: connect (persistently), take every desire waiting,
/// apply the newest, report, and stay connected for `stay`.
///
/// # Errors
///
/// The broker is unreachable.
pub async fn device_session(
    address: &str,
    id: &str,
    agent: &mut DeviceAgent<Config>,
    hardware: &mut Hardware,
    clock: &Clock,
    stay: Duration,
) -> Result<(), String> {
    let will = BusMessage {
        topic: topic(id, "status"),
        payload: b"offline".to_vec(),
        qos: QoS::AtLeastOnce,
        retain: true,
    };
    let mut client = MqttClient::connect(
        address,
        Connect { client_id: id.into(), clean_session: false, will: Some(will) },
    )
    .await?;
    client.subscribe(&topic(id, "desired"), QoS::AtLeastOnce).await?;
    client
        .publish(BusMessage {
            topic: topic(id, "status"),
            payload: b"online".to_vec(),
            qos: QoS::AtLeastOnce,
            retain: true,
        })
        .await?;
    let deadline = tokio::time::Instant::now() + stay;
    let mut newest: Option<Versioned<Config>> = None;
    while let Ok(Some(message)) = tokio::time::timeout_at(deadline, client.incoming.recv()).await {
        // What queued while the device was away arrives as a burst: take
        // all of it, then apply only the newest desire.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut burst = vec![message];
        while let Ok(more) = client.incoming.try_recv() {
            burst.push(more);
        }
        for message in burst {
            if let Ok(desired) = decode::<Config>(&message.payload) {
                if newest.as_ref().is_none_or(|current| desired.version > current.version) {
                    newest = Some(desired);
                }
            }
        }
        if let Some(desired) = newest.take() {
            agent.reconcile(&desired, DevicePolicy::DesiredWins, hardware, clock.now())?;
            if let Some(report) = agent.take_reports() {
                client
                    .publish(BusMessage {
                        topic: topic(id, "reported"),
                        payload: encode(&report)?,
                        qos: QoS::AtLeastOnce,
                        retain: true,
                    })
                    .await?;
            }
        }
    }
    client.vanish();
    Ok(())
}

/// The cloud side: publishes desires and collects reports into the twin.
pub struct Cloud {
    client: MqttClient,
    clock: Clock,
    device: String,
    /// The twin.
    pub twin: Twin<Config>,
}

impl Cloud {
    /// Connects, desiring `initial` for `device`.
    ///
    /// # Errors
    ///
    /// The broker is unreachable.
    pub async fn connect(address: &str, device: &str, initial: Config) -> Result<Self, String> {
        let mut client = MqttClient::connect(
            address,
            Connect { client_id: "cloud".into(), clean_session: true, will: None },
        )
        .await?;
        client.subscribe(&topic(device, "reported"), QoS::AtLeastOnce).await?;
        let clock = Clock::new(100);
        let twin = Twin::new(initial, clock.now());
        let mut cloud = Self { client, clock, device: device.into(), twin };
        cloud.publish().await?;
        Ok(cloud)
    }

    async fn publish(&mut self) -> Result<(), String> {
        let message = BusMessage {
            topic: topic(&self.device, "desired"),
            payload: encode(&self.twin.desired)?,
            qos: QoS::AtLeastOnce,
            retain: true,
        };
        self.client.publish(message).await
    }

    /// Declares a new desired configuration.
    ///
    /// # Errors
    ///
    /// The broker is unreachable.
    pub async fn desire(&mut self, config: Config) -> Result<(), String> {
        self.twin.desire(config, self.clock.now());
        self.publish().await
    }

    /// Takes the reports that arrived within `wait`.
    pub async fn collect(&mut self, wait: Duration) {
        while let Ok(Some(message)) = tokio::time::timeout(wait, self.client.incoming.recv()).await
        {
            if let Ok(report) = decode::<Config>(&message.payload) {
                self.twin.report(report);
            }
        }
    }
}
