use serde::Deserialize;

/// A table column that can be sorted
pub(crate) trait Column: Copy + Eq + Default + 'static {
    /// List every column, so that a query value can be matched to one
    const ALL: &'static [Self];

    /// Name the column as it appears in the `sort` query parameter
    fn name(self) -> &'static str;
}

/// A column and direction, parsed from `[-]column`
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(try_from = "String", bound = "C: Column")]
pub(crate) struct Sort<C> {
    pub column: C,
    pub descending: bool,
}

impl<C: Column> TryFrom<String> for Sort<C> {
    type Error = String;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        let (descending, name) = match value.strip_prefix('-') {
            Some(name) => (true, name),
            None => (false, value.as_str()),
        };
        let column = C::ALL
            .iter()
            .copied()
            .find(|column| column.name() == name)
            .ok_or_else(|| format!("unknown sort column {value:?}"))?;
        Ok(Sort { column, descending })
    }
}

impl<C: Column> Sort<C> {
    pub fn href(&self, column: C) -> String {
        let descending = self.is(column, false);
        format!(
            "?sort={}{}",
            if descending { "-" } else { "" },
            column.name()
        )
    }

    /// Check whether `column` is active in the given direction
    pub fn is(&self, column: C, descending: bool) -> bool {
        self.column == column && self.descending == descending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    enum Test {
        #[default]
        Title,
        Year,
    }

    impl Column for Test {
        const ALL: &'static [Self] = &[Test::Title, Test::Year];

        fn name(self) -> &'static str {
            match self {
                Test::Title => "title",
                Test::Year => "year",
            }
        }
    }

    #[test]
    fn sort_parses_direction_and_rejects_unknown_columns() {
        let year = Sort::try_from("-year".to_owned()).unwrap();
        assert!(year.is(Test::Year, true));
        assert!(
            Sort::try_from("title".to_owned())
                .unwrap()
                .is(Test::Title, false)
        );
        assert!(Sort::<Test>::try_from("bogus".to_owned()).is_err());
        assert_eq!(year.href(Test::Year), "?sort=year");
        assert_eq!(year.href(Test::Title), "?sort=title");
        assert_eq!(Sort::default().href(Test::Title), "?sort=-title");
    }
}
