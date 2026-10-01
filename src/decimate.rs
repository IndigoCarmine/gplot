//! Reduces a series to roughly what the screen can show, so huge files don't
//! blow past the GPU's vertex/index buffer limits.

/// True if x is finite everywhere and non-decreasing, i.e. rows are sorted by
/// x and can be binary-searched and bucketed by pixel column.
pub fn is_monotonic(xs: impl Iterator<Item = f64>) -> bool {
    let mut prev = f64::NEG_INFINITY;
    for x in xs {
        if !x.is_finite() || x < prev {
            return false;
        }
        prev = x;
    }
    true
}

/// M4 decimation: splits `[x_min, x_max]` into `buckets` pixel columns and keeps
/// the first, min-y, max-y and last point of each, in original order. Drawn as a
/// line this is indistinguishable from the full series. `rows` must be sorted by x.
pub fn m4(rows: &[Vec<f64>], x: usize, y: usize, x_min: f64, x_max: f64, buckets: usize) -> Vec<[f64; 2]> {
    let buckets = buckets.max(1);
    let scale = if x_max > x_min { buckets as f64 / (x_max - x_min) } else { 0.0 };
    let bucket_of = |px: f64| (((px - x_min) * scale) as usize).min(buckets - 1);

    let mut out = Vec::with_capacity(4 * buckets);
    // Indices of first, min, max, last within the current bucket.
    let mut cur: Option<(usize, [usize; 4])> = None;
    let flush = |out: &mut Vec<[f64; 2]>, mut idx: [usize; 4]| {
        idx.sort_unstable();
        let mut last = usize::MAX;
        for i in idx {
            if i != last {
                out.push([rows[i][x], rows[i][y]]);
                last = i;
            }
        }
    };

    for (i, r) in rows.iter().enumerate() {
        if !r[y].is_finite() {
            continue;
        }
        let b = bucket_of(r[x]);
        match &mut cur {
            Some((cb, idx)) if *cb == b => {
                if r[y] < rows[idx[1]][y] {
                    idx[1] = i;
                }
                if r[y] > rows[idx[2]][y] {
                    idx[2] = i;
                }
                idx[3] = i;
            }
            _ => {
                if let Some((_, idx)) = cur {
                    flush(&mut out, idx);
                }
                cur = Some((b, [i; 4]));
            }
        }
    }
    if let Some((_, idx)) = cur {
        flush(&mut out, idx);
    }
    out
}

/// Keeps every n-th row so at most about `max_points` remain. Used when x isn't
/// sorted, where pixel bucketing would reorder the line.
pub fn stride(rows: &[Vec<f64>], x: usize, y: usize, max_points: usize) -> Vec<[f64; 2]> {
    let step = rows.len().div_ceil(max_points.max(1)).max(1);
    rows.iter()
        .step_by(step)
        .map(|r| [r[x], r[y]])
        .filter(|[x, y]| x.is_finite() && y.is_finite())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(n: usize) -> Vec<Vec<f64>> {
        (0..n).map(|i| vec![i as f64, (i as f64 * 0.37).sin() * i as f64]).collect()
    }

    #[test]
    fn monotonic_detection() {
        assert!(is_monotonic([0.0, 1.0, 1.0, 2.0].into_iter()));
        assert!(!is_monotonic([0.0, 2.0, 1.0].into_iter()));
        assert!(!is_monotonic([0.0, f64::NAN, 1.0].into_iter()));
    }

    #[test]
    fn m4_bounds_size_and_keeps_extremes() {
        let rows = series(100_000);
        let out = m4(&rows, 0, 1, 0.0, 99_999.0, 500);
        assert!(out.len() <= 4 * 500);
        let max = rows.iter().map(|r| r[1]).fold(f64::MIN, f64::max);
        let min = rows.iter().map(|r| r[1]).fold(f64::MAX, f64::min);
        assert!(out.iter().any(|p| p[1] == max));
        assert!(out.iter().any(|p| p[1] == min));
        assert_eq!(out.first().unwrap()[0], 0.0);
        assert_eq!(out.last().unwrap()[0], 99_999.0);
        assert!(out.windows(2).all(|w| w[0][0] <= w[1][0]));
    }

    #[test]
    fn m4_skips_non_finite_y() {
        let rows = vec![vec![0.0, 1.0], vec![1.0, f64::NAN], vec![2.0, 3.0]];
        let out = m4(&rows, 0, 1, 0.0, 2.0, 10);
        assert_eq!(out, [[0.0, 1.0], [2.0, 3.0]]);
    }

    #[test]
    fn stride_respects_cap() {
        let rows = series(1_000_001);
        assert!(stride(&rows, 0, 1, 1000).len() <= 1000);
        assert_eq!(stride(&rows[..10], 0, 1, 1000).len(), 10);
    }
}
