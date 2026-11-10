use askama::Template;
use scorarium_engine::value::Classification;

/// A Music/Prose toggle that posts `classification`, with neither side chosen for `None`
#[derive(Debug, Template)]
#[template(path = "widget/classification_input.html")]
pub struct ClassificationInput {
    pub value: Option<Classification>,
}

impl askama::filters::HtmlSafe for ClassificationInput {}
