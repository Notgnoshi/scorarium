use std::path::Path;
use std::time::UNIX_EPOCH;

use askama::Template;
use scorarium_engine::entity::{AnyEntity, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::{Person, PersonId, PersonSummary};
use scorarium_engine::publication::Publication;
use scorarium_engine::work::Work;

use crate::icon::Icon;

/// A cover or profile picture, or the placeholder that stands in for a missing one
#[derive(Clone, Template)]
#[template(path = "widget/picture.html")]
pub(crate) struct Picture {
    /// The versioned URL of the picture, or `None` to show the placeholder
    src: Option<String>,
    /// The icon the placeholder shows
    icon: Icon,
    /// The entity's name, which captions the placeholder and describes the picture
    title: String,
    /// Whether to draw the picture as a thumbnail with a fixed size
    thumbnail: bool,
}

impl Picture {
    /// Draw the picture as a thumbnail with a fixed size
    pub fn thumbnail(mut self) -> Picture {
        self.thumbnail = true;
        self
    }

    /// Show the file at `src` in place of the entity's own picture, or the placeholder for `None`
    pub fn showing(mut self, src: Option<String>) -> Picture {
        self.src = src;
        self
    }

    /// Check whether the placeholder is shown in place of a picture
    pub fn is_placeholder(&self) -> bool {
        self.src.is_none()
    }

    /// Show the profile picture of a person summarized in `library`
    pub fn summary(library: LibraryId, person: &PersonSummary) -> Picture {
        Picture {
            src: person
                .profile
                .as_deref()
                .map(|profile| profile_src(library, person.id, profile)),
            icon: Icon::Person,
            title: person.name.clone(),
            thumbnail: false,
        }
    }
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
            thumbnail: false,
        }
    }
}

/// Show the person's profile picture, or the person placeholder
impl From<&Person> for Picture {
    fn from(person: &Person) -> Self {
        Picture {
            src: person
                .profile()
                .map(|profile| profile_src(person.library(), person.id(), profile)),
            icon: Icon::Person,
            title: person.name().to_owned(),
            thumbnail: false,
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
            thumbnail: false,
        }
    }
}

/// Show the picture of whichever entity a search hit holds
impl From<&AnyEntity> for Picture {
    fn from(entity: &AnyEntity) -> Self {
        match entity {
            AnyEntity::Publication(publication) => Picture::from(publication),
            AnyEntity::Work(work) => Picture::from(work),
            AnyEntity::Person(person) => Picture::from(person),
        }
    }
}

/// Get the versioned URL that serves the profile picture of a person
fn profile_src(library: LibraryId, person: PersonId, profile: &Path) -> String {
    versioned(
        format!("/library/{library}/person/{person}/profile"),
        profile,
    )
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
