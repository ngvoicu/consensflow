//! `Array.prototype.sort` with a comparator, as a reader's JavaScript sorted
//! its items: stable, an item before another only where the comparator says
//! less than 0 (`NaN` and 0 say neither), and a comparator that throws fails
//! the sort.
//!
//! Every stable sort orders items alike by a comparator that is an order, so
//! this one orders them as V8's TimSort does. Kept from Node on purpose: it
//! compares other pairs than TimSort does, so by a comparator that is no
//! order (its numbers `NaN`) the order may be another, and by one that fails
//! on some pairs alone whether the sort fails, and on which pair, may differ.
//! Rust's own sort is not used: it may panic on a comparator that is no order.

/// `items` sorted stably by `compare`, a JavaScript comparator's number,
/// or the comparator's failure.
pub(crate) fn sort<T>(
    items: Vec<T>,
    mut compare: impl FnMut(&T, &T) -> Result<f64, String>,
) -> Result<Vec<T>, String> {
    let length = items.len();
    let mut order: Vec<usize> = (0..length).collect();
    let mut merged = order.clone();
    let mut width = 1;
    while width < length {
        let mut start = 0;
        while start < length {
            let middle = (start + width).min(length);
            let end = (start + 2 * width).min(length);
            let (mut left, mut right, mut out) = (start, middle, start);
            while left < middle && right < end {
                // The right one first only where it is less: equals keep their order.
                if compare(&items[order[right]], &items[order[left]])? < 0.0 {
                    merged[out] = order[right];
                    right += 1;
                } else {
                    merged[out] = order[left];
                    left += 1;
                }
                out += 1;
            }
            let rest = middle - left;
            merged[out..out + rest].copy_from_slice(&order[left..middle]);
            merged[out + rest..end].copy_from_slice(&order[right..end]);
            start = end;
        }
        std::mem::swap(&mut order, &mut merged);
        width *= 2;
    }
    let mut taken: Vec<Option<T>> = items.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .filter_map(|at| taken[at].take())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_sorted_stably_by_the_comparator_s_sign() {
        let items: Vec<(i32, char)> = vec![(3, 'a'), (1, 'b'), (3, 'c'), (2, 'd'), (1, 'e')];
        let sorted = sort(items, |left, right| Ok(f64::from(left.0 - right.0))).unwrap();
        assert_eq!(sorted, [(1, 'b'), (1, 'e'), (2, 'd'), (3, 'a'), (3, 'c')]);
        let empty: Vec<i32> = Vec::new();
        assert!(sort(empty, |_, _| Err("never asked".to_owned()))
            .unwrap()
            .is_empty());
        // One item is never compared.
        assert_eq!(
            sort(vec![1], |_, _| Err("never asked".to_owned())).unwrap(),
            [1]
        );
    }

    #[test]
    fn nan_says_neither_and_a_failing_comparator_fails_the_sort() {
        let sorted = sort(vec![2, 1, 3], |_, _| Ok(f64::NAN)).unwrap();
        assert_eq!(sorted, [2, 1, 3], "NaN keeps every item in its place");
        let failed = sort(vec![2, 1], |left, _| {
            if *left == 1 {
                Err("one".to_owned())
            } else {
                Ok(0.0)
            }
        });
        assert_eq!(failed.unwrap_err(), "one");
        // Many items: the same as a stable sort's.
        let items: Vec<(u32, usize)> = (0..1000_usize)
            .map(|at| (u32::try_from(at * 7919 % 13).unwrap(), at))
            .collect();
        let mut expected = items.clone();
        expected.sort_by_key(|item| item.0);
        assert_eq!(
            sort(items, |left, right| Ok(
                f64::from(left.0) - f64::from(right.0)
            ))
            .unwrap(),
            expected
        );
    }
}
