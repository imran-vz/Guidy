//! The Storage page's numbers: how the disk bar divides and how the rows sort.

use guidy_core::downloads::format_bytes;
use guidy_core::storage::{BucketUsage, DiskSpace};

/// The smallest share Guidy's segment shows when it holds anything: about
/// 2 pt on the narrowest page, and the view also sets a 2 pt minimum width.
pub const MIN_GUIDY_FRACTION: f32 = 0.005;

/// The disk bar's segments, in order. They sum to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarFractions {
    pub guidy: f32,
    pub other_used: f32,
    pub free: f32,
}

/// Divide the bar. `guidy_bytes` counts only what's on this volume and is
/// clamped to the used space (files can change between the scan and the disk
/// reading). A sliver stays visible when Guidy holds anything at all.
pub fn bar_fractions(guidy_bytes: u64, disk: &DiskSpace) -> BarFractions {
    if disk.total_bytes == 0 {
        return BarFractions {
            guidy: 0.,
            other_used: 0.,
            free: 1.,
        };
    }
    let total = disk.total_bytes as f64;
    let used_bytes = disk.used_bytes();
    let guidy_bytes = guidy_bytes.min(used_bytes);
    let mut guidy = (guidy_bytes as f64 / total) as f32;
    if guidy_bytes > 0 {
        guidy = guidy.max(MIN_GUIDY_FRACTION);
    }
    let used = ((used_bytes as f64 / total) as f32)
        .clamp(0., 1.)
        .max(guidy);
    BarFractions {
        guidy,
        other_used: used - guidy,
        free: 1. - used,
    }
}

/// The rows to show: largest first, empty buckets left out. Ties keep the
/// report's order, so a refresh doesn't shuffle equal rows.
pub fn ordered_rows(buckets: &[BucketUsage]) -> Vec<BucketUsage> {
    let mut rows: Vec<BucketUsage> = buckets.iter().copied().filter(|b| b.bytes > 0).collect();
    rows.sort_by_key(|b| std::cmp::Reverse(b.bytes));
    rows
}

/// `format_bytes`, except that nothing reads as nothing.
pub fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        "0 KB".into()
    } else {
        format_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guidy_core::storage::Category;

    fn disk(total_bytes: u64, free_bytes: u64) -> DiskSpace {
        DiskSpace {
            total_bytes,
            free_bytes,
        }
    }

    fn assert_near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.0001, "{a} != {b}");
    }

    fn assert_whole(f: BarFractions) {
        assert_near(f.guidy + f.other_used + f.free, 1.);
        assert!(f.guidy >= 0. && f.other_used >= 0. && f.free >= 0., "{f:?}");
    }

    #[test]
    fn fractions_divide_the_disk() {
        let f = bar_fractions(100, &disk(1000, 400));
        assert_near(f.guidy, 0.1);
        assert_near(f.other_used, 0.5);
        assert_near(f.free, 0.4);
        assert_whole(f);
    }

    #[test]
    fn guidy_clamps_to_used() {
        let f = bar_fractions(5000, &disk(1000, 400));
        assert_near(f.guidy, 0.6);
        assert_near(f.other_used, 0.);
        assert_near(f.free, 0.4);
        assert_whole(f);
    }

    #[test]
    fn tiny_share_keeps_a_sliver() {
        let f = bar_fractions(1, &disk(1_000_000_000_000, 500_000_000_000));
        assert_near(f.guidy, MIN_GUIDY_FRACTION);
        assert_near(f.other_used, 0.5 - MIN_GUIDY_FRACTION);
        assert_near(f.free, 0.5);
        assert_whole(f);
    }

    #[test]
    fn sliver_on_a_nearly_empty_disk_comes_out_of_free() {
        let f = bar_fractions(1, &disk(1_000_000_000_000, 999_999_999_000));
        assert_near(f.guidy, MIN_GUIDY_FRACTION);
        assert_near(f.other_used, 0.);
        assert_near(f.free, 1. - MIN_GUIDY_FRACTION);
        assert_whole(f);
    }

    #[test]
    fn nothing_gets_no_sliver() {
        let f = bar_fractions(0, &disk(1000, 400));
        assert_eq!(f.guidy, 0.);
        assert_near(f.other_used, 0.6);
        assert_whole(f);
        // Nothing used: the clamp leaves no room for Guidy either.
        let f = bar_fractions(100, &disk(1000, 1000));
        assert_eq!(f.guidy, 0.);
        assert_near(f.free, 1.);
        assert_whole(f);
    }

    #[test]
    fn free_above_total_and_empty_disk_stay_in_range() {
        assert_whole(bar_fractions(100, &disk(1000, 5000)));
        let f = bar_fractions(100, &disk(0, 0));
        assert_eq!(f.free, 1.);
        assert_whole(f);
    }

    #[test]
    fn full_disk_leaves_nothing_free() {
        let f = bar_fractions(u64::MAX, &disk(u64::MAX, 0));
        assert_near(f.guidy, 1.);
        assert_near(f.free, 0.);
        assert_whole(f);
    }

    #[test]
    fn rows_sort_largest_first_without_empties() {
        let row = |category, bytes| BucketUsage { category, bytes };
        let rows = ordered_rows(&[
            row(Category::Models, 10),
            row(Category::SharedModels, 0),
            row(Category::Logs, 30),
            row(Category::Cache, 10),
            row(Category::Other, 20),
        ]);
        assert_eq!(
            rows.iter().map(|r| r.category).collect::<Vec<_>>(),
            [
                Category::Logs,
                Category::Other,
                Category::Models,
                Category::Cache
            ]
        );
    }

    #[test]
    fn sizes_format() {
        assert_eq!(format_size(0), "0 KB");
        assert_eq!(format_size(1), "1 KB");
        assert_eq!(format_size(2_500_000_000), "2.5 GB");
    }
}
