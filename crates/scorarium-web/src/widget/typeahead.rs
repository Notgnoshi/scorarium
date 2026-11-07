use askama::Template;

/// A text input that offers suggestions from a fragment route as the user types
///
/// The fragment route returns HTML fragments, which this [Typeahead] widget knows how to render and
/// act on in combination with `typeahead.js`.
///
/// There are three ways to use a typeahead:
///
/// 1. Selection is a go-to link
/// 2. Selection inputs text into the input
/// 3. Selection is an action that submits a form
///
/// These options map to the macros offered by `widget/suggestion.html` to facilitate building a
/// fragment route:
///
/// 1. `link(href)` renders a link
/// 2. `value(text)` renders a button carrying a `data-value` that inputs `text` into the form
/// 3. `action(url, name, value)` renders a submit button in its own form. Selection posts
///    `name=value` to `url` immediately
/// 4. `empty()` renders `No matches`
#[derive(Debug, Template)]
#[template(path = "widget/typeahead.html")]
pub struct Typeahead {
    /// The form field name and the element id
    pub name: String,
    /// The route that returns the suggestion fragment for `?q=`
    pub url: String,
    pub placeholder: String,
    /// The current text
    pub value: String,
}

impl askama::filters::HtmlSafe for Typeahead {}
