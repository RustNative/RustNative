//! Device desired state: the cloud declares what a device's configuration
//! should be; the device reconciles towards it and reports what it is.
//!
//! The same idea as the UI reconciler, applied to a fleet:
//!
//! - [`Twin`] holds a device's desired and reported state, each versioned.
//! - A [`DeviceAgent`] on the device applies a newer desired state
//!   (through the application's [`Actuate`]) and reports the result. Offline,
//!   it keeps the last desired state it saw and queues its reports; back
//!   online, it catches up — at most one application of the newest desire,
//!   however many it missed.
//! - A change made on the device itself (a knob turned) is settled with
//!   the desired state by the declared [`DevicePolicy`].
//! - [`DeviceModel`] maps typed state to a standard device data model's
//!   resource paths (LwM2M/IPSO object/instance/resource); commissioning is
//!   the existing stacks' job (`docs/sync.md`).

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock::Hlc;

/// A versioned state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Versioned<T> {
    /// The state.
    pub value: T,
    /// Its version (desired: set by the cloud; reported: the desired
    /// version it reflects).
    pub version: u64,
    /// When it was written.
    pub stamp: Hlc,
}

/// A device's desired and reported state, as the cloud holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Twin<T> {
    /// What it should be.
    pub desired: Versioned<T>,
    /// What it last said it is.
    pub reported: Option<Versioned<T>>,
}

impl<T: Clone + PartialEq> Twin<T> {
    /// A twin desiring `value`.
    pub const fn new(value: T, stamp: Hlc) -> Self {
        Self { desired: Versioned { value, version: 1, stamp }, reported: None }
    }

    /// Declares a new desired state.
    pub fn desire(&mut self, value: T, stamp: Hlc) {
        self.desired = Versioned { value, version: self.desired.version + 1, stamp };
    }

    /// Records a report.
    pub fn report(&mut self, reported: Versioned<T>) {
        if self.reported.as_ref().is_none_or(|current| reported.stamp > current.stamp) {
            self.reported = Some(reported);
        }
    }

    /// Whether the device has converged on the newest desire.
    #[must_use]
    pub fn converged(&self) -> bool {
        self.reported.as_ref().is_some_and(|reported| {
            reported.version == self.desired.version && reported.value == self.desired.value
        })
    }
}

/// How a device-local change is settled with the desired state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePolicy {
    /// The cloud's desire wins; a local change is undone at the next
    /// desired state.
    DesiredWins,
    /// The later of the two (by clock) wins.
    LastWriterWins,
}

/// What applies a state on the device.
pub trait Actuate<T> {
    /// Makes the device so; returns what it actually is (a device may clamp
    /// or refuse part of it).
    ///
    /// # Errors
    ///
    /// The hardware refused.
    fn apply(&mut self, desired: &T) -> Result<T, String>;
}

/// The device side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAgent<T> {
    current: Versioned<T>,
    outbox: Vec<Versioned<T>>,
    applications: u32,
}

impl<T: Clone + PartialEq> DeviceAgent<T> {
    /// A device whose state is `initial`.
    pub fn new(initial: T, stamp: Hlc) -> Self {
        Self {
            current: Versioned { value: initial, version: 0, stamp },
            outbox: Vec::new(),
            applications: 0,
        }
    }

    /// Its state.
    pub const fn current(&self) -> &Versioned<T> {
        &self.current
    }

    /// How many times it has applied a desired state.
    pub const fn applications(&self) -> u32 {
        self.applications
    }

    /// A desired state arrived (or several did while offline; the newest
    /// is enough). Applies it if newer and the policy lets it, and queues
    /// the report.
    ///
    /// # Errors
    ///
    /// The device refused; nothing is reported.
    pub fn reconcile(
        &mut self,
        desired: &Versioned<T>,
        policy: DevicePolicy,
        actuator: &mut impl Actuate<T>,
        now: Hlc,
    ) -> Result<(), String> {
        if desired.version <= self.current.version {
            return Ok(());
        }
        if policy == DevicePolicy::LastWriterWins && self.current.stamp > desired.stamp {
            // The local change is newer: report it as the answer to this
            // desire, and let the cloud see what the device chose.
            self.current.version = desired.version;
            self.outbox.push(self.current.clone());
            return Ok(());
        }
        let actual = actuator.apply(&desired.value)?;
        self.applications += 1;
        self.current = Versioned { value: actual, version: desired.version, stamp: now };
        self.outbox.push(self.current.clone());
        Ok(())
    }

    /// A change made on the device itself.
    pub fn local_change(&mut self, value: T, now: Hlc) {
        self.current = Versioned { value, version: self.current.version, stamp: now };
        self.outbox.push(self.current.clone());
    }

    /// The reports to send (the newest only: older ones are superseded).
    pub fn take_reports(&mut self) -> Option<Versioned<T>> {
        let newest = self.outbox.pop();
        self.outbox.clear();
        newest
    }
}

/// A mapping between typed state and a standard device data model.
pub trait DeviceModel: Sized {
    /// The state as resources, by path (`"3303/0/5700"`: IPSO temperature,
    /// instance 0, sensor value).
    fn to_resources(&self) -> BTreeMap<String, Value>;

    /// The state from resources.
    ///
    /// # Errors
    ///
    /// A required resource is missing or of the wrong type.
    fn from_resources(resources: &BTreeMap<String, Value>) -> Result<Self, String>;
}

/// Serializes a twin's desired state for a topic (`devices/<id>/desired`).
///
/// # Errors
///
/// The state does not serialize.
pub fn encode<T: Serialize>(state: &Versioned<T>) -> Result<Vec<u8>, String> {
    serde_json::to_vec(state).map_err(|error| error.to_string())
}

/// Reads a versioned state from a topic's payload.
///
/// # Errors
///
/// The payload is not one.
pub fn decode<T: DeserializeOwned>(payload: &[u8]) -> Result<Versioned<T>, String> {
    serde_json::from_slice(payload).map_err(|error| error.to_string())
}
