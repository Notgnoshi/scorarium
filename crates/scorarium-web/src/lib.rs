mod api;
mod assets;
mod auth;
mod duplicate;
mod error;
mod forms;
mod hit;
mod icon;
mod page;
mod proposal;
mod redirect;
mod router;
mod serve;
mod sort;
mod widget;

pub use router::{SESSION_COOKIE, Settings, router};
pub use serve::{ServeArgs, serve};

const VERSION: &str = env!("CARGO_PKG_VERSION");
