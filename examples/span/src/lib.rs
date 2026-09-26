//! The one-stack span (`PLAN.md` Milestone 52): one application, written
//! once, for a desktop window and for a device's small screen. The
//! component depends on nothing but `rustnative-core`; each end supplies
//! only its platform and its screen size.
//!
//! - **Desktop:** `src/main.rs` runs it on Windows.
//! - **Device:** the same component on the headless backend at a device's
//!   240×320 display, as its tests do. Running it on a microcontroller is
//!   owed with Milestone 37 (see `docs/policy/embedded.md`).

use rustnative_core::{Component, Event, Node, NodeId, Size};

/// A device's display.
pub const DEVICE_SCREEN: Size = Size::new(240, 320);
/// A desktop window.
pub const DESKTOP_WINDOW: Size = Size::new(480, 360);

/// A thermostat: a target temperature and a mode.
pub struct Thermostat {
    target: i32,
    heating: bool,
}

impl Component for Thermostat {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self { target: 20, heating: true }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        Node::column(
            "thermostat",
            [
                Node::label("target", format!("{} °C", self.target)),
                Node::row("adjust", [Node::button("cooler", "−"), Node::button("warmer", "+")]),
                Node::button("mode", if self.heating { "Heating" } else { "Off" }),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        let Event::Click { target } = event else { return };
        if target == NodeId::from_key("cooler") {
            self.target = (self.target - 1).max(5);
        } else if target == NodeId::from_key("warmer") {
            self.target = (self.target + 1).min(30);
        } else if target == NodeId::from_key("mode") {
            self.heating = !self.heating;
        }
    }
}
