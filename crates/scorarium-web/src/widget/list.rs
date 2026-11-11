/// The edits that a form makes to one list
pub(crate) struct Edits<R, V> {
    /// The addresses of the stored values to remove
    pub remove: Vec<R>,
    /// The values to add
    pub add: Vec<V>,
}

impl<R, V> Edits<R, V> {
    /// Record what Save does with one entry
    ///
    /// `saved` is the address of a saved entry, and whether its fields differ from the text
    /// that it was drawn with. `parse` is called only for an entry that is neither removed
    /// nor blank.
    pub fn entry(
        &mut self,
        saved: Option<(R, bool)>,
        removed: bool,
        text: &str,
        parse: impl FnOnce(&str) -> Option<V>,
    ) {
        let blank = text.trim().is_empty();
        match saved {
            Some((address, _)) if removed || blank => self.remove.push(address),
            Some((address, true)) => {
                if let Some(value) = parse(text) {
                    self.remove.push(address);
                    self.add.push(value);
                }
            }
            Some((_, false)) => {}
            None if removed || blank => {}
            None => self.add.extend(parse(text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_applies_what_save_does_with_an_entry() {
        let parse = |text: &str| text.parse::<u32>().ok();
        let mut edits = Edits {
            remove: vec![],
            add: vec![],
        };
        edits.entry(Some(("unchanged", false)), false, "1", parse);
        edits.entry(Some(("removed", false)), true, "2", parse);
        edits.entry(Some(("edited", true)), false, "3", parse);
        edits.entry(Some(("cleared", true)), false, " ", parse);
        edits.entry(None, false, "", parse);
        edits.entry(None, false, "4", parse);
        edits.entry(None, false, "not valid", parse);
        edits.entry(Some(("edited, not valid", true)), false, "not valid", parse);
        edits.entry(Some(("removed, not valid", true)), true, "not valid", parse);
        assert_eq!(
            edits.remove,
            ["removed", "edited", "cleared", "removed, not valid"]
        );
        assert_eq!(edits.add, [3, 4]);
    }
}
