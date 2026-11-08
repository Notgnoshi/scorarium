use askama::Template;

use crate::page::library::entity::ExternalLink;

/// An editable list of links that posts `add_link` and `remove_link` for each change
#[derive(Debug, Template)]
#[template(path = "widget/links_input.html")]
pub struct LinksInput {
    pub links: Vec<ExternalLink>,
}

impl askama::filters::HtmlSafe for LinksInput {}
