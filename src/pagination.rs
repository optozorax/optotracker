use std::ops::Range;

/// Offset-based pagination that can begin at an arbitrary item.
///
/// Unlike page-number pagination, this preserves a caller-selected anchor
/// that does not necessarily fall on a page-size boundary.
pub struct Pagination {
    start: usize,
    page_size: usize,
}

impl Pagination {
    pub fn new(page_size: usize) -> Self {
        assert!(page_size > 0, "page size must not be zero");
        Self {
            start: 0,
            page_size,
        }
    }

    pub fn set_start(&mut self, start: usize) {
        self.start = start;
    }

    pub fn visible_range(&mut self, total: usize) -> Range<usize> {
        self.normalize(total);
        self.start..self.start.saturating_add(self.page_size).min(total)
    }

    pub fn has_previous(&self) -> bool {
        self.start > 0
    }

    pub fn has_next(&self, total: usize) -> bool {
        self.start.saturating_add(self.page_size) < total
    }

    pub fn previous(&mut self) {
        self.start = self.start.saturating_sub(self.page_size);
    }

    pub fn next(&mut self, total: usize) {
        if self.has_next(total) {
            self.start += self.page_size;
        }
    }

    fn normalize(&mut self, total: usize) {
        if total == 0 {
            self.start = 0;
        } else if self.start >= total {
            self.start = total.saturating_sub(self.page_size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_start_is_preserved_across_navigation() {
        let mut pagination = Pagination::new(100);
        pagination.set_start(37);

        assert_eq!(pagination.visible_range(250), 37..137);
        pagination.next(250);
        assert_eq!(pagination.visible_range(250), 137..237);
        pagination.previous();
        assert_eq!(pagination.visible_range(250), 37..137);
        pagination.previous();
        assert_eq!(pagination.visible_range(250), 0..100);
    }

    #[test]
    fn out_of_bounds_start_recovers_to_a_full_last_page() {
        let mut pagination = Pagination::new(100);
        pagination.set_start(300);

        assert_eq!(pagination.visible_range(250), 150..250);
    }
}
