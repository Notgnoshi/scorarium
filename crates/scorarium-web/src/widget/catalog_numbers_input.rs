use askama::Template;
use scorarium_engine::work::CatalogNumber;

/// An editable list of work catalog numbers that posts `add_catalog_number` and
/// `remove_catalog_number` for each change
#[derive(Debug, Template)]
#[template(path = "widget/catalog_numbers_input.html")]
pub struct CatalogNumbersInput {
    pub numbers: Vec<CatalogNumber>,
}

impl askama::filters::HtmlSafe for CatalogNumbersInput {}
