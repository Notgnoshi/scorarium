use serde::Deserialize;

/// The `?q=` every fragment route takes
#[derive(Deserialize)]
pub(crate) struct Query {
    pub q: Option<String>,
}
