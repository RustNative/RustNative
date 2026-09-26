//! The device example, end to end on one machine: an MQTT broker, the
//! cloud, and a device that goes offline while its desired configuration
//! changes, then converges.

use std::time::Duration;

use device_desired::{Cloud, Config, Hardware, device_session};
use rustnative_sync::Clock;
use rustnative_sync::bus::Broker;
use rustnative_sync::device::DeviceAgent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:1883").await?;
    let address = listener.local_addr()?.to_string();
    tokio::spawn(rustnative_sync::mqtt::serve(Broker::new(), listener));
    println!("broker on mqtt://{address}");

    let clock = Clock::new(1);
    let mut agent =
        DeviceAgent::new(Config { interval: 60, channel: "stable".into() }, clock.now());
    let mut hardware = Hardware::default();
    let mut cloud =
        Cloud::connect(&address, "sensor-7", Config { interval: 30, channel: "stable".into() })
            .await?;

    device_session(
        &address,
        "sensor-7",
        &mut agent,
        &mut hardware,
        &clock,
        Duration::from_millis(500),
    )
    .await?;
    cloud.collect(Duration::from_millis(300)).await;
    println!("device online: converged = {}", cloud.twin.converged());

    println!("device offline; the cloud asks for a 1-second interval on the beta channel");
    cloud.desire(Config { interval: 1, channel: "beta".into() }).await?;
    println!("converged = {}", cloud.twin.converged());

    device_session(
        &address,
        "sensor-7",
        &mut agent,
        &mut hardware,
        &clock,
        Duration::from_millis(500),
    )
    .await?;
    cloud.collect(Duration::from_millis(300)).await;
    println!(
        "device back: reports {:?}; converged = {}",
        agent.current().value,
        cloud.twin.reported.as_ref().is_some_and(|r| r.version == cloud.twin.desired.version)
    );
    Ok(())
}
