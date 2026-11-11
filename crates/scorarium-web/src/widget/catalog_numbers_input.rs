use askama::Template;
use scorarium_engine::work::CatalogNumber;
use serde::Deserialize;

use crate::widget::list::Edits;
use crate::widget::refusals::Refusals;

/// An editable list of work catalog numbers that posts `add_catalog_number` and
/// `remove_catalog_number` for each change
#[derive(Debug, Template)]
#[template(path = "widget/catalog_numbers_input.html")]
pub struct CatalogNumbersInput {
    pub numbers: Vec<CatalogNumber>,
}

impl askama::filters::HtmlSafe for CatalogNumbersInput {}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct CatalogNumbersForm {
    add_catalog_number: Vec<String>,
    remove_catalog_number: Vec<String>,
}

impl CatalogNumbersForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Edits<CatalogNumber, CatalogNumber> {
        let add = self
            .add_catalog_number
            .iter()
            .filter_map(|number| refusals.check(number.parse()))
            .collect();
        let remove = self
            .remove_catalog_number
            .iter()
            .filter_map(|number| refusals.check(number.parse()))
            .collect();
        Edits { remove, add }
    }
}
