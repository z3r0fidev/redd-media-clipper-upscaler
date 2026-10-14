//! rmedia core: all media logic shared by the CLI and the desktop app.

pub mod discover;
pub mod doctor;
pub mod error;
pub mod event;
pub mod exec;
pub mod input;
pub mod models;
pub mod paths;
pub mod pathser;
pub mod plan;
pub mod preset;
pub mod preview;
pub mod probe;
pub mod request;
pub mod settings;
pub mod time;
pub mod tools;
pub mod upscale;

pub use error::{Error, ErrorInfo, Result};
pub use time::{EndSpec, Ts};
