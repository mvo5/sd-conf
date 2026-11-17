//! Read systemd-style INI configuration files with drop-in override support.
//!
//! See the crate README for an overview. Public surface: [`SearchPaths`],
//! [`Config`], [`Error`], and [`parse_str`] with [`Entry`] and
//! [`ParseError`] for input that is not a file on disk.

mod config;
mod dropins;
mod error;
mod parser;
mod paths;

pub use config::Config;
pub use error::Error;
pub use parser::{parse_str, Entry, ParseError};
pub use paths::SearchPaths;
