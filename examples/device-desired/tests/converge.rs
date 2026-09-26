//! A device converges on its desired configuration after a period offline
//! (`PLAN.md` Milestone 55's third "done when"), over MQTT on a socket.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::time::Duration;

use device_desired::{Cloud, Config, Hardware, device_session};
use rustnative_sync::Clock;
use rustnative_sync::bus::Broker;
use rustnative_sync::device::DeviceAgent;

fn config(interval: u32, channel: &str) -> Config {
    Config { interval, channel: channel.into() }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_device_converges_after_being_offline() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(rustnative_sync::mqtt::serve(Broker::new(), listener));

    let clock = Clock::new(1);
    let mut agent = DeviceAgent::new(config(60, "stable"), clock.now());
    let mut hardware = Hardware::default();
    let mut cloud = Cloud::connect(&address, "sensor-7", config(30, "stable")).await.unwrap();

    device_session(
        &address,
        "sensor-7",
        &mut agent,
        &mut hardware,
        &clock,
        Duration::from_millis(300),
    )
    .await
    .unwrap();
    cloud.collect(Duration::from_millis(200)).await;
    assert!(cloud.twin.converged(), "online: converged at once");

    // The device is offline while the cloud changes its mind three times.
    cloud.desire(config(10, "beta")).await.unwrap();
    cloud.desire(config(1, "beta")).await.unwrap();
    cloud.desire(config(2, "stable")).await.unwrap();
    assert!(!cloud.twin.converged());

    device_session(
        &address,
        "sensor-7",
        &mut agent,
        &mut hardware,
        &clock,
        Duration::from_millis(400),
    )
    .await
    .unwrap();
    cloud.collect(Duration::from_millis(300)).await;
    assert_eq!(
        agent.current().value,
        config(5, "stable"),
        "the newest desire, clamped by the hardware"
    );
    let reported = cloud.twin.reported.clone().unwrap();
    assert_eq!(
        reported.version, cloud.twin.desired.version,
        "the device reports the version it reached"
    );
    assert_eq!(
        hardware.applied.len(),
        2,
        "catching up applies the newest desire, not every missed one: {:?}",
        hardware.applied
    );
}
