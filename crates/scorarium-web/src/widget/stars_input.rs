use askama::Template;

/// A five-star rating input that posts `stars`, or nothing when no star is set
#[derive(Debug, Template)]
#[template(path = "widget/stars_input.html")]
pub struct StarsInput {
    pub value: Option<u8>,
}

impl askama::filters::HtmlSafe for StarsInput {}
