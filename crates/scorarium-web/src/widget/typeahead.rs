use askama::Template;

/// A text input that offers suggestions from a fragment route as the user types
///
/// The fragment route returns HTML fragments, which this [Typeahead] widget knows how to render and
/// act on in combination with `typeahead.js`.
///
/// There are three ways to use a typeahead:
///
/// 1. Selection is a link
/// 2. Selection fills in text
/// 3. Selection fills in text and records an id
///
/// These options map to the macros offered by `widget/suggestion.html` to facilitate building a
/// fragment route:
///
/// 1. `link(href)` renders a link
/// 2. `value(text)` renders a button carrying a `data-value` that inputs `text` into the form
/// 3. `pick(id, text)` renders a button that, like `value()`, inputs `text`, but also records `id`
///    in the hidden input listed by [Typeahead::picked_id]
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
    /// A hidden input used by `pick()` to store the ID of the picked entity
    pub picked_id: Option<String>,
    /// The validation error message to display when a pick is *required* (to disallow free text entry)
    pub pick_required: Option<String>,
    /// Any extra attributes to add to the `<input>`
    pub attrs: Vec<(&'static str, String)>,
}

impl askama::filters::HtmlSafe for Typeahead {}
