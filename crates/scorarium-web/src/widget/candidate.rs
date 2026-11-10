use askama::Template;
use scorarium_engine::entity::EntityKind;
use scorarium_engine::link::HttpUrl;
use scorarium_engine::suggest::ExternalCandidate;

#[derive(Template)]
#[template(path = "widget/candidate.html")]
pub(crate) struct Candidate {
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) record: HttpUrl,
    pub(crate) source: &'static str,
    pub(crate) cover: Option<HttpUrl>,
}

impl askama::filters::HtmlSafe for Candidate {}

impl Candidate {
    /// Summarize `candidate`, or return `None` if its record has no page for a publication
    pub fn new(candidate: &ExternalCandidate) -> Option<Candidate> {
        let record = candidate.record.url(EntityKind::Publication)?;
        let title = match &candidate.subtitle {
            Some(subtitle) => format!("{}: {subtitle}", candidate.title),
            None => candidate.title.clone(),
        };
        let published: Vec<String> = candidate
            .publisher
            .iter()
            .cloned()
            .chain(candidate.year.map(|year| year.get().to_string()))
            .collect();
        let parts: Vec<String> = [candidate.contributors.join(", "), published.join(", ")]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect();
        Some(Candidate {
            title,
            detail: parts.join(" - "),
            record,
            source: candidate.record.kind().name(),
            cover: candidate.cover.clone(),
        })
    }
}
