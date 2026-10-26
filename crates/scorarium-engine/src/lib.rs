// TODO: Remove once scorarium-web migrates fully to the new scorarium-engine API
pub mod legacy;

pub mod audit;
pub mod entity;
pub mod library;
pub mod publication;
pub mod value;
pub mod work;

mod engine;
mod error;

pub use engine::{Engine, EngineOptions};
pub use error::{Error, Result};
