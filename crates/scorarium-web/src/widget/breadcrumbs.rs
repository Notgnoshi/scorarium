use askama::Template;

#[derive(Debug)]
pub struct Crumb {
    label: String,
    href: String,
}

impl Crumb {
    pub fn new(label: impl Into<String>, href: impl Into<String>) -> Crumb {
        Crumb {
            label: label.into(),
            href: href.into(),
        }
    }
}

#[derive(Debug, Template)]
#[template(path = "widget/breadcrumbs.html")]
pub struct Breadcrumbs {
    parents: Vec<Crumb>,
    current: String,
}

impl Breadcrumbs {
    pub fn new(parents: Vec<Crumb>, current: impl Into<String>) -> Breadcrumbs {
        Breadcrumbs {
            parents,
            current: current.into(),
        }
    }
}

impl askama::filters::HtmlSafe for Breadcrumbs {}
