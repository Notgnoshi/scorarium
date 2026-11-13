use askama::Template;

/// The title of an edit page
#[derive(Template)]
#[template(path = "widget/edit_heading.html")]
pub(crate) struct EditHeading {
    pub title: String,
    pub draft: bool,
}

impl askama::filters::HtmlSafe for EditHeading {}
