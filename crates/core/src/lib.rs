//! Osprey's core: everything the app does that isn't its window.
//!
//! [`app`] holds the state of the three screens (the queue, the tools and
//! the settings) and what each action does to it; the window
//! (`crates/desktop` and `ui/`) only shows that state and sends the actions.

pub mod app;
pub mod app_update;
pub mod config;
pub mod desktop;
pub mod engine;
pub mod explain;
pub mod i18n;
pub mod live;
pub mod log;
pub mod metadata;
pub mod process;
pub mod provision;
pub mod redact;
pub mod runner;

#[cfg(test)]
pub(crate) mod testing;
