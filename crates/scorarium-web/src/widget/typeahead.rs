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
///    in the hidden input listed by [Typeahead::picked]
/// 4. `empty()` renders `No matches`
#[derive(Debug, Template)]
#[template(path = "widget/typeahead.html")]
pub struct Typeahead {
    /// The element id, unique on the page
    pub id: String,
    /// The form field name
    pub name: String,
    /// The route that returns the suggestion fragment for `?q=`
    pub url: String,
    pub placeholder: String,
    /// The current text
    pub value: String,
    /// The hidden input that stores the picked entity's id: its field name and initial value
    pub picked: Option<(String, String)>,
    /// The validation error message to display when a pick is *required* (to disallow free text entry)
    pub pick_required: Option<String>,
    /// The reason that a Save refused the field
    pub refusal: Option<String>,
    /// Any extra attributes to add to the `<input>`
    pub attrs: Vec<(&'static str, String)>,
}

impl Typeahead {
    /// Get the message beneath the field: the refusal, or else the demand for a selection
    fn feedback(&self) -> Option<&str> {
        self.refusal.as_deref().or(self.pick_required.as_deref())
    }
}

impl askama::filters::HtmlSafe for Typeahead {}
