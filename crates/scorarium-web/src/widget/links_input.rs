use askama::Template;
use scorarium_engine::link::{HttpUrl, Link, LinkId};
use serde::Deserialize;

use crate::forms;
use crate::icon::Icon;
use crate::widget::list::Edits;
use crate::widget::refusals::Refusals;

#[derive(Clone, Deserialize)]
struct SavedLink {
    id: String,
    url: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct LinkEntry {
    saved: Option<SavedLink>,
    removed: bool,
    url: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct LinksForm {
    #[serde(deserialize_with = "forms::entries")]
    link: Vec<LinkEntry>,
}

impl LinksForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Edits<LinkId, HttpUrl> {
        let mut edits = Edits {
            remove: vec![],
            add: vec![],
        };
        for (n, entry) in self.link.iter().enumerate() {
            let saved = match &entry.saved {
                Some(saved) => match refusals.check(parse_link_id(&saved.id)) {
                    Some(id) => Some((id, saved.url.trim() != entry.url.trim())),
                    None => continue,
                },
                None => None,
            };
            let field = format!("link[{n}][url]");
            edits.entry(saved, entry.removed, &entry.url, |text| {
                refusals.parse(&field, text)
            });
        }
        edits
    }
}

impl From<&[Link]> for LinksForm {
    fn from(links: &[Link]) -> Self {
        let link = links
            .iter()
            .map(|link| LinkEntry {
                saved: Some(SavedLink {
                    id: link.id.to_string(),
                    url: link.url.to_string(),
                }),
                removed: false,
                url: link.url.to_string(),
            })
            .collect();
        LinksForm { link }
    }
}

fn parse_link_id(id: &str) -> scorarium_engine::Result<LinkId> {
    id.parse()
        .map_err(|_| scorarium_engine::Error::InvalidValue(format!("Unknown link '{id}'.")))
}

struct EntryInput {
    entry: LinkEntry,
    icon: Icon,
    refusal: Option<String>,
    autofocus: bool,
}

#[derive(Template)]
#[template(path = "widget/links_input.html")]
pub struct LinksInput {
    entries: Vec<EntryInput>,
    blank: EntryInput,
}

impl askama::filters::HtmlSafe for LinksInput {}

impl LinksInput {
    /// Draw the entries of `form` beside the stored `links` and the refusals of a Save
    pub fn new(form: &LinksForm, links: &[Link], refusals: &Refusals) -> LinksInput {
        let entries = form
            .link
            .iter()
            .enumerate()
            .map(|(n, entry)| {
                let field = format!("link[{n}][url]");
                let url = entry.url.trim();
                EntryInput {
                    entry: entry.clone(),
                    icon: links
                        .iter()
                        .find(|link| link.url.to_string() == url)
                        .map_or(Icon::Link, Icon::link),
                    refusal: refusals.of(&field).map(str::to_owned),
                    autofocus: refusals.first() == Some(field.as_str()),
                }
            })
            .collect();
        LinksInput {
            entries,
            blank: EntryInput {
                entry: LinkEntry::default(),
                icon: Icon::Link,
                refusal: None,
                autofocus: false,
            },
        }
    }
}
