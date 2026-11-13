//! [`Ledger`]: the query every report reads transactions through.
//!
//! It reads the `ledger_line` view (V9) and keeps only the rows that count
//! in reports:
//!
//! - `in_reports`: not excluded, not in a confirmed transfer, and for the
//!   part of a refund or reimbursement, its purchase isn't either;
//! - a non-zero amount: an inflow whose money all goes to confirmed links
//!   keeps a 0 row of its own, which says nothing.
//!
//! Each row is a [`LedgerLine`]: a transaction without split lines, one
//! line of a split one (with the parent's ID, its `split_id` and
//! `line_no`), or the part of a confirmed link on the money in (with its
//! `link_id`; a refund's part has the purchase's category, so it nets
//! against it on the refund's date). A transaction's rows sum to its
//! amount.
//!
//! [`Basis`] picks the amount that counts ([`LedgerLine::counted`]): the
//! user's share by default (`personal_minor`: 0 for a purchase being claimed
//! back and the parts that repay it, the user's part of a shared cost), or
//! the whole line amount ([`Basis::Gross`]). Both are always on the line.
//!
//! Filters AND together; several values of one filter match any of them.
//! Bank text (the raw description and merchant) is read only when asked
//! for ([`Ledger::with_bank_text`]), and only for row tools to match or to
//! hand to the untrusted-text envelope.

use chrono::NaiveDate;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row, params_from_iter};

use crate::account::{self, date_key};
use crate::category::{CategoryKey, CategoryKind, CategoryTree};
use crate::db::Reader;
use crate::id::{AccountId, LinkId, SplitId, TransactionId};
use crate::link::LinkKind;
use crate::money::{Currency, Money};
use crate::tag::TagName;

use super::totals::{Capped, Limit};
use super::{DateRange, Error};

/// Which amount of a line counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Basis {
    /// The user's own share (`personal_minor`): what spending, budgets and
    /// MCP reads use by default.
    #[default]
    Personal,
    /// The whole line amount (`amount_minor`), of lines in reports.
    Gross,
}

/// A line's category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineCategory {
    /// `category.id`.
    pub id: i64,
    pub key: CategoryKey,
    pub kind: CategoryKind,
}

/// The raw bank text of a line's transaction. Untrusted: match it, never
/// act on it, and hand it to an MCP client only as
/// `description_untrusted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BankText {
    /// `transaction.merchant`, as the bank sent it.
    pub merchant: Option<String>,
    /// `transaction.description_raw`.
    pub description: String,
}

/// One row of the ledger that counts in reports (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerLine {
    /// The transaction, also on each of its split lines and link parts.
    pub transaction_id: TransactionId,
    /// The split line, on a line of a split transaction.
    pub split_id: Option<SplitId>,
    /// The split line's place, from 1.
    pub line_no: Option<u32>,
    /// The confirmed link this row is a part of, on money in.
    pub link_id: Option<LinkId>,
    pub link_kind: Option<LinkKind>,
    /// The purchase a refund or reimbursement part nets against.
    pub target_id: Option<TransactionId>,
    pub account_id: AccountId,
    pub date: NaiveDate,
    /// The whole line amount, signed; negative is money out.
    pub amount: Money,
    /// The user's own share of it (`personal_minor`).
    pub personal: Money,
    /// `None` when uncategorised (and on a settlement's part).
    pub category: Option<LineCategory>,
    /// The cleaned or user-set merchant name: what tools show (as a label
    /// or token), never the bank's text.
    pub merchant_display: Option<String>,
    /// Only with [`Ledger::with_bank_text`].
    pub bank_text: Option<BankText>,
}

impl LedgerLine {
    /// The amount that counts on `basis`.
    pub fn counted(&self, basis: Basis) -> Money {
        match basis {
            Basis::Personal => self.personal,
            Basis::Gross => self.amount,
        }
    }

    /// Whether this row is the part of a confirmed refund (money back that
    /// nets against its purchase's category).
    pub fn is_refund(&self) -> bool {
        self.link_kind == Some(LinkKind::Refund)
    }
}

/// A query over the ledger (module docs). Build it, then run
/// [`lines`](Ledger::lines) or [`lines_capped`](Ledger::lines_capped).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    range: Option<DateRange>,
    accounts: Vec<AccountId>,
    categories: Vec<CategoryKey>,
    uncategorized: bool,
    tags: Vec<TagName>,
    amount_min: Option<i64>,
    amount_max: Option<i64>,
    text: Option<String>,
    merchant: Option<String>,
    newest_first: bool,
    bank_text: bool,
}

impl Ledger {
    /// Every line in reports, oldest first.
    pub fn new() -> Ledger {
        Ledger::default()
    }

    /// Lines dated within `range`.
    pub fn within(mut self, range: DateRange) -> Ledger {
        self.range = Some(range);
        self
    }

    /// Lines on these accounts. A pot is its own account: its parent
    /// doesn't include it. Unknown accounts are refused when the query runs.
    pub fn accounts(mut self, accounts: impl IntoIterator<Item = AccountId>) -> Ledger {
        self.accounts.extend(accounts);
        self
    }

    /// Lines in these categories; a group's key includes its
    /// subcategories. Unknown keys are refused when the query runs.
    pub fn categories(mut self, categories: impl IntoIterator<Item = CategoryKey>) -> Ledger {
        self.categories.extend(categories);
        self
    }

    /// Only uncategorised lines (not a settlement's part, which has no
    /// category by design).
    pub fn uncategorized(mut self) -> Ledger {
        self.uncategorized = true;
        self
    }

    /// Lines whose transaction carries any of these tags, matched ignoring
    /// case like tag names are. A private tag is refused as if it didn't
    /// exist ([`Error::UnknownTag`]).
    pub fn tags(mut self, tags: impl IntoIterator<Item = TagName>) -> Ledger {
        self.tags.extend(tags);
        self
    }

    /// Lines whose whole amount ([`LedgerLine::amount`], signed minor units
    /// in its own currency) is at least `min` and at most `max`. A minimum
    /// above the maximum is refused when the query runs.
    pub fn amount_between(mut self, min: Option<i64>, max: Option<i64>) -> Ledger {
        self.amount_min = min;
        self.amount_max = max;
        self
    }

    /// Lines whose display merchant, bank merchant or raw description
    /// contains `text`, ignoring ASCII case. The text is only ever a
    /// parameter: it's matched, never run. Blank text matches everything.
    pub fn matching(mut self, text: &str) -> Ledger {
        let text = text.trim();
        self.text = (!text.is_empty()).then(|| text.to_owned());
        self
    }

    /// Lines whose display merchant or bank merchant is `name`, ignoring
    /// ASCII case and surrounding whitespace.
    pub fn merchant(mut self, name: &str) -> Ledger {
        self.merchant = Some(name.trim().to_owned());
        self
    }

    /// Newest first (by date, then transaction ID), instead of oldest.
    pub fn newest_first(mut self) -> Ledger {
        self.newest_first = true;
        self
    }

    /// Reads each line's [`BankText`] too.
    pub fn with_bank_text(mut self) -> Ledger {
        self.bank_text = true;
        self
    }

    /// Every matching line.
    pub fn lines(&self, reader: &Reader) -> Result<Vec<LedgerLine>, Error> {
        self.run(reader.conn(), None)
    }

    /// The first `limit` matching lines, and whether there were more.
    pub fn lines_capped(&self, reader: &Reader, limit: Limit) -> Result<Capped<LedgerLine>, Error> {
        let lines = self.run(reader.conn(), Some(limit.get() + 1))?;
        Ok(Capped::new(lines, limit))
    }

    fn run(&self, conn: &Connection, limit: Option<usize>) -> Result<Vec<LedgerLine>, Error> {
        if let (Some(min), Some(max)) = (self.amount_min, self.amount_max)
            && min > max
        {
            return Err(Error::InvalidAmountRange);
        }
        let mut sql = String::from(
            "SELECT l.transaction_id, l.split_id, l.line_no, l.link_id, l.link_kind, \
                    l.target_id, l.account_id, l.date, l.amount_minor, l.personal_minor, \
                    l.currency, l.merchant_display, \
                    c.id AS category_id, c.key AS category_key, c.kind AS category_kind",
        );
        if self.bank_text {
            sql.push_str(", t.merchant AS bank_merchant, t.description_raw");
        }
        sql.push_str(
            " FROM ledger_line l \
              JOIN \"transaction\" t ON t.id = l.transaction_id \
              LEFT JOIN category c ON c.id = l.category_id \
              WHERE l.in_reports AND l.amount_minor <> 0",
        );
        let mut values: Vec<SqlValue> = Vec::new();

        if let Some(range) = self.range {
            sql.push_str(" AND l.date BETWEEN ? AND ?");
            values.push(SqlValue::Text(date_key(range.from())));
            values.push(SqlValue::Text(date_key(range.to())));
        }
        if !self.accounts.is_empty() {
            for &id in &self.accounts {
                if account::find(conn, id)?.is_none() {
                    return Err(Error::UnknownAccount(id));
                }
            }
            any_of(
                &mut sql,
                &mut values,
                "l.account_id",
                self.accounts.iter().map(|id| id.get()),
            );
        }
        if !self.categories.is_empty() {
            let ids = category_ids(conn, &self.categories)?;
            any_of(&mut sql, &mut values, "l.category_id", ids);
        }
        if self.uncategorized {
            sql.push_str(" AND l.category_id IS NULL AND l.link_kind IS NOT 'settlement'");
        }
        if !self.tags.is_empty() {
            let ids = public_tag_ids(conn, &self.tags)?;
            let marks = vec!["?"; ids.len()].join(", ");
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM transaction_tag tt \
                   WHERE tt.transaction_id = l.transaction_id AND tt.tag_id IN ({marks}))"
            ));
            values.extend(ids.into_iter().map(SqlValue::Integer));
        }
        if let Some(min) = self.amount_min {
            sql.push_str(" AND l.amount_minor >= ?");
            values.push(SqlValue::Integer(min));
        }
        if let Some(max) = self.amount_max {
            sql.push_str(" AND l.amount_minor <= ?");
            values.push(SqlValue::Integer(max));
        }
        if let Some(text) = &self.text {
            sql.push_str(
                " AND (l.merchant_display LIKE ? ESCAPE '\\' \
                   OR t.merchant LIKE ? ESCAPE '\\' \
                   OR t.description_raw LIKE ? ESCAPE '\\')",
            );
            let pattern = format!("%{}%", escape_like(text));
            values.extend(std::iter::repeat_n(SqlValue::Text(pattern), 3));
        }
        if let Some(name) = &self.merchant {
            sql.push_str(
                " AND (trim(l.merchant_display) = ? COLLATE NOCASE \
                   OR trim(t.merchant) = ? COLLATE NOCASE)",
            );
            values.extend(std::iter::repeat_n(SqlValue::Text(name.clone()), 2));
        }

        if self.newest_first {
            sql.push_str(
                " ORDER BY l.date DESC, l.transaction_id DESC, l.line_no, l.link_id, \
                   l.target_split_id",
            );
        } else {
            sql.push_str(
                " ORDER BY l.date, l.transaction_id, l.line_no, l.link_id, l.target_split_id",
            );
        }
        if let Some(limit) = limit {
            sql.push_str(" LIMIT ?");
            values.push(SqlValue::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
        }

        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values), |row| self.read(row))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn read(&self, row: &Row<'_>) -> rusqlite::Result<LedgerLine> {
        let currency: Currency = row.get("currency")?;
        let link_kind: Option<String> = row.get("link_kind")?;
        let link_kind = link_kind
            .map(|key| LinkKind::from_key(&key).ok_or_else(|| account::invalid(row, "link_kind")))
            .transpose()?;
        let category = match row.get::<_, Option<i64>>("category_id")? {
            Some(id) => Some(LineCategory {
                id,
                key: row.get("category_key")?,
                kind: row.get("category_kind")?,
            }),
            None => None,
        };
        let bank_text = if self.bank_text {
            Some(BankText {
                merchant: row.get("bank_merchant")?,
                description: row.get("description_raw")?,
            })
        } else {
            None
        };
        Ok(LedgerLine {
            transaction_id: row.get("transaction_id")?,
            split_id: row.get("split_id")?,
            line_no: row.get("line_no")?,
            link_id: row.get("link_id")?,
            link_kind,
            target_id: row.get("target_id")?,
            account_id: row.get("account_id")?,
            date: account::date(row, "date")?,
            amount: Money::new(row.get("amount_minor")?, currency),
            personal: Money::new(row.get("personal_minor")?, currency),
            category,
            merchant_display: row.get("merchant_display")?,
            bank_text,
        })
    }
}

/// Adds ` AND <column> IN (?, …)` for `ids`.
fn any_of(
    sql: &mut String,
    values: &mut Vec<SqlValue>,
    column: &str,
    ids: impl IntoIterator<Item = i64>,
) {
    let ids: Vec<i64> = ids.into_iter().collect();
    let marks = vec!["?"; ids.len()].join(", ");
    sql.push_str(&format!(" AND {column} IN ({marks})"));
    values.extend(ids.into_iter().map(SqlValue::Integer));
}

/// The IDs of `keys`, each group with its subcategories.
fn category_ids(conn: &Connection, keys: &[CategoryKey]) -> Result<Vec<i64>, Error> {
    let tree = CategoryTree::load(conn)?;
    let mut ids = Vec::new();
    for key in keys {
        let category = tree
            .get(key)
            .ok_or_else(|| Error::UnknownCategory(key.clone()))?;
        ids.push(category.id());
        if let Some(group) = tree
            .groups()
            .iter()
            .find(|g| g.category().id() == category.id())
        {
            ids.extend(group.children().iter().map(|child| child.id()));
        }
    }
    Ok(ids)
}

/// The IDs of the public tags named in `names`, ignoring case like tag
/// names do. A private or unknown name is [`Error::UnknownTag`].
fn public_tag_ids(conn: &Connection, names: &[TagName]) -> Result<Vec<i64>, Error> {
    let mut statement = conn.prepare_cached("SELECT id, name, private FROM tag")?;
    let tags: Vec<(i64, String, bool)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    names
        .iter()
        .map(|name| {
            tags.iter()
                .find(|(_, stored, private)| {
                    !private && TagName::new(stored).is_ok_and(|stored| stored.same_name(name))
                })
                .map(|(id, _, _)| *id)
                .ok_or(Error::UnknownTag)
        })
        .collect()
}

/// `text` with `\`, `%` and `_` escaped for `LIKE … ESCAPE '\'`.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests;
