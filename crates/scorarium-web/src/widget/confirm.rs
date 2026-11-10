use askama::Template;

/// A modal dialog that asks before a destructive action and holds the form that performs it
#[derive(Template)]
#[template(path = "widget/confirm.html")]
pub(crate) struct Confirm {
    /// The element id, which the button that opens the dialog refers to
    pub id: String,
    /// The question in the header
    pub title: String,
    /// One or two sentences that say what is deleted
    pub message: String,
    /// What the deletion takes with it; a non-empty list is rendered as a "dangerous" dialog
    pub also_deleted: Vec<String>,
    /// The label of the button that goes ahead
    pub action: &'static str,
    /// The URL that confirming posts to
    pub url: String,
}

impl askama::filters::HtmlSafe for Confirm {}

pub(crate) fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
