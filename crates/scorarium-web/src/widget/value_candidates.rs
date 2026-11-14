use askama::Template;

use crate::proposal::Candidate;

/// The proposed values of a field, with the sources that proposed them
#[derive(Template)]
#[template(path = "widget/value_candidates.html")]
pub(crate) struct ValueCandidates {
    candidates: Vec<Candidate<String>>,
}

impl ValueCandidates {
    pub fn new(candidates: Vec<Candidate<String>>) -> ValueCandidates {
        ValueCandidates { candidates }
    }
}

impl askama::filters::HtmlSafe for ValueCandidates {}
