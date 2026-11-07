use askama::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Template)]
#[template(path = "widget/stars.html")]
pub struct Stars {
    stars: u8,
}

impl From<scorarium_engine::value::Stars> for Stars {
    fn from(stars: scorarium_engine::value::Stars) -> Stars {
        Stars { stars: stars.get() }
    }
}

impl askama::filters::HtmlSafe for Stars {}
