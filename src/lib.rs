//! MimirDLP: a portable graphical front end for yt-dlp.

pub mod app_update;
pub mod config;
pub mod desktop;
pub mod engine;
pub mod explain;
pub mod gui;
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
