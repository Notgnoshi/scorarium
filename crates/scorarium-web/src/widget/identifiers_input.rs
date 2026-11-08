use askama::Template;
use scorarium_engine::publication::{Identifier, IdentifierKind};

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

/// Get the label shown before an identifier, if its kind needs one
pub(crate) fn kind_label(kind: IdentifierKind) -> Option<&'static str> {
    match kind {
        IdentifierKind::Isbn => Some("ISBN"),
        IdentifierKind::Ismn => Some("ISMN"),
        IdentifierKind::PlateNumber => Some("Plate No."),
        IdentifierKind::PublisherNumber => None,
    }
}
