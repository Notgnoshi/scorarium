use std::path::Path;
use std::time::UNIX_EPOCH;

use askama::Template;
use scorarium_engine::entity::Entity;
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use scorarium_engine::work::Work;

use crate::icon::Icon;

/// A cover or profile picture, or the placeholder that stands in for a missing one
#[derive(Template)]
#[template(path = "widget/picture.html")]
pub(crate) struct Picture {
    /// The versioned URL of the picture, or `None` to show the placeholder
    src: Option<String>,
    /// The icon the placeholder shows
    icon: Icon,
    /// The entity's name, which captions the placeholder and describes the picture
    title: String,
}

impl askama::filters::HtmlSafe for Picture {}

/// Show the publication's cover, or the publication placeholder
impl From<&Publication> for Picture {
    fn from(publication: &Publication) -> Self {
        let src = publication.cover().map(|cover| {
            let url = format!(
                "/library/{}/publication/{}/cover",
                publication.library(),
                publication.id()
            );
            versioned(url, cover)
        });
        Picture {
            src,
            icon: Icon::Publication,
            title: publication.title().unwrap_or("Untitled").to_owned(),
        }
    }
}

/// Show the person's profile picture, or the person placeholder
impl From<&Person> for Picture {
    fn from(person: &Person) -> Self {
        let src = person.profile().map(|profile| {
            let url = format!(
                "/library/{}/person/{}/profile",
                person.library(),
                person.id()
            );
            versioned(url, profile)
        });
        Picture {
            src,
            icon: Icon::Person,
            title: person.name().to_owned(),
        }
    }
}

/// Show the work's placeholder, because a work has no picture
impl From<&Work> for Picture {
    fn from(work: &Work) -> Self {
        Picture {
            src: None,
            icon: Icon::work(work.classification()),
            title: work.title().to_owned(),
        }
    }
}

/// Append the modification time of `file` to `url` as its version
///
/// This means when the file is changed, the browser will pick up on it immediately.
fn versioned(url: String, file: &Path) -> String {
    let since_epoch = std::fs::metadata(file)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok());
    // Whole seconds would give two replacements within one second the same URL, and the browser
    // would keep showing the first, which it caches as immutable.
    match since_epoch {
        Some(since_epoch) => format!("{url}?v={}", since_epoch.as_millis()),
        None => url,
    }
}
