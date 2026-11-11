use askama::Template;

use crate::widget::confirm::Confirm;

/// The heading and the action bar of an edit page
#[derive(Template)]
#[template(path = "widget/edit_heading.html")]
pub(crate) struct EditHeading {
    pub title: String,
    pub mode: EditMode,
    /// Where Cancel returns back to
    pub cancel: String,
    /// The confirmation that the destructive button opens
    pub delete: Confirm,
}

/// What an edit page is editing, which decides the buttons of its action bar
pub(crate) enum EditMode {
    /// Delete, Cancel, Save
    Committed,
    /// Discard draft, Cancel, Save
    Draft,
    /// Discard draft, Cancel, Save draft, Finish import
    Import {
        /// The route that saves the form and commits the draft
        finish: String,
    },
}

impl askama::filters::HtmlSafe for EditHeading {}
