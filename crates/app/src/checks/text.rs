//! What the review queues write: why a pair was flagged, where a row came
//! from, where a balance came from, and the sentences the canvas and the
//! inspector compose (`docs/DESIGN.md` → "Review queues"; the wording the
//! artboards don't show is recorded in `docs/ARCHITECTURE.md` → "Decision
//! record: review queues (built)").
//!
//! Merchant names, bank text and bank IDs are untrusted: they go through
//! [`plain`] before they're drawn. Account names are the user's, and go
//! through it too.

use chrono::NaiveDate;
use coffer_core::change::Field;
use coffer_core::checks::{Added, BankIds, PairRow, Why};
use coffer_core::money::Money;
use coffer_core::reconciliation::BalanceSource;

use crate::format::{self, Sign};
pub use crate::inbox::text::{NAME_SHOWN, Part, count, plain, rich, unsigned};

/// The longest bank text or bank ID shown.
pub const BANK_TEXT_SHOWN: usize = 120;

/// A date in a list or a sentence: `9 Nov`, or `2 Oct 25` in another year.
pub fn date(date: NaiveDate, today: NaiveDate) -> String {
    format::list_date(date, today)
}

/// `−£42.50`, `+£12.00`.
pub fn signed(amount: Money) -> String {
    format::money(amount, Sign::Always)
}

/// A balance: `−£24.10`, `£18.40`.
pub fn balance(amount: Money) -> String {
    format::money(amount, Sign::Negative)
}

/// A row's name, plain: the display merchant, else the bank text.
pub fn name(row: &PairRow) -> String {
    plain(row.name(), NAME_SHOWN)
}

/// "Same day", "1 day apart", "3 days apart".
pub fn gap(days: u32) -> String {
    match days {
        0 => "Same day".to_owned(),
        1 => "1 day apart".to_owned(),
        n => format!("{n} days apart"),
    }
}

/// The WHY FLAGGED lane: "1 day apart, text differs".
pub fn why(why: &Why) -> String {
    let difference = if !why.same_text {
        "text differs"
    } else if why.manual {
        "one added by hand"
    } else {
        match why.bank_ids {
            BankIds::Different => "bank IDs differ",
            _ => "bank ID on one only",
        }
    };
    format!("{}, {difference}", gap(why.days_apart))
}

/// The criteria that kept the exact dedupe from matching the pair, for
/// the inspector's ≠ rows.
pub fn differences(why: &Why) -> Vec<&'static str> {
    let mut found = Vec::new();
    if !why.same_text {
        found.push("Different text");
    }
    if why.manual {
        found.push("One added by hand");
    }
    match why.bank_ids {
        BankIds::OneOnly if !why.manual => found.push("Bank ID on one only"),
        BankIds::Different => found.push("Different bank IDs"),
        _ => {}
    }
    found
}

/// The caption after the criteria: "An exact match is skipped at import.
/// These two differ in their text and only one has the bank's ID, so
/// Coffer can't be sure and asks you."
pub fn why_caption(why: &Why) -> String {
    let mut reasons = Vec::new();
    if !why.same_text {
        reasons.push("differ in their text");
    }
    if why.manual {
        reasons.push("one was added by hand");
    } else {
        match why.bank_ids {
            BankIds::OneOnly => reasons.push("only one has the bank’s ID"),
            BankIds::Different => reasons.push("have different bank IDs"),
            _ => {}
        }
    }
    if reasons.is_empty() {
        return "An exact match is skipped at import. Coffer can’t be sure these two are one \
                payment, so it asks you."
            .to_owned();
    }
    format!(
        "An exact match is skipped at import. These two {}, so Coffer can’t be sure and asks you.",
        join(&reasons)
    )
}

/// "a", "a and b", "a, b and c".
pub fn join(items: &[impl AsRef<str>]) -> String {
    match items {
        [] => String::new(),
        [one] => one.as_ref().to_owned(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

/// Where a row came from, under its column's heading: "Imported, with the
/// bank's ID", "Added by you on 5 Nov".
pub fn added(row: &PairRow, today: NaiveDate) -> String {
    match &row.added {
        Added::Imported { .. } if row.bank_id.is_some() => {
            "Imported, with the bank’s ID".to_owned()
        }
        Added::Imported { file, .. } if !file.is_empty() => {
            format!("Imported from {}", plain(file, NAME_SHOWN))
        }
        Added::Imported { .. } => "Imported, without a bank ID".to_owned(),
        Added::Mirrored => "From a pot transfer in an import".to_owned(),
        Added::Manual { on } => format!("Added by you on {}", date(*on, today)),
    }
}

/// The values a merge moves to the kept row, for the "Moves" row:
/// "Category and note".
pub fn carried(fields: &[Field]) -> String {
    let words: Vec<&str> = fields
        .iter()
        .filter_map(|field| match field {
            Field::Category => Some("category"),
            Field::Split => Some("split"),
            Field::Share => Some("share"),
            Field::Merchant => Some("name"),
            Field::UserNote => Some("note"),
            Field::Tags => Some("tags"),
            Field::Reimbursable => Some("claim"),
            _ => None,
        })
        .collect();
    let mut text = join(&words);
    if let Some(first) = text.get(..1) {
        text = first.to_uppercase() + &text[1..];
    }
    text
}

/// A balance's source in the table: "Entered by you", "Statement (OFX)".
pub fn source(source: BalanceSource) -> &'static str {
    match source {
        BalanceSource::Opening => "Opening balance",
        BalanceSource::Manual => "Entered by you",
        BalanceSource::Statement => "Statement",
        BalanceSource::Ofx => "Statement (OFX)",
        BalanceSource::Csv => "Statement (CSV)",
        BalanceSource::Valuation => "Valuation",
    }
}

/// A balance's source after its amount: "£18.40 entered by you".
pub fn source_after(source: BalanceSource) -> &'static str {
    match source {
        BalanceSource::Opening => "your opening balance",
        BalanceSource::Manual => "entered by you",
        BalanceSource::Statement => "from a statement",
        BalanceSource::Ofx => "from an OFX statement",
        BalanceSource::Csv => "from a CSV file",
        BalanceSource::Valuation => "a valuation",
    }
}

/// Who stated a balance, in a sentence: "The balance you entered for Joint
/// account on 9 Nov".
pub fn stated_by(source: BalanceSource) -> &'static str {
    match source {
        BalanceSource::Opening | BalanceSource::Manual => "The balance you entered",
        BalanceSource::Statement => "The statement balance",
        BalanceSource::Ofx | BalanceSource::Csv => "The balance in your import",
        BalanceSource::Valuation => "The valuation",
    }
}

/// The "Not checked" caption: "Not checked: Amex has no balance yet, and
/// Stocks ISA is valued by you. A valuation sets the balance rather than
/// checking it. Pots follow their transfers." `None` when everything is
/// checked.
pub fn not_checked(no_balance: &[String], valued: &[String], pots: usize) -> Option<String> {
    let names = |names: &[String]| {
        join(
            &names
                .iter()
                .map(|name| plain(name, NAME_SHOWN))
                .collect::<Vec<_>>(),
        )
    };
    let mut clauses = Vec::new();
    if !no_balance.is_empty() {
        let verb = if no_balance.len() == 1 { "has" } else { "have" };
        clauses.push(format!("{} {verb} no balance yet", names(no_balance)));
    }
    if !valued.is_empty() {
        let verb = if valued.len() == 1 { "is" } else { "are" };
        clauses.push(format!("{} {verb} valued by you", names(valued)));
    }
    let mut text = match clauses.as_slice() {
        [] => String::new(),
        [one] => format!("Not checked: {one}."),
        [first, second] => format!("Not checked: {first}, and {second}."),
        _ => unreachable!("two clauses at most"),
    };
    if !valued.is_empty() {
        text.push_str(" A valuation sets the balance rather than checking it.");
    }
    if pots > 0 {
        if text.is_empty() {
            text.push_str("Not checked: pots follow their transfers.");
        } else {
            text.push_str(" Pots follow their transfers.");
        }
    }
    (!text.is_empty()).then(|| text.trim_start().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use coffer_core::checks::Added;
    use coffer_core::duplicate::DuplicateWindow;
    use coffer_core::money::Currency;

    fn why(days: u32, same_text: bool, bank_ids: BankIds, manual: bool) -> Why {
        Why {
            days_apart: days,
            window: DuplicateWindow::DEFAULT,
            same_text,
            bank_ids,
            manual,
        }
    }

    #[test]
    fn why_flagged_reads_like_the_artboard() {
        assert_eq!(
            super::why(&why(1, false, BankIds::OneOnly, true)),
            "1 day apart, text differs"
        );
        assert_eq!(
            super::why(&why(0, false, BankIds::Different, false)),
            "Same day, text differs"
        );
        assert_eq!(
            super::why(&why(2, true, BankIds::OneOnly, true)),
            "2 days apart, one added by hand"
        );
        assert_eq!(
            differences(&why(1, false, BankIds::OneOnly, false)),
            ["Different text", "Bank ID on one only"]
        );
        assert_eq!(
            why_caption(&why(1, false, BankIds::OneOnly, false)),
            "An exact match is skipped at import. These two differ in their text and only one \
             has the bank’s ID, so Coffer can’t be sure and asks you."
        );
    }

    #[test]
    fn lists_join_with_and() {
        assert_eq!(join(&[] as &[&str]), "");
        assert_eq!(join(&["a"]), "a");
        assert_eq!(join(&["a", "b"]), "a and b");
        assert_eq!(join(&["a", "b", "c"]), "a, b and c");
        assert_eq!(
            carried(&[Field::Category, Field::UserNote]),
            "Category and note"
        );
    }

    #[test]
    fn not_checked_names_each_kind_of_account() {
        assert_eq!(
            not_checked(&["Amex".to_owned()], &["Stocks ISA".to_owned()], 2).as_deref(),
            Some(
                "Not checked: Amex has no balance yet, and Stocks ISA is valued by you. A \
                 valuation sets the balance rather than checking it. Pots follow their transfers."
            )
        );
        assert_eq!(not_checked(&[], &[], 0), None);
        assert_eq!(
            not_checked(&[], &[], 1).as_deref(),
            Some("Not checked: pots follow their transfers.")
        );
    }

    #[test]
    fn a_row_says_where_it_came_from() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let mut row = PairRow {
            transaction: coffer_core::id::TransactionId::new(1),
            date: today,
            merchant: Some("Ocado\u{202e}".to_owned()),
            amount: Money::new(-4_250, Currency::GBP),
            category: coffer_core::checks::RowCategory::Uncategorised,
            bank_text: "OCADO".to_owned(),
            bank_id: Some("tx_1".to_owned()),
            added: Added::Imported {
                file: "joint.ofx".to_owned(),
                on: today,
            },
        };
        assert_eq!(added(&row, today), "Imported, with the bank’s ID");
        // Untrusted text is stripped of bidi controls.
        assert_eq!(name(&row), "Ocado");
        row.bank_id = None;
        assert_eq!(added(&row, today), "Imported from joint.ofx");
        row.added = Added::Manual {
            on: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
        };
        assert_eq!(added(&row, today), "Added by you on 5 Nov");
    }
}
