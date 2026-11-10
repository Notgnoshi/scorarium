use askama::Template;

use crate::widget::confirm::Confirm;

/// The heading of an entity's edit page
#[derive(Template)]
#[template(path = "widget/edit_heading.html")]
pub(crate) struct EditHeading {
    pub title: String,
    pub draft: bool,
    /// The confirmation that the destructive button opens
    pub delete: Confirm,
}

impl askama::filters::HtmlSafe for EditHeading {}
