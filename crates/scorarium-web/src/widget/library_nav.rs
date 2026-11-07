use askama::Template;
use scorarium_engine::contributor::Role;
use scorarium_engine::library::{Library, LibraryId};

use crate::error::Result;

/// The heading of a library, with a nav bar for its different views
#[derive(Debug, Template)]
#[template(path = "widget/library_nav.html")]
pub struct LibraryNav {
    id: LibraryId,
    name: String,
    private: bool,
    logged_in: bool,
    publications: usize,
    people: usize,
    roles: Vec<(Role, usize)>,
    active: Active,
}

/// A listings view of a library on its nav bar
#[derive(Debug, PartialEq, Eq)]
pub enum Active {
    Publications,
    People,
    Role(Role),
    Tags,
}

impl LibraryNav {
    /// Load the counts for the navigation bar of `library`
    pub async fn load(library: &Library, logged_in: bool, active: Active) -> Result<LibraryNav> {
        let mut roles = library.roles(false).await?;
        roles.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(LibraryNav {
            id: library.id(),
            name: library.name().to_owned(),
            private: library.is_private(),
            logged_in,
            publications: library.publication_count(false).await?,
            people: library.people_summary(None, false).await?.len(),
            roles,
            active,
        })
    }

    /// Check whether `role` is the active entry
    fn is_role(&self, role: &Role) -> bool {
        matches!(&self.active, Active::Role(active) if active == role)
    }
}

impl askama::filters::HtmlSafe for LibraryNav {}
