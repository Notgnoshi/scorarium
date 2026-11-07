use std::collections::BTreeSet;

use askama::Template;

/// A numbered page selector that links to the pages of a paginated list
#[derive(Debug, Template)]
#[template(path = "widget/pagination.html")]
pub struct Pagination {
    path: String,
    page: usize,
    pages: usize,
    previous: usize,
    next: usize,
    /// Page numbers to show in order, with `None` where a gap is elided
    window: Vec<Option<usize>>,
}

impl Pagination {
    /// Lay out the pages for `total` items at `page_size` per page, around the current `page`
    pub fn new(path: &str, page: usize, page_size: usize, total: usize) -> Pagination {
        let pages = total.div_ceil(page_size).max(1);
        // The page comes from the query string, so it may be anywhere in the range of `usize`.
        // Clip the neighborhood before iterating so a huge page number never iterates a huge range.
        let near = page.saturating_sub(2).max(1)..=page.saturating_add(2).min(pages);
        let shown: BTreeSet<usize> = [1, pages].into_iter().chain(near).collect();

        let mut window = Vec::new();
        let mut last = 0;
        for n in shown {
            if n != last + 1 && last != 0 {
                window.push(None);
            }
            window.push(Some(n));
            last = n;
        }

        Pagination {
            path: path.to_owned(),
            page,
            pages,
            previous: page.saturating_sub(1),
            next: page.saturating_add(1),
            window,
        }
    }
}

impl askama::filters::HtmlSafe for Pagination {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_window_surrounds_the_current_page() {
        let window = |page, total| Pagination::new("/settings/audit", page, 100, total).window;
        assert_eq!(
            window(15, 3000),
            [
                Some(1),
                None,
                Some(13),
                Some(14),
                Some(15),
                Some(16),
                Some(17),
                None,
                Some(30)
            ]
        );
        assert_eq!(window(1, 3000), [Some(1), Some(2), Some(3), None, Some(30)]);
        assert_eq!(window(1, 0), [Some(1)]);
        assert_eq!(window(usize::MAX, 3000), [Some(1), None, Some(30)]);
    }
}
