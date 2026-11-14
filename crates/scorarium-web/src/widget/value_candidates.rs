use askama::Template;

use crate::proposal::Candidate;

/// The form inputs that accepting a candidate fills in
type Fills = Vec<(&'static str, String)>;

/// The proposed values of a field, with the sources that proposed them
#[derive(Template)]
#[template(path = "widget/value_candidates.html")]
pub(crate) struct ValueCandidates {
    candidates: Vec<(Candidate<String>, Fills)>,
}

impl ValueCandidates {
    /// Draw `candidates`, each of which accepting writes to `field`
    pub fn new(field: &'static str, candidates: Vec<Candidate<String>>) -> ValueCandidates {
        let candidates = candidates
            .into_iter()
            .map(|candidate| {
                let fills = vec![(field, candidate.value.clone())];
                (candidate, fills)
            })
            .collect();
        ValueCandidates { candidates }
    }

    /// Write what `value` derives from a candidate's text to `field` as well
    pub fn also_filling(
        mut self,
        field: &'static str,
        value: impl Fn(&str) -> String,
    ) -> ValueCandidates {
        for (candidate, fills) in &mut self.candidates {
            fills.push((field, value(&candidate.value)));
        }
        self
    }
}

impl askama::filters::HtmlSafe for ValueCandidates {}
