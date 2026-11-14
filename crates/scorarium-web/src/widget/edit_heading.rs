use askama::Template;

use crate::widget::review_queue::EnrichmentCell;

/// The title of an edit page
#[derive(Template)]
#[template(path = "widget/edit_heading.html")]
pub(crate) struct EditHeading {
    pub title: String,
    pub draft: bool,
    /// The entity's own enrichment task, or `None` for a page with no entity
    pub enrichment: Option<EnrichmentCell>,
}

impl askama::filters::HtmlSafe for EditHeading {}
