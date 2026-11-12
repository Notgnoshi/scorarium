use askama::Template;
use scorarium_engine::asset::StagedAsset;

/// A control that takes a file and uploads it
#[derive(Template)]
#[template(path = "widget/file_input.html")]
pub(crate) struct FileInput {
    /// The URL that the file is posted to
    pub url: String,
    /// The file types that the chooser offers, as an `accept` attribute
    pub accept: Option<&'static str>,
    /// The hint in the empty field
    pub hint: &'static str,
    /// Whether files uploads are disabled
    pub disabled: bool,
    /// Whether one paste, drop, or choice may carry several files, each sent as its own transfer
    pub multiple: bool,
    /// The name of the hidden field that holds the id of the staged file
    pub staged_name: Option<String>,
    /// The staged file that the form holds
    pub staged: Option<StagedAsset>,
    /// Whether the name and the progress of a file take the place of the field
    pub shows_file: bool,
}

impl askama::filters::HtmlSafe for FileInput {}
