// TODO: Remove once scorarium-web migrates fully to the new scorarium-engine API
pub mod legacy;

pub mod audit;
pub mod contributor;
pub mod entity;
pub mod holding;
pub mod library;
pub mod link;
pub mod person;
pub mod publication;
pub mod search;
pub mod suggest;
pub mod value;
pub mod work;

mod demo;
mod engine;
mod error;

pub use engine::{Engine, EngineOptions};
pub use error::{Error, Result};
