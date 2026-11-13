//! Turning a rule's basis-point shares into minor units, exactly and with
//! integers only.
//!
//! - **A share** ([`share_minor`]) is the amount times the basis points over
//!   10000, rounded half away from zero (`-0.5` minor units becomes `-1`,
//!   `0.5` becomes `1`). Because the basis points are `0..=10000`, the
//!   result has the amount's sign and is never beyond it, so it always meets
//!   the schema's `my_share_minor` check. [`Share::WHOLE`] is stored as
//!   `NULL` ("all of it"), not as the amount.
//! - **A split** ([`split_amounts`]) gives each line the amount times its
//!   basis points over 10000, rounded down (in absolute value), then hands
//!   the minor units left over, one each, to the lines with the largest
//!   remainders, ties going to the earlier line (largest-remainder
//!   allocation). The lines sum exactly to the amount and keep its sign.
//!   When a line would get nothing (the amount is too small to give every
//!   line at least one minor unit, or it is zero), there is no split:
//!   [`split_amounts`] returns `None` and the rules leave the row unsplit.
//!
//! Products are computed in `i128`, so no amount overflows.

use super::{Share, SplitTemplate};

const WHOLE: i128 = Share::WHOLE.basis_points() as i128;

/// `share` of `amount_minor`, rounded half away from zero: in
/// `0..=amount_minor` (or `amount_minor..=0` for money out).
pub fn share_of(amount_minor: i64, share: Share) -> i64 {
    let product = i128::from(amount_minor) * i128::from(share.basis_points());
    let quotient = product / WHOLE;
    let remainder = product % WHOLE;
    let rounded = if remainder.abs() * 2 >= WHOLE {
        quotient + product.signum()
    } else {
        quotient
    };
    // |rounded| <= |amount_minor|, since the share is at most the whole.
    i64::try_from(rounded).expect("a share is never beyond its amount")
}

/// What `transaction.my_share_minor` (or a split line's) stores for `share`
/// of `amount_minor`: `None` for [`Share::WHOLE`] ("all of it"), else
/// [`share_of`].
pub fn share_minor(amount_minor: i64, share: Share) -> Option<i64> {
    (share != Share::WHOLE).then(|| share_of(amount_minor, share))
}

/// The line amounts `template` gives `amount_minor`, in the template's line
/// order: each non-zero with the amount's sign, summing exactly to it
/// (largest remainder; ties to the earlier line). `None` when the amount is
/// zero or too small to give every line at least one minor unit.
pub fn split_amounts(amount_minor: i64, template: &SplitTemplate) -> Option<Vec<i64>> {
    if amount_minor == 0 {
        return None;
    }
    let total = i128::from(amount_minor).abs();
    let mut amounts = Vec::with_capacity(template.lines().len());
    let mut remainders = Vec::with_capacity(template.lines().len());
    for line in template.lines() {
        let product = total * i128::from(line.share().basis_points());
        amounts.push(product / WHOLE);
        remainders.push(product % WHOLE);
    }
    // The shares sum to the whole, so what is left is less than the number
    // of lines, and only lines with a remainder can take one.
    let left = total - amounts.iter().sum::<i128>();
    let mut order: Vec<usize> = (0..amounts.len()).collect();
    order.sort_by(|a, b| remainders[*b].cmp(&remainders[*a]).then(a.cmp(b)));
    for index in order.into_iter().take(usize::try_from(left).ok()?) {
        amounts[index] += 1;
    }
    let sign = i128::from(amount_minor.signum());
    amounts
        .into_iter()
        .map(|amount| {
            (amount != 0)
                .then(|| i64::try_from(amount * sign).expect("a line is within its amount"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::CategoryKey;
    use crate::rule::SplitLine;

    fn share(bp: u64) -> Share {
        Share::new(bp).unwrap()
    }

    fn template(shares: &[u64]) -> SplitTemplate {
        SplitTemplate::new(
            shares
                .iter()
                .enumerate()
                .map(|(i, bp)| {
                    SplitLine::new(CategoryKey::new(&format!("c{i}")).unwrap(), share(*bp))
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn a_share_rounds_half_away_from_zero() {
        assert_eq!(share_of(-1_000, share(5_000)), -500);
        assert_eq!(share_of(1_000, share(5_000)), 500);
        // 0.5 minor units: away from zero, both ways.
        assert_eq!(share_of(1, share(5_000)), 1);
        assert_eq!(share_of(-1, share(5_000)), -1);
        assert_eq!(share_of(3, share(5_000)), 2);
        assert_eq!(share_of(-3, share(5_000)), -2);
        // Below a half rounds toward zero.
        assert_eq!(share_of(1, share(4_999)), 0);
        assert_eq!(share_of(-1, share(4_999)), 0);
        assert_eq!(share_of(-999, share(3_333)), -333);
        assert_eq!(share_of(-1_000, share(3_333)), -333);
        assert_eq!(share_of(-1_001, share(6_667)), -667);
        assert_eq!(share_of(0, share(5_000)), 0);
        assert_eq!(share_of(-1_234, share(0)), 0);
        assert_eq!(share_of(-1_234, Share::WHOLE), -1_234);
    }

    #[test]
    fn a_share_stays_within_its_amount_at_the_extremes() {
        for amount in [i64::MIN, i64::MIN + 1, -1, 1, i64::MAX] {
            for bp in [0, 1, 4_999, 5_000, 9_999, 10_000] {
                let minor = share_of(amount, share(bp));
                if amount < 0 {
                    assert!((amount..=0).contains(&minor), "{amount} {bp}");
                } else {
                    assert!((0..=amount).contains(&minor), "{amount} {bp}");
                }
            }
        }
        assert_eq!(share_of(i64::MAX, Share::WHOLE), i64::MAX);
        assert_eq!(share_of(i64::MIN, Share::WHOLE), i64::MIN);
    }

    #[test]
    fn the_whole_share_is_stored_as_all_of_it() {
        assert_eq!(share_minor(-1_000, Share::WHOLE), None);
        assert_eq!(share_minor(-1_000, share(2_500)), Some(-250));
        assert_eq!(share_minor(-1_000, share(0)), Some(0));
    }

    #[test]
    fn split_lines_sum_exactly_with_the_amounts_sign() {
        assert_eq!(
            split_amounts(-1_000, &template(&[6_000, 4_000])),
            Some(vec![-600, -400])
        );
        assert_eq!(
            split_amounts(1_000, &template(&[6_000, 4_000])),
            Some(vec![600, 400])
        );
        // A third each: the left-over unit goes to the first line.
        assert_eq!(
            split_amounts(-1_000, &template(&[3_334, 3_333, 3_333])),
            Some(vec![-334, -333, -333])
        );
        assert_eq!(
            split_amounts(-100, &template(&[3_333, 3_333, 3_334])),
            Some(vec![-33, -33, -34])
        );
        assert_eq!(
            split_amounts(-2, &template(&[3_333, 3_333, 3_334])),
            None,
            "two units can't make three lines"
        );
        // Equal remainders: the earlier line takes the unit.
        assert_eq!(
            split_amounts(5, &template(&[5_000, 5_000])),
            Some(vec![3, 2])
        );
        assert_eq!(
            split_amounts(-5, &template(&[5_000, 5_000])),
            Some(vec![-3, -2])
        );
        // The largest remainder wins over line order.
        assert_eq!(
            split_amounts(7, &template(&[1_000, 9_000])),
            Some(vec![1, 6])
        );
        assert_eq!(
            split_amounts(-1_001, &template(&[2_500, 2_500, 5_000])),
            Some(vec![-250, -250, -501])
        );
    }

    #[test]
    fn too_small_or_zero_amounts_dont_split() {
        assert_eq!(split_amounts(0, &template(&[5_000, 5_000])), None);
        assert_eq!(split_amounts(1, &template(&[5_000, 5_000])), None);
        assert_eq!(split_amounts(-1, &template(&[5_000, 5_000])), None);
        assert_eq!(
            split_amounts(-3, &template(&[3_334, 3_333, 3_333])),
            Some(vec![-1, -1, -1])
        );
        // 1% of 50 is 0.5, and 99% is 49.5: equal remainders, so the unit
        // goes to the earlier line.
        assert_eq!(
            split_amounts(-50, &template(&[100, 9_900])),
            Some(vec![-1, -49])
        );
        // 1% of 40 is 0.4, and 99% is 39.6: the unit goes to the larger
        // remainder, leaving the first line empty.
        assert_eq!(split_amounts(-40, &template(&[100, 9_900])), None);
    }

    #[test]
    fn split_lines_sum_exactly_for_many_amounts() {
        let templates = [
            template(&[3_334, 3_333, 3_333]),
            template(&[1, 9_999]),
            template(&[2_500, 2_500, 2_500, 2_500]),
            template(&[6_667, 3_333]),
        ];
        for template in &templates {
            for amount in (-2_000..=2_000).chain([i64::MIN, i64::MAX, i64::MIN + 1]) {
                if let Some(lines) = split_amounts(amount, template) {
                    assert_eq!(
                        lines.iter().map(|l| i128::from(*l)).sum::<i128>(),
                        i128::from(amount)
                    );
                    assert!(
                        lines
                            .iter()
                            .all(|l| *l != 0 && l.signum() == amount.signum())
                    );
                } else {
                    // Only when a line would be empty.
                    assert!(amount.unsigned_abs() < 10_000, "{amount}");
                }
            }
        }
    }
}
