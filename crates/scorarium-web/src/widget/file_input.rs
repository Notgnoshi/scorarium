use askama::Template;

/// A control that takes a file and uploads it
#[derive(Template)]
#[template(path = "widget/file_input.html")]
pub(crate) struct FileInput {
    /// The URL that the file is posted to
    pub url: String,
    /// The file types that the chooser offers, as an `accept` attribute
    pub accept: Option<&'static str>,
    /// Whether files uploads are disabled
    pub disabled: bool,
}

impl askama::filters::HtmlSafe for FileInput {}
