//! The native Win32 window backend: reconciles the platform-independent
//! Rust UI tree against real Win32 windows and controls, and runs the
//! message loop that drives it.

pub(crate) mod accelerator;
mod animation;
#[cfg(test)]
mod animation_integration;
mod app;
#[cfg(test)]
mod capture;
mod commands;
#[cfg(test)]
mod components_integration;
mod container;
pub(crate) mod context;
#[cfg(test)]
mod controls_integration;
mod cursor;
#[cfg(test)]
mod data_integration;
pub(crate) mod embed;
pub(crate) mod foreign;
pub(crate) mod graphics;
#[cfg(test)]
mod graphics_integration;
#[cfg(test)]
mod guarantees_integration;
#[cfg(test)]
mod harness;
pub(crate) mod host_content;
mod host_traits;
#[cfg(test)]
mod i18n_integration;
pub(crate) mod input;
#[cfg(test)]
mod input_integration;
mod inspect;
#[cfg(test)]
mod inspect_integration;
#[cfg(test)]
mod integration;
mod lifecycle;
mod measure;
mod memory_watch;
mod menu;
mod message_loop;
#[cfg(test)]
mod navigation_integration;
mod ownership;
#[cfg(test)]
mod preview_integration;
mod registry;
pub(crate) mod rendering;
#[cfg(test)]
mod responsiveness_integration;
mod runtime;
pub(crate) mod single_instance;
#[cfg(test)]
mod style_integration;
pub(crate) mod surfaces;
#[cfg(test)]
mod surfaces_integration;
mod teardown;
#[cfg(test)]
mod test_support;
pub(crate) mod uia;
#[cfg(test)]
mod uia_integration;
mod user_data;
pub(crate) mod util;
mod virtual_list;
#[cfg(test)]
mod virtual_list_integration;
pub(crate) mod win32;
pub(crate) mod window_handles;

pub(super) use app::run_application;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

pub(crate) const WINDOW_CLASS_NAME: &str = "NativeRustFrameworkWindow";
pub(crate) const CONTAINER_CLASS_NAME: &str = "NativeRustFrameworkContainer";
pub(crate) const WM_FRAMEWORK_SCHEDULE: u32 = WM_APP + 1;
pub(crate) const WM_MOUSELEAVE: u32 = 0x02A3;

#[link(name = "user32")]
unsafe extern "system" {
    /// Enables or disables a top-level owner while a modal child is open.
    pub(crate) fn EnableWindow(hwnd: HWND, enable: i32) -> i32;
}
