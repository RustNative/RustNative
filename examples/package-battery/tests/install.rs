//! The sample package installs into an application's services without
//! forking the framework, only on the backends and framework versions it
//! declares, and its `Cargo.toml` metadata says what its code does.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use package_battery::{BatteryPackage, BatteryService};
use rustnative_core::Services;
use rustnative_core::package::{CapabilityPackage, PackageError};

#[test]
fn the_package_installs_only_where_it_has_code() {
    #[cfg(windows)]
    {
        let services = Services::default().install(&BatteryPackage, "windows").unwrap();
        let battery = services.package::<Box<dyn BatteryService>>("package-battery").unwrap();
        let status = battery.status();
        assert!(status.percent.is_none_or(|percent| percent <= 100), "{status:?}");
    }
    let refused = Services::default().install(&BatteryPackage, "android").unwrap_err();
    assert!(matches!(refused, PackageError::Backend { .. }), "{refused}");
    assert!(
        Services::default().package::<Box<dyn BatteryService>>("package-battery").is_none(),
        "nothing global"
    );
}

#[test]
fn the_cargo_metadata_matches_the_manifest() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let cargo: toml::Value = toml::from_str(&text).unwrap();
    let metadata = &cargo["package"]["metadata"]["rustnative"];
    let manifest = BatteryPackage.manifest();
    let backends: Vec<&str> = metadata["backends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|backend| backend.as_str().unwrap())
        .collect();
    assert_eq!(backends, manifest.backends);
    assert_eq!(metadata["framework"].as_str(), Some(manifest.framework.as_str()));
}
