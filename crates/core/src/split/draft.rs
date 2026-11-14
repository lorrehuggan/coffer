//! The split editor's model (ROADMAP M3.9; `docs/DESIGN.md` →
//! "Transactions additions" → Split section and Split states).
//!
//! A [`SplitDraft`] is a split as the user is typing it: the transaction's
//! amount, locked, and lines whose amounts may still be missing. It knows
//! what the editor shows and does, so the view only draws it:
//!
//! - **Remaining** ([`SplitDraft::remaining`]) is the amount less the lines
//!   typed so far, signed: `−£4.00` when a purchase's lines are £4.00 short
//!   of it, `+£3.60` when they are £3.60 over.
//! - **The sign is never typed.** A line takes the transaction's sign
//!   whatever was typed ([`SplitDraft::set_amount`]): a purchase splits into
//!   amounts spent, a refund into amounts back.
//! - **"Add line" fills in the remainder** ([`SplitDraft::add_line`]) when
//!   the lines are short, so the common two-way split is one action.
//! - **Only a balanced split is saved** ([`SplitDraft::status`],
//!   [`SplitDraft::to_lines`]): two or more lines, none zero or missing,
//!   summing exactly to the amount ([`super::validate`]). Until then the
//!   stored split stays as it was.
//! - **Starting a split** ([`SplitDraft::start`]) gives one line with the
//!   whole amount in the transaction's category (user, 2026-11-14): lower
//!   it, then "Add line" takes the rest.
//!
//! A line keeps the share, share label and note it was stored with: the
//! editor doesn't show them yet (M3.11), and saving mustn't drop them.

use crate::category::CategoryKey;
use crate::money::Money;

use super::{Error, SplitLine, StoredSplitLine, validate};

/// One line of a [`SplitDraft`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftLine {
    /// `None` while the amount field is empty or can't be read. Always in
    /// the transaction's currency, with its sign.
    pub amount: Option<Money>,
    /// `None` leaves the line uncategorised.
    pub category: Option<CategoryKey>,
    my_share: Option<Money>,
    share_label: Option<String>,
    user_note: Option<String>,
}

impl DraftLine {
    fn new(amount: Option<Money>, category: Option<CategoryKey>) -> DraftLine {
        DraftLine {
            amount,
            category,
            my_share: None,
            share_label: None,
            user_note: None,
        }
    }
}

/// Where a [`SplitDraft`] stands: what the editor's footer says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftStatus {
    /// The lines split the amount: they can be saved.
    Balanced,
    /// The lines add up but there is only one: a split needs another.
    OneLine,
    /// The lines don't add up to the amount. `over` when they are more
    /// than it (the remainder has the opposite sign to the amount).
    Unbalanced { remaining: Money, over: bool },
    /// Line `line` (from 1) has no amount yet.
    MissingAmount { line: usize },
    /// Line `line` (from 1) is zero.
    ZeroLine { line: usize },
    /// Line `line`'s stored share is more than its new amount.
    ShareOutsideLine { line: usize },
    /// The lines add up to more than can be represented.
    Overflow,
}

impl DraftStatus {
    /// Whether the draft can be saved.
    pub fn is_balanced(self) -> bool {
        self == DraftStatus::Balanced
    }
}

/// A split being edited: see the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitDraft {
    parent: Money,
    lines: Vec<DraftLine>,
}

impl SplitDraft {
    /// A new split of a transaction of `parent` in `category`: one line
    /// with the whole amount.
    pub fn start(parent: Money, category: Option<CategoryKey>) -> SplitDraft {
        SplitDraft {
            parent,
            lines: vec![DraftLine::new(Some(parent), category)],
        }
    }

    /// The stored split `lines` of a transaction of `parent`, to edit.
    pub fn from_stored(parent: Money, lines: &[StoredSplitLine]) -> SplitDraft {
        let lines: Vec<SplitLine> = lines
            .iter()
            .map(|line| SplitLine {
                amount: line.amount,
                category: line.category.clone(),
                my_share: line.my_share,
                share_label: line.share_label.clone(),
                user_note: line.user_note.clone(),
            })
            .collect();
        SplitDraft::from_lines(parent, &lines)
    }

    /// The split `lines` of a transaction of `parent`, to edit.
    pub fn from_lines(parent: Money, lines: &[SplitLine]) -> SplitDraft {
        SplitDraft {
            parent,
            lines: lines
                .iter()
                .map(|line| DraftLine {
                    amount: Some(line.amount),
                    category: line.category.clone(),
                    my_share: line.my_share,
                    share_label: line.share_label.clone(),
                    user_note: line.user_note.clone(),
                })
                .collect(),
        }
    }

    /// The transaction's amount, which the lines must sum to.
    pub fn parent(&self) -> Money {
        self.parent
    }

    pub fn lines(&self) -> &[DraftLine] {
        &self.lines
    }

    /// The amount less the lines typed so far (missing ones count as
    /// nothing), signed; `None` when the sum can't be represented.
    pub fn remaining(&self) -> Option<Money> {
        let typed = Money::sum(
            self.parent.currency(),
            self.lines.iter().filter_map(|line| line.amount),
        )
        .ok()?;
        self.parent.checked_sub(typed).ok()
    }

    /// Sets line `index`'s amount, with the transaction's sign whatever
    /// `amount`'s; `None` empties it. An amount that can't take the sign
    /// (`i64::MIN`) empties it too.
    pub fn set_amount(&mut self, index: usize, amount: Option<Money>) {
        if let Some(line) = self.lines.get_mut(index) {
            line.amount = amount.and_then(|amount| signed_like(self.parent, amount));
        }
    }

    pub fn set_category(&mut self, index: usize, category: Option<CategoryKey>) {
        if let Some(line) = self.lines.get_mut(index) {
            line.category = category;
        }
    }

    /// Adds an uncategorised line and returns its index. It starts with
    /// the remainder when the lines are short of the amount, and empty
    /// otherwise.
    pub fn add_line(&mut self) -> usize {
        let amount = self
            .remaining()
            .filter(|rest| !rest.is_zero() && rest.is_negative() == self.parent.is_negative());
        self.lines.push(DraftLine::new(amount, None));
        self.lines.len() - 1
    }

    /// Removes line `index`, if there is one.
    pub fn remove_line(&mut self, index: usize) {
        if index < self.lines.len() {
            self.lines.remove(index);
        }
    }

    /// Where the draft stands. A missing or zero line comes first (it says
    /// which line to fix), then whether the lines add up, then whether
    /// there are enough of them.
    pub fn status(&self) -> DraftStatus {
        for (index, line) in self.lines.iter().enumerate() {
            match line.amount {
                None => return DraftStatus::MissingAmount { line: index + 1 },
                Some(amount) if amount.is_zero() => {
                    return DraftStatus::ZeroLine { line: index + 1 };
                }
                Some(_) => {}
            }
        }
        let Some(remaining) = self.remaining() else {
            return DraftStatus::Overflow;
        };
        if !remaining.is_zero() {
            return DraftStatus::Unbalanced {
                remaining,
                over: remaining.is_negative() != self.parent.is_negative(),
            };
        }
        if self.lines.len() < 2 {
            return DraftStatus::OneLine;
        }
        match validate(self.parent, &self.split_lines()) {
            Ok(()) => DraftStatus::Balanced,
            Err(Error::ShareOutsideLine(line)) => DraftStatus::ShareOutsideLine { line },
            Err(Error::ZeroLine(line)) => DraftStatus::ZeroLine { line },
            Err(Error::TooFewLines(_)) => DraftStatus::OneLine,
            // The remainder is zero and every line has the parent's sign
            // and currency: nothing else can fail.
            Err(_) => DraftStatus::Overflow,
        }
    }

    /// The lines to save, when the draft is balanced.
    pub fn to_lines(&self) -> Option<Vec<SplitLine>> {
        self.status().is_balanced().then(|| self.split_lines())
    }

    fn split_lines(&self) -> Vec<SplitLine> {
        self.lines
            .iter()
            .map(|line| SplitLine {
                amount: line.amount.unwrap_or(Money::new(0, self.parent.currency())),
                category: line.category.clone(),
                my_share: line.my_share,
                share_label: line.share_label.clone(),
                user_note: line.user_note.clone(),
            })
            .collect()
    }
}

/// `amount` in `parent`'s currency with `parent`'s sign.
fn signed_like(parent: Money, amount: Money) -> Option<Money> {
    let size = amount.minor().checked_abs()?;
    let minor = if parent.is_negative() { -size } else { size };
    Some(Money::new(minor, parent.currency()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;

    fn gbp(minor: i64) -> Money {
        Money::new(minor, Currency::GBP)
    }

    fn key(key: &str) -> Option<CategoryKey> {
        Some(CategoryKey::new(key).unwrap())
    }

    #[test]
    fn starts_with_the_whole_amount_on_one_line() {
        let draft = SplitDraft::start(gbp(-5216), key("food.groceries"));
        assert_eq!(draft.lines().len(), 1);
        assert_eq!(draft.lines()[0].amount, Some(gbp(-5216)));
        assert_eq!(draft.lines()[0].category, key("food.groceries"));
        assert_eq!(draft.remaining(), Some(gbp(0)));
        // Nothing to save until there is a second line.
        assert_eq!(draft.status(), DraftStatus::OneLine);
        assert_eq!(draft.to_lines(), None);
    }

    #[test]
    fn add_line_fills_in_the_remainder_so_two_way_is_one_action() {
        let mut draft = SplitDraft::start(gbp(-5216), key("food.groceries"));
        draft.set_amount(0, Some(gbp(-3866)));
        assert_eq!(
            draft.status(),
            DraftStatus::Unbalanced {
                remaining: gbp(-1350),
                over: false
            }
        );
        let added = draft.add_line();
        assert_eq!(added, 1);
        assert_eq!(draft.lines()[1].amount, Some(gbp(-1350)));
        assert_eq!(draft.lines()[1].category, None);
        assert_eq!(draft.status(), DraftStatus::Balanced);
        let lines = draft.to_lines().unwrap();
        assert_eq!(
            lines.iter().map(|line| line.amount).collect::<Vec<_>>(),
            [gbp(-3866), gbp(-1350)]
        );
    }

    #[test]
    fn says_how_far_off_the_lines_are_both_ways() {
        // The states board: lines short by £4.00, then over by £3.60.
        let mut draft = SplitDraft::from_lines(
            gbp(-5216),
            &[
                SplitLine::new(gbp(-3866), key("food.groceries")),
                SplitLine::new(gbp(-950), key("shopping")),
            ],
        );
        assert_eq!(draft.remaining(), Some(gbp(-400)));
        assert_eq!(
            draft.status(),
            DraftStatus::Unbalanced {
                remaining: gbp(-400),
                over: false
            }
        );
        draft.set_amount(1, Some(gbp(-1710)));
        assert_eq!(
            draft.status(),
            DraftStatus::Unbalanced {
                remaining: gbp(360),
                over: true
            }
        );
        // Over: a new line starts empty rather than with a wrong sign.
        let added = draft.add_line();
        assert_eq!(draft.lines()[added].amount, None);
        assert_eq!(draft.status(), DraftStatus::MissingAmount { line: 3 });
        draft.remove_line(added);
        draft.set_amount(1, Some(gbp(-950)));
        draft.add_line();
        assert_eq!(draft.status(), DraftStatus::Balanced);
    }

    #[test]
    fn money_in_splits_into_amounts_back() {
        let mut draft = SplitDraft::start(gbp(3400), key("shopping"));
        draft.set_amount(0, Some(gbp(2000)));
        assert_eq!(
            draft.status(),
            DraftStatus::Unbalanced {
                remaining: gbp(1400),
                over: false
            }
        );
        draft.add_line();
        assert_eq!(draft.lines()[1].amount, Some(gbp(1400)));
        assert!(draft.status().is_balanced());
        draft.set_amount(1, Some(gbp(1500)));
        assert_eq!(
            draft.status(),
            DraftStatus::Unbalanced {
                remaining: gbp(-100),
                over: true
            }
        );
    }

    #[test]
    fn the_sign_is_never_typed() {
        let mut draft = SplitDraft::start(gbp(-5216), None);
        // Typed without a sign, or with the wrong one: the parent's wins.
        draft.set_amount(0, Some(gbp(4000)));
        assert_eq!(draft.lines()[0].amount, Some(gbp(-4000)));
        let mut refund = SplitDraft::start(gbp(3400), None);
        refund.set_amount(0, Some(gbp(-1000)));
        assert_eq!(refund.lines()[0].amount, Some(gbp(1000)));
        // In the parent's currency, whatever the field read.
        draft.set_amount(0, Some(Money::new(-4000, Currency::EUR)));
        assert_eq!(draft.lines()[0].amount, Some(gbp(-4000)));
        // `i64::MIN` has no positive twin: the line empties.
        draft.set_amount(0, Some(gbp(i64::MIN)));
        assert_eq!(draft.lines()[0].amount, None);
    }

    #[test]
    fn a_missing_or_zero_line_is_named_before_the_sum() {
        let mut draft = SplitDraft::start(gbp(-1000), None);
        draft.set_amount(0, None);
        assert_eq!(draft.status(), DraftStatus::MissingAmount { line: 1 });
        assert_eq!(draft.remaining(), Some(gbp(-1000)));
        draft.set_amount(0, Some(gbp(0)));
        assert_eq!(draft.status(), DraftStatus::ZeroLine { line: 1 });
        draft.set_amount(0, Some(gbp(-1000)));
        draft.add_line();
        // Lines that already sum: the new one starts empty.
        assert_eq!(draft.status(), DraftStatus::MissingAmount { line: 2 });
    }

    #[test]
    fn removing_a_line_and_recategorising_keep_the_rest() {
        let mut draft = SplitDraft::from_lines(
            gbp(-5216),
            &[
                SplitLine::new(gbp(-3866), key("food.groceries")),
                SplitLine::new(gbp(-950), key("shopping")),
                SplitLine::new(gbp(-400), None),
            ],
        );
        assert!(draft.status().is_balanced());
        draft.set_category(2, key("other"));
        assert_eq!(draft.lines()[2].category, key("other"));
        draft.remove_line(1);
        assert_eq!(draft.lines().len(), 2);
        assert_eq!(
            draft.status(),
            DraftStatus::Unbalanced {
                remaining: gbp(-950),
                over: false
            }
        );
        // Out of range does nothing.
        draft.remove_line(9);
        draft.set_amount(9, Some(gbp(1)));
        draft.set_category(9, None);
        assert_eq!(draft.lines().len(), 2);
    }

    #[test]
    fn stored_shares_and_notes_survive_an_edit() {
        let stored = [
            StoredSplitLine {
                id: crate::id::SplitId::new(1),
                transaction: crate::id::TransactionId::new(1),
                line_no: 1,
                amount: gbp(-3000),
                category: key("food"),
                my_share: Some(gbp(-1500)),
                share_label: Some("Sam".into()),
                user_note: Some("half".into()),
            },
            StoredSplitLine {
                id: crate::id::SplitId::new(2),
                transaction: crate::id::TransactionId::new(1),
                line_no: 2,
                amount: gbp(-2000),
                category: None,
                my_share: None,
                share_label: None,
                user_note: None,
            },
        ];
        let mut draft = SplitDraft::from_stored(gbp(-5000), &stored);
        let lines = draft.to_lines().unwrap();
        assert_eq!(lines[0].my_share, Some(gbp(-1500)));
        assert_eq!(lines[0].share_label.as_deref(), Some("Sam"));
        assert_eq!(lines[0].user_note.as_deref(), Some("half"));
        // Lowering the line below its share is caught here, not on save.
        draft.set_amount(0, Some(gbp(-1000)));
        draft.set_amount(1, Some(gbp(-4000)));
        assert_eq!(draft.status(), DraftStatus::ShareOutsideLine { line: 1 });
    }
}
