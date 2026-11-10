use askama::Template;

use crate::widget::confirm::Confirm;

/// The heading of an entity's edit page
#[derive(Template)]
#[template(path = "widget/edit_heading.html")]
pub(crate) struct EditHeading {
    pub title: String,
    pub commit: Option<CommitAction>,
    /// The confirmation that the destructive button opens
    pub delete: Confirm,
}

pub(crate) struct CommitAction {
    /// The route that saves the form and commits
    pub url: String,
    pub tooltip: &'static str,
}

impl askama::filters::HtmlSafe for EditHeading {}
