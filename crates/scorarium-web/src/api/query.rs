use serde::Deserialize;

/// The number of entries fragment suggestion routes return
pub(crate) const SUGGESTION_LIMIT: usize = 8;

/// The `?q=` every fragment route takes
#[derive(Deserialize)]
pub(crate) struct Query {
    pub q: Option<String>,
}
