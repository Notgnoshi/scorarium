use askama::Template;
use scorarium_engine::publication::{Identifier, IdentifierKind};
use scorarium_engine::{Error, Result};
use serde::Deserialize;

use crate::widget::list::Edits;
use crate::widget::refusals::Refusals;

/// An editable list of publication identifiers that posts `add_identifier` and
/// `remove_identifier` for each change
///
/// Each posted value is `<kind>:<value>`, where the kind is the `IdentifierKind` display name.
#[derive(Debug, Template)]
#[template(path = "widget/identifiers_input.html")]
pub struct IdentifiersInput {
    pub identifiers: Vec<Identifier>,
}

impl askama::filters::HtmlSafe for IdentifiersInput {}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct IdentifiersForm {
    add_identifier: Vec<String>,
    remove_identifier: Vec<String>,
}

impl IdentifiersForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Edits<Identifier, Identifier> {
        let add = self
            .add_identifier
            .iter()
            .filter_map(|identifier| refusals.check(parse(identifier)))
            .collect();
        let remove = self
            .remove_identifier
            .iter()
            .filter_map(|identifier| refusals.check(parse(identifier)))
            .collect();
        Edits { remove, add }
    }
}

/// Get the label shown before an identifier, if its kind needs one
pub(crate) fn kind_label(kind: IdentifierKind) -> Option<&'static str> {
    match kind {
        IdentifierKind::Isbn => Some("ISBN"),
        IdentifierKind::Ismn => Some("ISMN"),
        IdentifierKind::PlateNumber => Some("Plate No."),
        IdentifierKind::PublisherNumber => None,
    }
}

/// Parse an identifier staged as `<kind>:<value>`
fn parse(staged: &str) -> Result<Identifier> {
    // identifier values can contain colons
    let Some((kind, value)) = staged.split_once(':') else {
        return Err(Error::InvalidValue(format!(
            "Unknown identifier '{staged}'."
        )));
    };
    Identifier::new(kind.parse()?, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_on_first_colon_only() {
        let identifier = parse("plate_number:A:123").unwrap();
        assert_eq!(identifier.kind(), IdentifierKind::PlateNumber);
        assert_eq!(identifier.as_str(), "A:123");
    }

    #[test]
    fn parse_requires_a_colon() {
        assert!(matches!(
            parse("9780306406157"),
            Err(Error::InvalidValue(_))
        ));
    }
}
