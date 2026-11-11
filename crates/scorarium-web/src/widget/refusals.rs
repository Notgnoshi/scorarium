use std::str::FromStr;

use askama::Template;

/// The reasons that a submitted form was refused
#[derive(Default, Template)]
#[template(path = "widget/refusals.html")]
pub(crate) struct Refusals {
    /// The refused field, if any, and the message, in form order
    entries: Vec<(Option<String>, String)>,
}

impl Refusals {
    /// Parse `text`, recording a refusal against `field` when the engine refuses it
    pub fn parse<T>(&mut self, field: &str, text: &str) -> Option<T>
    where
        T: FromStr<Err = scorarium_engine::Error>,
    {
        self.check_field(field, text.parse())
    }

    /// Unwrap `result`, recording a refusal against `field` when it is an error
    pub fn check_field<T>(
        &mut self,
        field: &str,
        result: scorarium_engine::Result<T>,
    ) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                self.entries.push((Some(field.to_owned()), e.to_string()));
                None
            }
        }
    }

    /// Unwrap `result`, recording a refusal that concerns no field when it is an error
    pub fn check<T>(&mut self, result: scorarium_engine::Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                self.refuse(e.to_string());
                None
            }
        }
    }

    /// Record a refusal that concerns no field
    pub fn refuse(&mut self, message: impl Into<String>) {
        self.entries.push((None, message.into()));
    }

    /// Find the reason that `field` was refused
    pub fn of(&self, field: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(refused, _)| refused.as_deref() == Some(field))
            .map(|(_, message)| message.as_str())
    }

    /// Find the first field that was refused
    pub fn first(&self) -> Option<&str> {
        self.entries.iter().find_map(|(field, _)| field.as_deref())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl askama::filters::HtmlSafe for Refusals {}
