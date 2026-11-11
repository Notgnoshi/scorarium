use askama::Template;
use scorarium_engine::link::{HttpUrl, LinkId};
use serde::Deserialize;

use crate::page::library::entity::ExternalLink;
use crate::widget::list::Edits;
use crate::widget::refusals::Refusals;

/// An editable list of links that posts `add_link` and `remove_link` for each change
#[derive(Debug, Template)]
#[template(path = "widget/links_input.html")]
pub struct LinksInput {
    pub links: Vec<ExternalLink>,
}

impl askama::filters::HtmlSafe for LinksInput {}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct LinksForm {
    add_link: Vec<String>,
    remove_link: Vec<String>,
}

impl LinksForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Edits<LinkId, HttpUrl> {
        let add = self
            .add_link
            .iter()
            .filter_map(|url| refusals.check(url.parse()))
            .collect();
        let remove = self
            .remove_link
            .iter()
            .filter_map(|id| refusals.check(parse_link_id(id)))
            .collect();
        Edits { remove, add }
    }
}

fn parse_link_id(id: &str) -> scorarium_engine::Result<LinkId> {
    id.parse()
        .map_err(|_| scorarium_engine::Error::InvalidValue(format!("Unknown link '{id}'.")))
}
