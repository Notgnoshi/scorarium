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
    /// The name of the hidden field that holds the id of the staged file
    pub staged_name: String,
    /// The staged file that the form holds
    pub staged: Option<StagedAsset>,
    /// Whether the name and the progress of a file take the place of the field
    pub shows_file: bool,
    /// The name that the form posts the URL field under
    pub url_name: String,
    pub url_text: String,
    /// Why a Save refused the URL text
    pub refusal: Option<String>,
    pub autofocus: bool,
    /// The tooltip of the browse button, which is then drawn as an icon
    pub browse_icon: Option<&'static str>,
}

impl askama::filters::HtmlSafe for FileInput {}
