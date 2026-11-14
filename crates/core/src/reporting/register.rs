//! `register`: the Transactions table's read model (ROADMAP M3.3;
//! `DESIGN.md` → "Data table (Transactions)"; `ARCHITECTURE.md` →
//! "Decision record: the Transactions view").
//!
//! The reports read the [`Ledger`], which keeps only what
//! counts: no transfers, no excluded rows, one line per split. The table is
//! the other view of the same rows: one [`RegisterRow`] per transaction,
//! transfers and excluded rows included, with its split lines inside it.
//!
//! - [`Register::load`] reads every transaction once, newest first, with
//!   the accounts and categories the table names. A person's ledger fits
//!   in memory (tens of thousands of rows), so the app reads it again only
//!   after a change, never per keystroke.
//! - [`Register::select`] is pure: it filters ([`Filter`]: account,
//!   category, dates, uncategorised, text, excluded), groups the rows into
//!   days with each day's net, and sorts within each day ([`Sort`]). Day
//!   headers hold in every order: sorting by merchant, category, account or
//!   amount reorders a day's rows, never the days.
//! - **Category colours** ([`Colour`]) are ranked when the register is read
//!   (user, 2026-11-14): the six top-level groups with the most spending
//!   over the 12 months to `today` take `chart-1` … `chart-6`, their
//!   subcategories share the colour, and everything else (income,
//!   transfers, the `other` group, groups with no spending) is
//!   [`Colour::Other`]. Nothing is stored yet; M4.1 stores the colours so
//!   charts and table agree. Amounts in different currencies are never
//!   added: the ranking uses the currency with the most spending lines.
//!
//! Text search matches the display merchant, the bank's merchant and the
//! raw description, ignoring case. That text is the user's own data and
//! untrusted: it is only ever matched and shown, and nothing here logs it.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use chrono::{Datelike as _, Months, NaiveDate, NaiveTime};
use rusqlite::Connection;

use crate::account::{self, AccountKind};
use crate::category::{CategoryKey, CategoryKind, CategoryTree};
use crate::db::Reader;
use crate::exclusion::ExclusionReason;
use crate::id::{AccountId, RuleId, TransactionId};
use crate::money::{Currency, Money};

use super::{DateRange, Error, Ledger};

/// The most category groups that get a colour of their own (`DESIGN.md` →
/// "Data visualisation": at most six named series).
pub const NAMED_COLOURS: usize = 6;

/// The key of the group that always draws as "Other", whatever it holds.
const OTHER_GROUP: &str = "other";

/// A category's marker colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Colour {
    /// `chart-1` … `chart-6`: the slot, from 1.
    Chart(u8),
    /// The "Other" marker: an outline, not a seventh colour.
    Other,
}

/// A category as the table shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterCategory {
    /// `category.id`.
    pub id: i64,
    pub key: CategoryKey,
    pub label: String,
    pub kind: CategoryKind,
    /// Its group's ID; `None` for a group.
    pub group: Option<i64>,
    pub colour: Colour,
}

/// An account as the table names it (closed ones too: their rows stay).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterAccount {
    pub id: AccountId,
    pub name: String,
    pub kind: AccountKind,
}

/// One line of a split transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitPart {
    pub amount: Money,
    /// `category.id`; `None` while the line is uncategorised.
    pub category: Option<i64>,
}

/// A row's category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowCategory {
    Uncategorised,
    /// `category.id`.
    Category(i64),
    /// Split into lines, in line order.
    Split(Vec<SplitPart>),
}

/// One transaction in the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterRow {
    pub id: TransactionId,
    pub account: AccountId,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    /// Signed: negative is money out.
    pub amount: Money,
    /// The amount in the currency spent, when it differs (a card abroad).
    pub original: Option<Money>,
    /// What the Merchant column shows: the display merchant, else the
    /// bank's merchant, else the raw description.
    pub merchant: String,
    /// The bank's merchant text, as imported.
    pub bank_merchant: Option<String>,
    /// The raw description, as imported (the inspector's "Raw description").
    pub description: String,
    /// The bank's kind of payment ("Card payment"), when the export has it.
    pub payment_type: Option<String>,
    pub category: RowCategory,
    /// The rule that set the category.
    pub rule: Option<RuleId>,
    /// In a confirmed transfer: the other side's accounts. Transfers are
    /// left out of the day nets, and the table names them by the other side.
    pub transfer: Option<Vec<AccountId>>,
    pub excluded: Option<ExclusionReason>,
    /// The user's own note.
    pub note: Option<String>,
}

impl RegisterRow {
    /// Whether the row is in a confirmed transfer.
    pub fn is_transfer(&self) -> bool {
        self.transfer.is_some()
    }

    /// Whether it needs a category: no category and not a transfer, or a
    /// split line without one.
    pub fn is_uncategorised(&self) -> bool {
        match &self.category {
            RowCategory::Uncategorised => !self.is_transfer(),
            RowCategory::Category(_) => false,
            RowCategory::Split(parts) => parts.iter().any(|part| part.category.is_none()),
        }
    }

    /// Whether it counts in the day's net: not a transfer, not excluded.
    pub fn counts_in_net(&self) -> bool {
        !self.is_transfer() && self.excluded.is_none()
    }
}

/// A preset date range for the Date filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DatePreset {
    ThisMonth,
    LastMonth,
    /// This month and the two before it.
    LastThreeMonths,
    ThisYear,
    #[default]
    AllTime,
}

impl DatePreset {
    pub const ALL: [DatePreset; 5] = [
        DatePreset::ThisMonth,
        DatePreset::LastMonth,
        DatePreset::LastThreeMonths,
        DatePreset::ThisYear,
        DatePreset::AllTime,
    ];

    /// The days it covers as of `today`; `None` for all time. Ranges run to
    /// the end of the period, so a row dated later this month still shows.
    pub fn range(self, today: NaiveDate) -> Option<DateRange> {
        let month_start = today.with_day(1)?;
        let month_end = month_start.checked_add_months(Months::new(1))?.pred_opt()?;
        let (from, to) = match self {
            DatePreset::ThisMonth => (month_start, month_end),
            DatePreset::LastMonth => (
                month_start.checked_sub_months(Months::new(1))?,
                month_start.pred_opt()?,
            ),
            DatePreset::LastThreeMonths => {
                (month_start.checked_sub_months(Months::new(2))?, month_end)
            }
            DatePreset::ThisYear => (
                NaiveDate::from_ymd_opt(today.year(), 1, 1)?,
                NaiveDate::from_ymd_opt(today.year(), 12, 31)?,
            ),
            DatePreset::AllTime => return None,
        };
        DateRange::new(from, to).ok()
    }
}

/// What the table shows. Every part is optional and they AND together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// One account (a pot is its own account).
    pub account: Option<AccountId>,
    /// A category's ID; a group includes its subcategories. A split row
    /// matches when any of its lines does.
    pub category: Option<i64>,
    pub dates: Option<DateRange>,
    /// Only rows that need a category ([`RegisterRow::is_uncategorised`]).
    pub uncategorised: bool,
    /// Text in the merchant, the bank's merchant or the raw description,
    /// ignoring case. Blank matches everything.
    pub text: String,
    /// Excluded rows are hidden unless this is set (the Excluded filter,
    /// M3.10).
    pub include_excluded: bool,
}

/// A column the rows of each day can be sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortColumn {
    Merchant,
    Category,
    Account,
    Amount,
}

/// How each day's rows are ordered; the default is newest first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sort {
    /// As imported: newest first.
    #[default]
    Default,
    /// By `column`, A–Z or smallest first.
    Ascending(SortColumn),
    /// By `column`, Z–A or largest first.
    Descending(SortColumn),
}

impl Sort {
    /// What a click on `column`'s header does: sort by it, then reverse,
    /// then back to the default order.
    pub fn next(self, column: SortColumn) -> Sort {
        match self {
            Sort::Ascending(current) if current == column => Sort::Descending(column),
            Sort::Descending(current) if current == column => Sort::Default,
            _ => Sort::Ascending(column),
        }
    }

    /// The column it sorts by, if any.
    pub fn column(self) -> Option<SortColumn> {
        match self {
            Sort::Default => None,
            Sort::Ascending(column) | Sort::Descending(column) => Some(column),
        }
    }
}

/// One day of a [`Listing`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingDay {
    pub date: NaiveDate,
    /// The net of the day's rows that count ([`RegisterRow::counts_in_net`]),
    /// per currency, never added across currencies. Empty when none count.
    pub net: Vec<Money>,
    /// The day's rows: a range of [`Listing::rows`].
    pub rows: Range<usize>,
}

/// What [`Register::select`] picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Indices into [`Register::rows`], day by day, newest day first.
    pub rows: Vec<usize>,
    pub days: Vec<ListingDay>,
    /// How many rows match every part of the filter but `uncategorised`
    /// and need a category: the Uncategorised chip's count.
    pub uncategorised: usize,
}

impl Listing {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Every transaction, with the accounts and categories it names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Register {
    rows: Vec<RegisterRow>,
    /// Each row's searchable text, lowercased, by row index.
    haystacks: Vec<String>,
    by_id: HashMap<TransactionId, usize>,
    accounts: Vec<RegisterAccount>,
    categories: Vec<RegisterCategory>,
    category_index: HashMap<i64, usize>,
}

impl Register {
    /// A register of `rows` (any order: kept newest first, then by ID
    /// descending), naming `accounts` and `categories` (in menu order:
    /// each group, then its subcategories).
    pub fn new(
        mut rows: Vec<RegisterRow>,
        accounts: Vec<RegisterAccount>,
        categories: Vec<RegisterCategory>,
    ) -> Register {
        rows.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
        let haystacks = rows.iter().map(haystack).collect();
        let by_id = rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.id, index))
            .collect();
        let category_index = categories
            .iter()
            .enumerate()
            .map(|(index, category)| (category.id, index))
            .collect();
        Register {
            rows,
            haystacks,
            by_id,
            accounts,
            categories,
            category_index,
        }
    }

    /// Reads every transaction, account and category, with the colours
    /// ranked over the 12 months to `today` (module docs).
    pub fn load(reader: &Reader, today: NaiveDate) -> Result<Register, Error> {
        let conn = reader.conn();
        let categories = load_categories(reader, today)?;
        let accounts = load_accounts(conn)?;
        let rows = load_rows(conn)?;
        Ok(Register::new(rows, accounts, categories))
    }

    /// Every row, newest first.
    pub fn rows(&self) -> &[RegisterRow] {
        &self.rows
    }

    pub fn row(&self, id: TransactionId) -> Option<&RegisterRow> {
        self.by_id.get(&id).map(|&index| &self.rows[index])
    }

    pub fn accounts(&self) -> &[RegisterAccount] {
        &self.accounts
    }

    pub fn account(&self, id: AccountId) -> Option<&RegisterAccount> {
        self.accounts.iter().find(|account| account.id == id)
    }

    /// Every category in menu order: each group, then its subcategories.
    pub fn categories(&self) -> &[RegisterCategory] {
        &self.categories
    }

    pub fn category(&self, id: i64) -> Option<&RegisterCategory> {
        self.category_index
            .get(&id)
            .map(|&index| &self.categories[index])
    }

    /// The categories of `row`, for its label and markers: one for a
    /// categorised row, each line's (in line order, uncategorised lines
    /// left out) for a split one.
    pub fn row_categories(&self, row: &RegisterRow) -> Vec<&RegisterCategory> {
        match &row.category {
            RowCategory::Uncategorised => Vec::new(),
            RowCategory::Category(id) => self.category(*id).into_iter().collect(),
            RowCategory::Split(parts) => parts
                .iter()
                .filter_map(|part| part.category.and_then(|id| self.category(id)))
                .collect(),
        }
    }

    /// The rows `filter` keeps, grouped by day, each day sorted by `sort`.
    pub fn select(&self, filter: &Filter, sort: Sort) -> Listing {
        let text = filter.text.trim().to_lowercase();
        let mut uncategorised = 0;
        let mut kept: Vec<usize> = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            if !self.matches(row, filter) {
                continue;
            }
            if !text.is_empty() && !self.haystacks[index].contains(&text) {
                continue;
            }
            let needs_category = row.is_uncategorised();
            if needs_category {
                uncategorised += 1;
            }
            if filter.uncategorised && !needs_category {
                continue;
            }
            kept.push(index);
        }

        // `kept` is newest first, so each day's rows are together.
        let mut rows = Vec::with_capacity(kept.len());
        let mut days = Vec::new();
        for day_rows in kept.chunk_by(|&a, &b| self.rows[a].date == self.rows[b].date) {
            let mut day_rows = day_rows.to_vec();
            if sort != Sort::Default {
                day_rows.sort_by(|&a, &b| self.compare(a, b, sort));
            }
            let start = rows.len();
            rows.extend_from_slice(&day_rows);
            days.push(ListingDay {
                date: self.rows[day_rows[0]].date,
                net: net(day_rows.iter().map(|&index| &self.rows[index])),
                rows: start..rows.len(),
            });
        }
        Listing {
            rows,
            days,
            uncategorised,
        }
    }

    /// Every part of `filter` but the text and `uncategorised`.
    fn matches(&self, row: &RegisterRow, filter: &Filter) -> bool {
        if row.excluded.is_some() && !filter.include_excluded {
            return false;
        }
        if filter.account.is_some_and(|account| account != row.account) {
            return false;
        }
        if let Some(range) = filter.dates
            && !(range.from() <= row.date && row.date <= range.to())
        {
            return false;
        }
        if let Some(wanted) = filter.category {
            let in_wanted = |id: i64| {
                id == wanted
                    || self
                        .category(id)
                        .is_some_and(|category| category.group == Some(wanted))
            };
            let matched = match &row.category {
                RowCategory::Uncategorised => false,
                RowCategory::Category(id) => in_wanted(*id),
                RowCategory::Split(parts) => parts
                    .iter()
                    .any(|part| part.category.is_some_and(in_wanted)),
            };
            if !matched {
                return false;
            }
        }
        true
    }

    /// The order of two rows of one day under `sort`; ties keep the
    /// default order (newest first, then by ID).
    fn compare(&self, a: usize, b: usize, sort: Sort) -> Ordering {
        let (column, descending) = match sort {
            Sort::Default => return a.cmp(&b),
            Sort::Ascending(column) => (column, false),
            Sort::Descending(column) => (column, true),
        };
        let (row_a, row_b) = (&self.rows[a], &self.rows[b]);
        let order = match column {
            SortColumn::Merchant => compare_text(&row_a.merchant, &row_b.merchant),
            SortColumn::Category => {
                compare_text(&self.category_label(row_a), &self.category_label(row_b))
            }
            SortColumn::Account => compare_text(
                self.account_name(row_a.account),
                self.account_name(row_b.account),
            ),
            SortColumn::Amount => row_a
                .amount
                .currency()
                .code()
                .cmp(row_b.amount.currency().code())
                .then(row_a.amount.minor().cmp(&row_b.amount.minor())),
        };
        let order = if descending { order.reverse() } else { order };
        order.then(a.cmp(&b))
    }

    fn account_name(&self, id: AccountId) -> &str {
        self.account(id).map_or("", |account| account.name.as_str())
    }

    /// The text a row sorts by in the Category column: its category's
    /// label, its lines' labels, "Transfer", or nothing (uncategorised
    /// rows sort first).
    fn category_label(&self, row: &RegisterRow) -> String {
        if row.is_transfer() {
            return "Transfer".to_owned();
        }
        self.row_categories(row)
            .iter()
            .map(|category| category.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Text compared ignoring case, then as written.
fn compare_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// The searchable text of a row, lowercased.
fn haystack(row: &RegisterRow) -> String {
    let mut text = row.merchant.to_lowercase();
    if let Some(merchant) = &row.bank_merchant {
        text.push('\n');
        text.push_str(&merchant.to_lowercase());
    }
    text.push('\n');
    text.push_str(&row.description.to_lowercase());
    text
}

/// The net of the rows that count, per currency (in code order).
fn net<'a>(rows: impl Iterator<Item = &'a RegisterRow>) -> Vec<Money> {
    let mut sums: BTreeMap<String, (Currency, i128)> = BTreeMap::new();
    for row in rows.filter(|row| row.counts_in_net()) {
        let currency = row.amount.currency();
        sums.entry(currency.code().to_owned())
            .or_insert((currency, 0))
            .1 += i128::from(row.amount.minor());
    }
    sums.into_values()
        .map(|(currency, sum)| {
            // A day's net can't leave i64 in practice; clamp rather than fail.
            let minor = i64::try_from(sum).unwrap_or(if sum < 0 { i64::MIN } else { i64::MAX });
            Money::new(minor, currency)
        })
        .collect()
}

fn load_accounts(conn: &Connection) -> Result<Vec<RegisterAccount>, Error> {
    let mut statement = conn.prepare("SELECT id, name, kind FROM account ORDER BY id")?;
    let accounts = statement
        .query_map([], |row| {
            let kind: String = row.get("kind")?;
            Ok(RegisterAccount {
                id: row.get("id")?,
                name: row.get("name")?,
                kind: AccountKind::from_key(&kind).ok_or_else(|| account::invalid(row, "kind"))?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(accounts)
}

/// The categories in menu order, with their colours ranked over the 12
/// months to `today`.
fn load_categories(reader: &Reader, today: NaiveDate) -> Result<Vec<RegisterCategory>, Error> {
    let tree = CategoryTree::load(reader.conn())?;
    let ranked = rank_groups(reader, today, |id| {
        tree.groups()
            .iter()
            .find(|group| {
                group.category().id() == id || group.children().iter().any(|child| child.id() == id)
            })
            .map(|group| group.category().id())
    })?;
    let colour_of = |group: i64| {
        ranked
            .iter()
            .position(|&id| id == group)
            .map_or(Colour::Other, |slot| Colour::Chart(slot as u8 + 1))
    };
    let mut categories = Vec::with_capacity(tree.len());
    for group in tree.groups() {
        let parent = group.category();
        let colour = colour_of(parent.id());
        categories.push(RegisterCategory {
            id: parent.id(),
            key: parent.key().clone(),
            label: parent.label().to_owned(),
            kind: parent.kind(),
            group: None,
            colour,
        });
        for child in group.children() {
            categories.push(RegisterCategory {
                id: child.id(),
                key: child.key().clone(),
                label: child.label().to_owned(),
                kind: child.kind(),
                group: Some(parent.id()),
                colour,
            });
        }
    }
    Ok(categories)
}

/// The IDs of the groups that take the six colours, most spending first:
/// spending lines over the 12 months to `today`, the user's share, in the
/// currency with the most such lines. `group_of` maps a category ID to its
/// group's. The `other` group is never ranked.
fn rank_groups(
    reader: &Reader,
    today: NaiveDate,
    group_of: impl Fn(i64) -> Option<i64>,
) -> Result<Vec<i64>, Error> {
    let from = today
        .checked_sub_months(Months::new(12))
        .and_then(|day| day.succ_opt())
        .unwrap_or(today);
    let lines = Ledger::new()
        .within(DateRange::new(from, today)?)
        .lines(reader)?;
    let mut by_currency: HashMap<Currency, usize> = HashMap::new();
    let mut spent: HashMap<(Currency, i64), i128> = HashMap::new();
    for line in &lines {
        let Some(category) = &line.category else {
            continue;
        };
        // The `other` group stays outlined, however much it holds.
        let in_other =
            category.key.as_str() == OTHER_GROUP || category.key.parent() == Some(OTHER_GROUP);
        if category.kind != CategoryKind::Spending || in_other {
            continue;
        }
        let Some(group) = group_of(category.id) else {
            continue;
        };
        let code = line.personal.currency();
        *by_currency.entry(code).or_default() += 1;
        *spent.entry((code, group)).or_default() -= i128::from(line.personal.minor());
    }
    let Some(currency) = by_currency
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.code().cmp(a.0.code())))
        .map(|(code, _)| *code)
    else {
        return Ok(Vec::new());
    };
    let mut ranked: Vec<(i64, i128)> = spent
        .into_iter()
        .filter(|((code, _), amount)| *code == currency && *amount > 0)
        .map(|((_, group), amount)| (group, amount))
        .collect();
    // Most spending first; ties by group ID, so the order is stable.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ranked
        .into_iter()
        .take(NAMED_COLOURS)
        .map(|(group, _)| group)
        .collect())
}

fn load_rows(conn: &Connection) -> Result<Vec<RegisterRow>, Error> {
    // Split lines and transfer partners first, by transaction.
    let mut splits: HashMap<TransactionId, Vec<SplitPart>> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT s.transaction_id, s.amount_minor, s.category_id, t.currency \
               FROM transaction_split s JOIN \"transaction\" t ON t.id = s.transaction_id \
              ORDER BY s.transaction_id, s.line_no",
        )?;
        let rows = statement.query_map([], |row| {
            let currency: Currency = row.get(3)?;
            Ok((
                row.get::<_, TransactionId>(0)?,
                SplitPart {
                    amount: Money::new(row.get(1)?, currency),
                    category: row.get(2)?,
                },
            ))
        })?;
        for row in rows {
            let (transaction, part) = row?;
            splits.entry(transaction).or_default().push(part);
        }
    }
    let mut partners: HashMap<TransactionId, Vec<AccountId>> = HashMap::new();
    {
        let mut statement = conn.prepare(
            "SELECT t.id, o.account_id FROM \"transaction\" t \
               JOIN transfer_group g ON g.id = t.transfer_group_id AND g.status = 'confirmed' \
               JOIN \"transaction\" o ON o.transfer_group_id = t.transfer_group_id \
                                      AND o.id <> t.id \
              ORDER BY t.id, o.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, TransactionId>(0)?, row.get::<_, AccountId>(1)?))
        })?;
        for row in rows {
            let (transaction, account) = row?;
            partners.entry(transaction).or_default().push(account);
        }
    }

    let mut statement = conn.prepare(
        "SELECT t.id, t.account_id, t.date, t.time, t.amount_minor, t.currency, \
                t.original_amount_minor, t.original_currency, t.merchant, \
                t.merchant_display, t.description_raw, t.source_type, t.category_id, \
                t.rule_id, t.excluded_reason, t.user_note, g.status AS transfer_status \
           FROM \"transaction\" t \
           LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
          ORDER BY t.date DESC, t.id DESC",
    )?;
    let rows = statement.query_map([], |row| {
        let id: TransactionId = row.get("id")?;
        let currency: Currency = row.get("currency")?;
        let original = match (
            row.get::<_, Option<i64>>("original_amount_minor")?,
            row.get::<_, Option<Currency>>("original_currency")?,
        ) {
            (Some(minor), Some(currency)) => Some(Money::new(minor, currency)),
            _ => None,
        };
        let time = row
            .get::<_, Option<String>>("time")?
            .map(|text| {
                NaiveTime::parse_from_str(&text, "%H:%M:%S")
                    .map_err(|_| account::invalid(row, "time"))
            })
            .transpose()?;
        let excluded = row
            .get::<_, Option<String>>("excluded_reason")?
            .map(|key| {
                ExclusionReason::from_key(&key)
                    .ok_or_else(|| account::invalid(row, "excluded_reason"))
            })
            .transpose()?;
        let bank_merchant: Option<String> = row.get("merchant")?;
        let display: Option<String> = row.get("merchant_display")?;
        let description: String = row.get("description_raw")?;
        let merchant = [display.as_deref(), bank_merchant.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| description.trim())
            .to_owned();
        let confirmed =
            row.get::<_, Option<String>>("transfer_status")?.as_deref() == Some("confirmed");
        let category = match row.get::<_, Option<i64>>("category_id")? {
            Some(category) => RowCategory::Category(category),
            None => RowCategory::Uncategorised,
        };
        Ok(RegisterRow {
            id,
            account: row.get("account_id")?,
            date: account::date(row, "date")?,
            time,
            amount: Money::new(row.get("amount_minor")?, currency),
            original,
            merchant,
            bank_merchant,
            description,
            payment_type: row.get("source_type")?,
            category,
            rule: row.get("rule_id")?,
            transfer: confirmed.then(Vec::new),
            excluded,
            note: row.get("user_note")?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let mut row = row?;
        if let Some(parts) = splits.remove(&row.id) {
            row.category = RowCategory::Split(parts);
        }
        if let Some(transfer) = &mut row.transfer {
            *transfer = partners.remove(&row.id).unwrap_or_default();
        }
        out.push(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
