//! Transaction rows: `search_transactions`, `uncategorized_transactions`,
//! `merchant_history` (ROADMAP M2.2, M2.14).
//!
//! Every query reads [`Ledger`] lines (so a split purchase is one row per
//! line with its parent's ID, and a confirmed refund is a row of its own
//! with its purchase's category), newest first, capped at a [`Limit`], with
//! each line's [`BankText`](super::ledger::BankText) so a tool can match it
//! or hand it on as untrusted text.
//!
//! Two helpers decide what of a row may leave the app:
//!
//! - [`public_tags`]: the public tag names of some transactions. Private
//!   tags never appear.
//! - [`has_account_identifier`]: whether text holds a sort code, an account
//!   or card number, or an IBAN. A row tool withholds such text (a
//!   display merchant derived from a transfer's bank text can be the whole
//!   text, account details included). It can't see a person's name: a
//!   payee who is a person still goes out under their name.
//!
//! Nothing here logs transaction text.

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::params_from_iter;

use crate::category::CategoryKey;
use crate::db::Reader;
use crate::id::{AccountId, TransactionId};
use crate::tag::TagName;

use super::{Capped, DateRange, Error, Ledger, LedgerLine, Limit, Totals};

/// The earliest and latest dates a stored date can have (`YYYY-MM-DD`,
/// four-digit years): the open end of a half-open search.
const FIRST_DAY: NaiveDate = NaiveDate::from_ymd_opt(1, 1, 1).expect("a valid date");
const LAST_DAY: NaiveDate = NaiveDate::from_ymd_opt(9999, 12, 31).expect("a valid date");

/// What `search_transactions` looks for. Every filter is optional and they
/// AND together; several values of one filter match any of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    /// Text the display merchant, bank merchant or raw description
    /// contains, ignoring ASCII case. Only ever matched.
    pub text: Option<String>,
    /// The first day, inclusive. Without `to`, every later day.
    pub from: Option<NaiveDate>,
    /// The last day, inclusive. Without `from`, every earlier day.
    pub to: Option<NaiveDate>,
    pub accounts: Vec<AccountId>,
    /// A group's key includes its subcategories.
    pub categories: Vec<CategoryKey>,
    /// Public tags only: a private one is [`Error::UnknownTag`].
    pub tags: Vec<TagName>,
    /// The smallest whole line amount, signed minor units.
    pub amount_min: Option<i64>,
    /// The largest whole line amount, signed minor units.
    pub amount_max: Option<i64>,
}

impl Search {
    /// The dates as a range, `None` when neither end is set. One end alone
    /// is open on the other side.
    pub fn range(&self) -> Result<Option<DateRange>, Error> {
        match (self.from, self.to) {
            (None, None) => Ok(None),
            (from, to) => {
                DateRange::new(from.unwrap_or(FIRST_DAY), to.unwrap_or(LAST_DAY)).map(Some)
            }
        }
    }

    fn ledger(&self) -> Result<Ledger, Error> {
        let mut ledger = rows()
            .accounts(self.accounts.iter().copied())
            .categories(self.categories.iter().cloned())
            .tags(self.tags.iter().cloned())
            .amount_between(self.amount_min, self.amount_max);
        if let Some(range) = self.range()? {
            ledger = ledger.within(range);
        }
        if let Some(text) = &self.text {
            ledger = ledger.matching(text);
        }
        Ok(ledger)
    }
}

/// The lines matching `search`, newest first.
pub fn search(reader: &Reader, search: &Search, limit: Limit) -> Result<Capped<LedgerLine>, Error> {
    search.ledger()?.lines_capped(reader, limit)
}

/// The uncategorised lines, newest first (a settlement's part has no
/// category by design and isn't one of them).
pub fn uncategorized(reader: &Reader, limit: Limit) -> Result<Capped<LedgerLine>, Error> {
    rows().uncategorized().lines_capped(reader, limit)
}

/// A merchant's lines and a summary of all of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerchantHistory {
    /// The newest lines, up to the limit.
    pub lines: Capped<LedgerLine>,
    /// Over every matching line, not only those in `lines`.
    pub summary: MerchantSummary,
}

/// What a merchant's whole history adds up to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MerchantSummary {
    /// How many ledger lines matched.
    pub lines: usize,
    /// How many transactions those lines belong to.
    pub transactions: usize,
    /// The user's share ([`LedgerLine::personal`]) summed per currency.
    pub personal: Totals,
    /// The earliest line's date; `None` when nothing matched.
    pub first: Option<NaiveDate>,
    /// The latest line's date; `None` when nothing matched.
    pub last: Option<NaiveDate>,
}

/// The lines whose display merchant or bank merchant is `name` (ignoring
/// ASCII case and surrounding whitespace), newest first, and a summary of
/// all of them. A blank name matches nothing.
pub fn merchant_history(
    reader: &Reader,
    name: &str,
    limit: Limit,
) -> Result<MerchantHistory, Error> {
    if name.trim().is_empty() {
        return Ok(MerchantHistory {
            lines: Capped::new(Vec::new(), limit),
            summary: MerchantSummary::default(),
        });
    }
    let lines = rows().merchant(name).lines(reader)?;
    let mut summary = MerchantSummary {
        lines: lines.len(),
        ..MerchantSummary::default()
    };
    let mut transactions = Vec::with_capacity(lines.len());
    for line in &lines {
        summary.personal.add(line.personal)?;
        summary.first = Some(
            summary
                .first
                .map_or(line.date, |first| first.min(line.date)),
        );
        summary.last = Some(summary.last.map_or(line.date, |last| last.max(line.date)));
        transactions.push(line.transaction_id);
    }
    transactions.sort_unstable();
    transactions.dedup();
    summary.transactions = transactions.len();
    Ok(MerchantHistory {
        lines: Capped::new(lines, limit),
        summary,
    })
}

/// Every row query's base: newest first, with bank text.
fn rows() -> Ledger {
    Ledger::new().newest_first().with_bank_text()
}

/// The public tag names of each of `transactions` that has any, sorted
/// ignoring ASCII case. Private tags never appear.
pub fn public_tags(
    reader: &Reader,
    transactions: &[TransactionId],
) -> Result<HashMap<TransactionId, Vec<String>>, Error> {
    let mut ids: Vec<TransactionId> = transactions.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut tags: HashMap<TransactionId, Vec<String>> = HashMap::new();
    // SQLite allows 32766 parameters; stay well under it.
    for chunk in ids.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(", ");
        let mut statement = reader.conn().prepare(&format!(
            "SELECT tt.transaction_id, tag.name FROM transaction_tag tt \
               JOIN tag ON tag.id = tt.tag_id \
              WHERE tag.private = 0 AND tt.transaction_id IN ({marks})"
        ))?;
        let rows = statement.query_map(params_from_iter(chunk), |row| {
            Ok((row.get::<_, TransactionId>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (transaction, name) = row?;
            tags.entry(transaction).or_default().push(name);
        }
    }
    for names in tags.values_mut() {
        names.sort_by(|a, b| {
            a.to_ascii_lowercase()
                .cmp(&b.to_ascii_lowercase())
                .then_with(|| a.cmp(b))
        });
    }
    Ok(tags)
}

/// Whether `text` holds something that identifies a bank account:
///
/// - a UK sort code: three pairs of digits joined by the same `-` or
///   space (`12-34-56`, `12 34 56`), not part of a longer number (so the
///   `26-11-20` inside `2026-11-20` isn't one);
/// - a run of eight or more digits: an account or card number;
/// - an IBAN: two letters, two check digits, then 11–30 letters and
///   digits (at least four of them digits), written together or in groups
///   of four separated by single spaces.
///
/// Short references (`*1234`), amounts and `YYYY-MM-DD` dates don't count.
/// Pure, so the app can show the same rows as withheld. It can't recognise
/// a person's name.
pub fn has_account_identifier(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    has_long_digit_run(&chars) || has_sort_code(&chars) || has_iban(&chars)
}

/// The fewest digits in a row that count as an account or card number.
const ACCOUNT_NUMBER_DIGITS: usize = 8;

fn has_long_digit_run(chars: &[char]) -> bool {
    chars
        .split(|c| !c.is_ascii_digit())
        .any(|run| run.len() >= ACCOUNT_NUMBER_DIGITS)
}

fn has_sort_code(chars: &[char]) -> bool {
    let digit = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    (0..chars.len()).any(|i| {
        let separator = match chars.get(i + 2) {
            Some(&c @ ('-' | ' ')) => c,
            _ => return false,
        };
        (i == 0 || !digit(i - 1))
            && digit(i)
            && digit(i + 1)
            && digit(i + 3)
            && digit(i + 4)
            && chars.get(i + 5) == Some(&separator)
            && digit(i + 6)
            && digit(i + 7)
            && !digit(i + 8)
    })
}

/// An IBAN's shortest and longest part after the check digits.
const IBAN_BBAN_MIN: usize = 11;
const IBAN_BBAN_MAX: usize = 30;
/// The fewest digits in that part: every country's has an account number.
const IBAN_BBAN_MIN_DIGITS: usize = 4;

fn has_iban(chars: &[char]) -> bool {
    let alnum = |i: usize| chars.get(i).is_some_and(char::is_ascii_alphanumeric);
    (0..chars.len()).any(|i| {
        let start = (i == 0 || !alnum(i - 1))
            && chars[i].is_ascii_alphabetic()
            && chars.get(i + 1).is_some_and(char::is_ascii_alphabetic)
            && chars.get(i + 2).is_some_and(char::is_ascii_digit)
            && chars.get(i + 3).is_some_and(char::is_ascii_digit);
        start && iban_tail(chars, i + 4)
    })
}

/// Whether the part after an IBAN's check digits, from `at`, looks like
/// one: written together, or in groups of four after single spaces.
fn iban_tail(chars: &[char], at: usize) -> bool {
    let run = |from: usize| -> &[char] {
        let from = from.min(chars.len());
        let len = chars[from..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        &chars[from..from + len]
    };
    let digits = |part: &[char]| part.iter().filter(|c| c.is_ascii_digit()).count();

    // Together: `GB94BARC10201530093459`.
    let together = run(at);
    if !together.is_empty() {
        return (IBAN_BBAN_MIN..=IBAN_BBAN_MAX).contains(&together.len())
            && digits(together) >= IBAN_BBAN_MIN_DIGITS;
    }

    // Grouped: `GB94 BARC 1020 1530 0934 59`. Every group but the last
    // has four characters; text after the last can't add to it.
    let (mut len, mut digit_count, mut next) = (0, 0, at);
    while chars.get(next) == Some(&' ') {
        let group = run(next + 1);
        if group.is_empty() || group.len() > 4 || len + group.len() > IBAN_BBAN_MAX {
            break;
        }
        len += group.len();
        digit_count += digits(group);
        if len >= IBAN_BBAN_MIN && digit_count >= IBAN_BBAN_MIN_DIGITS {
            return true;
        }
        if group.len() < 4 {
            break;
        }
        next += 1 + group.len();
    }
    false
}

#[cfg(test)]
mod tests;
