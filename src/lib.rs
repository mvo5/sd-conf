//! Read systemd-style INI configuration files with drop-in override support.
//!
//! See the crate README for an overview. Public surface: [`SearchPaths`],
//! [`Config`], [`Error`].

mod config;
mod dropins;
mod error;
mod parser;
mod paths;

pub use config::Config;
pub use error::Error;
pub use paths::SearchPaths;
