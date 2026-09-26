//! A third-party capability package (`PLAN.md` Milestone 52, `C71`),
//! written the way a community crate would be: a portable
//! [`BatteryService`] contract, code for each backend it supports, and a
//! manifest — here and in `Cargo.toml`'s `[package.metadata.rustnative]`,
//! which `rustnative add` checks before adding it.
//!
//! An application installs it without touching the framework:
//!
//! ```
//! use rustnative_core::Services;
//! use package_battery::{BatteryPackage, BatteryService};
//!
//! let services = Services::default().install(&BatteryPackage, "headless").unwrap();
//! let battery = services.package::<Box<dyn BatteryService>>("package-battery").unwrap();
//! assert!(battery.status().percent.is_none_or(|percent| percent <= 100));
//! ```

use rustnative_core::grant::ScopedServices;
use rustnative_core::package::{CapabilityPackage, PackageManifest};

/// The machine's power, as a portable answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryStatus {
    /// Running on mains power (`None`: unknown).
    pub charging: Option<bool>,
    /// Charge left, 0–100 (`None`: no battery, or unknown).
    pub percent: Option<u8>,
}

/// The portable contract the package implements per backend.
pub trait BatteryService: Send + Sync {
    /// The power status now.
    fn status(&self) -> BatteryStatus;
}

/// The package itself.
#[derive(Debug, Default, Clone, Copy)]
pub struct BatteryPackage;

impl CapabilityPackage for BatteryPackage {
    type Service = Box<dyn BatteryService>;

    fn manifest(&self) -> PackageManifest {
        PackageManifest::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            &["windows", "headless"],
            ">=0.1, <0.2",
        )
    }

    fn build(&self, _scope: ScopedServices, backend: &str) -> Option<Self::Service> {
        match backend {
            #[cfg(windows)]
            "windows" => Some(Box::new(windows::WindowsBattery)),
            "headless" => {
                Some(Box::new(Fixed(BatteryStatus { charging: Some(true), percent: Some(80) })))
            }
            _ => None,
        }
    }
}

/// A fixed answer, for the headless backend and tests.
#[derive(Debug, Clone, Copy)]
pub struct Fixed(pub BatteryStatus);

impl BatteryService for Fixed {
    fn status(&self) -> BatteryStatus {
        self.0
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    use super::{BatteryService, BatteryStatus};

    /// `GetSystemPowerStatus`.
    pub(super) struct WindowsBattery;

    impl BatteryService for WindowsBattery {
        fn status(&self) -> BatteryStatus {
            let mut power = SYSTEM_POWER_STATUS {
                ACLineStatus: 255,
                BatteryFlag: 255,
                BatteryLifePercent: 255,
                SystemStatusFlag: 0,
                BatteryLifeTime: 0,
                BatteryFullLifeTime: 0,
            };
            // SAFETY: an out-pointer to a live struct.
            if unsafe { GetSystemPowerStatus(&raw mut power) } == 0 {
                return BatteryStatus { charging: None, percent: None };
            }
            BatteryStatus {
                charging: match power.ACLineStatus {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                },
                // 255: unknown; flag 128: no system battery.
                percent: (power.BatteryLifePercent <= 100 && power.BatteryFlag & 128 == 0)
                    .then_some(power.BatteryLifePercent),
            }
        }
    }
}
