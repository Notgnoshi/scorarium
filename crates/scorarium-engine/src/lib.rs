// TODO: Remove once scorarium-web migrates fully to the new scorarium-engine API
pub mod legacy;

mod engine;
mod error;

pub use engine::{Engine, EngineOptions};
pub use error::{Error, Result};
