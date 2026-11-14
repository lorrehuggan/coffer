//! What the Inbox screen shows about a suggestion (ROADMAP M3.6, M3.14;
//! `docs/DESIGN.md` → "Inbox", "Inbox impact preview"): the transactions,
//! accounts and categories it names ([`describe`]), and, while it is
//! pending, the rows approving it would change, one by one ([`preview`]).
//!
//! [`impact`](super::impact) counts; this lists. Both are computed against
//! today's data each time they are called (decision record "Inbox impact
//! preview", point 6), and what [`preview`] lists comes from the same code
//! the approval runs, rolled back ([`change::dry_run`]):
//!
//! - **rule**: every past transaction the rule matches, with what it has
//!   now and what approving (with the apply to history the Inbox makes,
//!   point 1) gives it: the rule's category, or "stays" for a category the
//!   user set. The spending each category gains or loses, the rule that
//!   decides those rows today, and the pending categorisations the
//!   approval would settle (point 4). A [wide](super::RuleImpact::wide)
//!   rule is also summed by merchant ([`ByMerchant`]).
//! - **merchant_alias**: every transaction the name matches, with the name
//!   it shows now and after, or "stays" for a name the user set. Names
//!   don't change spending or which rules match (point 5).
//! - **categorization**: each item, now and after, and the spending change.
//! - **transfer_pair** and **refund_link**: how income and spending change
//!   in the month of the money.
//!
//! The lists are capped as the design draws them: up to
//! [`LIST_ALL`] matches are listed in full, more as the newest
//! [`LIST_NEWEST`]; [`Preview::matches`] says how many there are.
//!
//! Bank text, merchant names and patterns here are the user's bank's or an
//! AI client's text: the app shows them as plain text, stripped
//! ([`crate::envelope::shown`]), and acts on none of them. Nothing here is
//! for MCP.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;

use chrono::NaiveDate;
use regex::{Regex, RegexBuilder};
use rusqlite::{Connection, OptionalExtension, params};

use super::approve::{Applied, ApproveError, ApproveOptions, approve_in};
use super::impact::{Detail, Impact, impact};
use super::payload::{CategorizationItem, Target};
use super::{Error, Payload, Suggestion, SuggestionId, SuggestionKind, SuggestionStatus, get_in};
use crate::category::{CategoryKey, CategoryKind};
use crate::change;
use crate::db::Database;
use crate::id::{AccountId, RuleId, SplitId, TransactionId};
use crate::merchant::{self, AliasDefinition, AliasSet};
use crate::money::{Currency, Money};
use crate::reporting::Period;
use crate::rule::{self, MatchField, MatchInput, MatchKind, Pattern, RuleDefinition, RuleSet};

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;

/// Up to this many matches are listed in full.
pub const LIST_ALL: usize = 10;

/// Past [`LIST_ALL`], the newest this many are listed.
pub const LIST_NEWEST: usize = 6;

/// A wide rule's summary names this many merchants; the rest are summed
/// in one line.
pub const MERCHANTS_SHOWN: usize = 3;

// What a suggestion names ------------------------------------------------------

/// A category, as the Inbox shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CategoryRef {
    pub key: CategoryKey,
    /// The user's label.
    pub label: String,
    pub kind: CategoryKind,
}

/// Who set a transaction's category (`category_source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetBy {
    /// The user, by hand.
    User,
    /// The user, by approving a suggestion.
    Approved,
    /// A rule; `None` when the rule has since been deleted.
    Rule(Option<RuleId>),
    /// The bank's own category, through the preset.
    Preset,
}

impl SetBy {
    /// Whether the user set it, so an agent's rule leaves it alone
    /// (decision record "Inbox impact preview", point 1).
    pub const fn is_user(self) -> bool {
        matches!(self, SetBy::User | SetBy::Approved)
    }
}

/// A transaction's (or split line's) category now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Now {
    Uncategorised,
    /// A split transaction: its lines have the categories.
    Split,
    Category {
        category: CategoryRef,
        set_by: SetBy,
    },
}

impl Now {
    pub fn category(&self) -> Option<&CategoryRef> {
        match self {
            Now::Category { category, .. } => Some(category),
            _ => None,
        }
    }
}

/// A transaction as the Inbox shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionRef {
    pub id: TransactionId,
    /// The split line, when the suggestion names one: then `amount` and
    /// `now` are the line's.
    pub split_line: Option<SplitId>,
    pub account: AccountId,
    pub account_name: String,
    pub date: NaiveDate,
    /// `HH:MM:SS`, when the bank gives one.
    pub time: Option<String>,
    /// Signed: money out is negative.
    pub amount: Money,
    /// The name it shows (`merchant_display`).
    pub merchant: Option<String>,
    /// Whether that name was set by the user or a rule, so a merchant name
    /// leaves it alone.
    pub name_kept: bool,
    /// The bank's description, as imported.
    pub bank_text: String,
    /// The bank's payment type ("Card payment"), when the export has one.
    pub source_type: Option<String>,
    pub now: Now,
}

impl TransactionRef {
    /// The name it shows, or its bank text when it has none.
    pub fn name(&self) -> &str {
        self.merchant.as_deref().unwrap_or(&self.bank_text)
    }
}

/// A categorisation item and what it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRef {
    pub target: Target,
    /// `None`: gone since (deleted, undone, split again).
    pub transaction: Option<TransactionRef>,
    /// The suggested category; `None` when it no longer exists.
    pub category: Option<CategoryRef>,
}

/// What a suggestion names, resolved against today's data. Any status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    Categorization(Vec<ItemRef>),
    /// The categories the rule's actions name (set_category, or a split's
    /// lines), in order.
    Rule(Vec<CategoryRef>),
    TransferPair {
        outflow: Option<TransactionRef>,
        inflow: Option<TransactionRef>,
    },
    MerchantAlias,
    RefundLink {
        refund: Option<TransactionRef>,
        purchase: Option<TransactionRef>,
    },
}

/// What `suggestion` names (module docs). Any status: a decided
/// suggestion's rows are shown as they are now.
pub fn describe(db: &Database, suggestion: &Suggestion) -> Result<Subject, Error> {
    describe_in(db.conn(), &suggestion.payload)
}

fn describe_in(conn: &Connection, payload: &Payload) -> Result<Subject, Error> {
    Ok(match payload {
        Payload::Categorization(items) => Subject::Categorization(
            items
                .iter()
                .map(|item| item_ref(conn, item))
                .collect::<rusqlite::Result<_>>()?,
        ),
        Payload::Rule(definition) => Subject::Rule(
            rule_category_keys(definition)
                .iter()
                .filter_map(|key| category(conn, key).transpose())
                .collect::<rusqlite::Result<_>>()?,
        ),
        Payload::TransferPair(pair) => Subject::TransferPair {
            outflow: transaction(conn, pair.outflow)?,
            inflow: transaction(conn, pair.inflow)?,
        },
        Payload::MerchantAlias(_) => Subject::MerchantAlias,
        Payload::RefundLink(link) => Subject::RefundLink {
            refund: transaction(conn, link.refund)?,
            purchase: transaction(conn, link.purchase)?,
        },
    })
}

fn rule_category_keys(definition: &RuleDefinition) -> Vec<CategoryKey> {
    let mut keys = Vec::new();
    for action in definition.actions() {
        match action {
            rule::RuleAction::SetCategory(key) => keys.push(key.clone()),
            rule::RuleAction::SplitTemplate(template) => {
                keys.extend(template.lines().iter().map(|line| line.category().clone()));
            }
            _ => {}
        }
    }
    keys
}

/// The category with `key`, if it exists.
fn category(conn: &Connection, key: &CategoryKey) -> rusqlite::Result<Option<CategoryRef>> {
    conn.prepare_cached("SELECT label, kind FROM category WHERE key = ?1")?
        .query_row([key.as_str()], |row| {
            Ok(CategoryRef {
                key: key.clone(),
                label: row.get(0)?,
                kind: row.get(1)?,
            })
        })
        .optional()
}

const TRANSACTION: &str = "SELECT t.id, t.account_id, a.name, t.date, t.time, t.amount_minor, \
     t.currency, t.merchant_display, t.merchant_display_source, t.description_raw, \
     t.source_type, t.category_source, t.rule_id, c.key, c.label, c.kind, \
     EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id) \
     FROM \"transaction\" t JOIN account a ON a.id = t.account_id \
     LEFT JOIN category c ON c.id = t.category_id";

fn read_transaction(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionRef> {
    let currency: Currency = row.get(6)?;
    let source: Option<String> = row.get(11)?;
    let key: Option<String> = row.get(13)?;
    let split: bool = row.get(16)?;
    let now = match (key, split) {
        (_, true) => Now::Split,
        (None, false) => Now::Uncategorised,
        (Some(key), false) => Now::Category {
            category: CategoryRef {
                key: CategoryKey::new(&key).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        13,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                label: row.get(14)?,
                kind: row.get(15)?,
            },
            set_by: match source.as_deref() {
                Some("user") => SetBy::User,
                Some("ai_approved") => SetBy::Approved,
                Some("rule") => SetBy::Rule(row.get(12)?),
                _ => SetBy::Preset,
            },
        },
    };
    let name_source: Option<String> = row.get(8)?;
    let date: String = row.get(3)?;
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(TransactionRef {
        id: row.get(0)?,
        split_line: None,
        account: row.get(1)?,
        account_name: row.get(2)?,
        date,
        time: row.get(4)?,
        amount: Money::new(row.get(5)?, currency),
        merchant: row.get(7)?,
        name_kept: name_source.is_some(),
        bank_text: row.get(9)?,
        source_type: row.get(10)?,
        now,
    })
}

/// Transaction `id`, if it exists.
fn transaction(conn: &Connection, id: TransactionId) -> rusqlite::Result<Option<TransactionRef>> {
    conn.prepare_cached(&format!("{TRANSACTION} WHERE t.id = ?1"))?
        .query_row([id], read_transaction)
        .optional()
}

/// Every transaction, newest first.
fn all_transactions(conn: &Connection) -> rusqlite::Result<Vec<TransactionRef>> {
    conn.prepare(&format!("{TRANSACTION} ORDER BY t.date DESC, t.id DESC"))?
        .query_map([], read_transaction)?
        .collect()
}

fn item_ref(conn: &Connection, item: &CategorizationItem) -> rusqlite::Result<ItemRef> {
    let transaction = match item.target {
        Target::Transaction(id) => transaction(conn, id)?,
        Target::SplitLine(line) => split_line(conn, line)?,
    };
    Ok(ItemRef {
        target: item.target,
        transaction,
        category: category(conn, &item.category)?,
    })
}

/// Split line `line` as its transaction, with the line's amount and
/// category.
fn split_line(conn: &Connection, line: SplitId) -> rusqlite::Result<Option<TransactionRef>> {
    let Some((parent, amount, key)): Option<(TransactionId, i64, Option<String>)> = conn
        .query_row(
            "SELECT s.transaction_id, s.amount_minor, c.key FROM transaction_split s \
             LEFT JOIN category c ON c.id = s.category_id WHERE s.id = ?1",
            [line],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let Some(mut row) = transaction(conn, parent)? else {
        return Ok(None);
    };
    row.split_line = Some(line);
    row.amount = Money::new(amount, row.amount.currency());
    let category = match key {
        Some(key) => CategoryKey::new(&key)
            .ok()
            .map(|key| category(conn, &key))
            .transpose()?
            .flatten(),
        None => None,
    };
    // A split line's category is always the user's (approve.rs).
    row.now = match category {
        Some(category) => Now::Category {
            category,
            set_by: SetBy::User,
        },
        None => Now::Uncategorised,
    };
    Ok(Some(row))
}

// The preview -------------------------------------------------------------------

/// What approving a listed row would give it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    /// This category.
    Category(CategoryRef),
    /// Split by the rule's template into these lines (category and
    /// amount, minor units of the row's currency).
    Split(Vec<(CategoryRef, i64)>),
    /// Its category stays, but the rule changes its name or tags.
    Other,
    /// This name.
    Name(String),
    /// Left alone: the user set its category (or name).
    Stays,
    /// It already has what the suggestion gives it.
    Unchanged,
    /// A split row: a rule's apply skips it.
    SplitSkipped,
}

impl After {
    /// Whether approving changes the row.
    pub const fn changes(&self) -> bool {
        matches!(
            self,
            After::Category(_) | After::Split(_) | After::Name(_) | After::Other
        )
    }
}

/// A listed row: what it is now and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub transaction: TransactionRef,
    pub after: After,
    /// The part of `transaction.bank_text` the pattern matches, in bytes,
    /// when it can be found there.
    pub highlight: Option<Range<usize>>,
}

/// The rule that decides a rule's matches today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deciding {
    /// None of them is filed by a rule.
    NoRule,
    /// All the ones a rule filed, by this rule.
    Rule(RuleDefinition),
    /// More than one rule.
    Several,
}

/// The names a merchant name's matches show today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamesNow {
    One(String),
    /// This many different names.
    Several(usize),
}

/// How much more (positive) or less (negative) a category would count as
/// spent. `category` is `None` for uncategorised money out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendingChange {
    pub category: Option<CategoryRef>,
    pub change: Money,
}

/// A category summed over many rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryNow {
    Uncategorised,
    Category(CategoryRef),
    Several,
}

/// One line of a wide rule's by-merchant summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerchantLine {
    /// The name; empty for the summed rest.
    pub merchant: String,
    /// One of its rows' bank text, to show with the match highlighted.
    pub bank_text: String,
    pub highlight: Option<Range<usize>>,
    pub now: CategoryNow,
    /// What approving gives them; `None` when their rows differ.
    pub after: Option<After>,
    pub count: usize,
    /// The sum; `None` when the rows are in more than one currency.
    pub total: Option<Money>,
    /// Whether income would turn into spending.
    pub moves_income: bool,
}

/// A wide rule's matches summed by merchant, biggest surprise first:
/// income turning into spending, then by count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByMerchant {
    /// Distinct merchants among the matches.
    pub merchants: usize,
    /// The first [`MERCHANTS_SHOWN`] the rule changes.
    pub shown: Vec<MerchantLine>,
    /// The rest it changes, summed, and how many merchants they are.
    pub rest: Option<(MerchantLine, usize)>,
    /// The rows that stay because the user set them, summed.
    pub kept: Option<MerchantLine>,
}

/// How a transfer pair changes the month's totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferEffect {
    /// The month of the money out.
    pub month: Period,
    /// The money in counts as income now, and wouldn't.
    pub income_less: Option<Money>,
    /// The money out counts as spending now, and wouldn't.
    pub spending_less: Option<Money>,
}

/// How a refund link changes the refund's month.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkEffect {
    /// The month the money came back.
    pub month: Period,
    /// How much of the purchase would be paid back, this link included.
    pub refunded: Money,
    /// The money in counts as income now, and wouldn't.
    pub income_less: Option<Money>,
    /// The purchase's category (`None`: uncategorised) would count this much
    /// less spent in `month`.
    pub spending_less: Option<(Option<CategoryRef>, Money)>,
}

/// What approving a pending suggestion would change, row by row (module
/// docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The counts, and why approving would be refused.
    pub impact: Impact,
    /// What the suggestion names.
    pub subject: Subject,
    /// The rows listed, newest first for a rule or a name; in item order for
    /// a categorisation.
    pub lines: Vec<Line>,
    /// How many rows the list is drawn from: a rule's or name's matches, a
    /// categorisation's items.
    pub matches: usize,
    /// Rows left alone because the user set their category (rule) or name
    /// (merchant name).
    pub kept: usize,
    /// A wide rule's summary.
    pub by_merchant: Option<ByMerchant>,
    /// What each category gains or loses in spending, losses first.
    pub spending: Vec<SpendingChange>,
    /// A rule: the pending categorisations approving it settles.
    pub settles: Vec<SuggestionId>,
    /// A rule: what decides its matches today.
    pub deciding: Option<Deciding>,
    /// A rule: the rule it would run before (the first one now), if any.
    pub before_rule: Option<RuleDefinition>,
    /// A name: what its matches show now.
    pub names_now: Option<NamesNow>,
    /// A name: how many different names the rows it renames show now.
    pub names_merged: usize,
    pub transfer: Option<TransferEffect>,
    pub link: Option<LinkEffect>,
}

/// What approving pending suggestion `id` would change now, row by row
/// (module docs). Reads only: whatever it runs is rolled back. Fails as
/// [`impact`] fails.
pub fn preview(db: &Database, id: SuggestionId) -> Result<Preview, ApproveError> {
    let impact = impact(db, id)?;
    let suggestion = get_in(db.conn(), id)?;
    let subject = describe_in(db.conn(), &suggestion.payload)?;
    let mut preview = Preview {
        impact,
        subject,
        lines: Vec::new(),
        matches: 0,
        kept: 0,
        by_merchant: None,
        spending: Vec::new(),
        settles: Vec::new(),
        deciding: None,
        before_rule: None,
        names_now: None,
        names_merged: 0,
        transfer: None,
        link: None,
    };
    match &suggestion.payload {
        Payload::Categorization(_) => categorization(&mut preview),
        Payload::Rule(definition) => rule_preview(db, id, definition, &mut preview)?,
        Payload::MerchantAlias(alias) => alias_preview(db, id, alias, &mut preview)?,
        Payload::TransferPair(_) => preview.transfer = transfer_effect(&preview.subject),
        Payload::RefundLink(_) => preview.link = link_effect(&preview),
    }
    Ok(preview)
}

fn categorization(preview: &mut Preview) {
    let Subject::Categorization(items) = &preview.subject else {
        return;
    };
    preview.matches = items.len();
    preview.lines = items
        .iter()
        .filter_map(|item| {
            let transaction = item.transaction.clone()?;
            let category = item.category.clone()?;
            let after = if transaction.now.category() == Some(&category) {
                After::Unchanged
            } else {
                After::Category(category)
            };
            Some(Line {
                transaction,
                after,
                highlight: None,
            })
        })
        .collect();
    preview.spending = spending(&preview.lines);
}

fn rule_preview(
    db: &Database,
    id: SuggestionId,
    definition: &RuleDefinition,
    preview: &mut Preview,
) -> Result<(), ApproveError> {
    preview.settles = settled_by(db, id)?;
    let rules = rule::load_all(db.conn()).map_err(ApproveError::from)?;
    preview.before_rule = rules.first().map(|stored| stored.definition().clone());
    let by_id: HashMap<RuleId, RuleDefinition> = rules
        .iter()
        .map(|stored| (stored.id(), stored.definition().clone()))
        .collect();

    let matched = change::dry_run(db, |conn| -> Result<Vec<Line>, ApproveError> {
        let rows = all_transactions(conn)?;
        let rule_id = rule::create_at_in(conn, definition, 0)?;
        let plan = rule::preview_apply_in(conn, rule_id)?;
        let set = RuleSet::load(conn)?;
        let skipped: HashSet<TransactionId> = plan.split_rows.iter().copied().collect();
        let plans: HashMap<TransactionId, &rule::RowPlan> =
            plan.rows.iter().map(|row| (row.transaction, row)).collect();
        let raw = raw_text(conn)?;
        let mut lines = Vec::new();
        for row in rows {
            let Some((merchant, source_category)) = raw.get(&row.id) else {
                continue;
            };
            let input = MatchInput {
                account_id: row.account,
                amount_minor: row.amount.minor(),
                merchant: merchant.as_deref(),
                description: &row.bank_text,
                source_category: source_category.0.as_deref(),
                source_type: source_category.1.as_deref(),
            };
            if set.evaluate_rule(rule_id, &input).is_none() {
                continue;
            }
            let plan = plans.get(&row.id);
            let after = match plan.and_then(|plan| category_change(plan)) {
                Some((_, true)) => After::Stays,
                Some((target, false)) => after_target(conn, target)?,
                None if skipped.contains(&row.id) => After::SplitSkipped,
                None if plan.is_some_and(|plan| plan.has_automatic()) => After::Other,
                None => After::Unchanged,
            };
            let highlight = highlight(Some(definition.field()), definition.pattern(), &row);
            lines.push(Line {
                transaction: row,
                after,
                highlight,
            });
        }
        Ok(lines)
    })?;

    let filed_by: BTreeSet<Option<RuleId>> = matched
        .iter()
        .filter_map(|line| match &line.transaction.now {
            Now::Category {
                set_by: SetBy::Rule(rule),
                ..
            } => Some(*rule),
            _ => None,
        })
        .collect();
    preview.deciding = Some(match filed_by.len() {
        0 => Deciding::NoRule,
        1 => match filed_by
            .first()
            .copied()
            .flatten()
            .and_then(|rule| by_id.get(&rule))
        {
            Some(definition) => Deciding::Rule(definition.clone()),
            None => Deciding::Several,
        },
        _ => Deciding::Several,
    });
    preview.matches = matched.len();
    preview.kept = matched
        .iter()
        .filter(|line| line.after == After::Stays)
        .count();
    preview.spending = spending(&matched);
    if preview.impact.wide() {
        preview.by_merchant = Some(by_merchant(&matched));
    }
    preview.lines = listed(matched);
    Ok(())
}

/// The rows' bank merchant, source category and source type, which rules
/// match and the Inbox doesn't show.
type RawText = HashMap<TransactionId, (Option<String>, (Option<String>, Option<String>))>;

fn raw_text(conn: &Connection) -> rusqlite::Result<RawText> {
    conn.prepare("SELECT id, merchant, source_category, source_type FROM \"transaction\"")?
        .query_map([], |row| {
            Ok((row.get(0)?, (row.get(1)?, (row.get(2)?, row.get(3)?))))
        })?
        .collect()
}

/// A row plan's category change (a category or a split) and whether it
/// overwrites one the user set.
fn category_change(plan: &rule::RowPlan) -> Option<(&rule::Target, bool)> {
    plan.changes
        .iter()
        .find(|change| {
            matches!(
                change.new,
                rule::Target::Category { .. } | rule::Target::Split { .. }
            )
        })
        .map(|change| (&change.new, change.user_set))
}

fn after_target(conn: &Connection, target: &rule::Target) -> rusqlite::Result<After> {
    Ok(match target {
        rule::Target::Category { key, .. } => match category(conn, key)? {
            Some(category) => After::Category(category),
            None => After::Unchanged,
        },
        rule::Target::Split { lines, .. } => {
            let mut split = Vec::with_capacity(lines.len());
            for line in lines {
                if let Some(category) = category(conn, &line.category)? {
                    split.push((category, line.amount_minor));
                }
            }
            After::Split(split)
        }
        _ => After::Unchanged,
    })
}

/// The pending categorisations approving rule `id` (with its apply to
/// history) would settle.
fn settled_by(db: &Database, id: SuggestionId) -> Result<Vec<SuggestionId>, ApproveError> {
    change::dry_run(db, |conn| {
        Ok(approve_in(
            conn,
            id,
            ApproveOptions {
                apply_to_history: true,
            },
        )
        .map(|approved| approved.settled)
        .unwrap_or_default())
    })
}

/// Up to [`LIST_ALL`] lines as they are, else the newest [`LIST_NEWEST`]
/// (they come newest first).
fn listed(mut lines: Vec<Line>) -> Vec<Line> {
    if lines.len() > LIST_ALL {
        lines.truncate(LIST_NEWEST);
    }
    lines
}

/// Whether a row of `amount` in `category` counts as spending
/// (`reporting::summary`'s sides).
fn is_spending(category: Option<&CategoryRef>, amount: Money) -> bool {
    match category {
        Some(category) => category.kind == CategoryKind::Spending,
        None => amount.is_negative(),
    }
}

/// Whether a row of `amount` in `category` counts as income.
fn is_income(category: Option<&CategoryRef>, amount: Money) -> bool {
    match category {
        Some(category) => category.kind == CategoryKind::Income,
        None => amount.is_positive(),
    }
}

/// What each category gains or loses in spending when `lines` change
/// category: losses first, then gains, each biggest first.
fn spending(lines: &[Line]) -> Vec<SpendingChange> {
    let mut changes: BTreeMap<(Option<CategoryKey>, Currency), (Option<CategoryRef>, i64)> =
        BTreeMap::new();
    let mut add = |category: Option<&CategoryRef>, amount: Money, sign: i64| {
        if !is_spending(category, amount) {
            return;
        }
        let entry = changes
            .entry((category.map(|c| c.key.clone()), amount.currency()))
            .or_insert_with(|| (category.cloned(), 0));
        entry.1 = entry.1.saturating_add(sign.saturating_mul(-amount.minor()));
    };
    for line in lines {
        let amount = line.transaction.amount;
        let new: Vec<(&CategoryRef, Money)> = match &line.after {
            After::Category(new) => vec![(new, amount)],
            After::Split(split) => split
                .iter()
                .map(|(category, minor)| (category, Money::new(*minor, amount.currency())))
                .collect(),
            _ => continue,
        };
        match &line.transaction.now {
            Now::Split => continue,
            now => add(now.category(), amount, -1),
        }
        for (category, part) in new {
            add(Some(category), part, 1);
        }
    }
    let mut changes: Vec<SpendingChange> = changes
        .into_iter()
        .filter(|(_, (_, minor))| *minor != 0)
        .map(|((_, currency), (category, minor))| SpendingChange {
            category,
            change: Money::new(minor, currency),
        })
        .collect();
    changes.sort_by_key(|change| {
        let minor = change.change.minor();
        (minor > 0, if minor > 0 { -minor } else { minor })
    });
    changes
}

/// A wide rule's matches by merchant (see [`ByMerchant`]).
fn by_merchant(lines: &[Line]) -> ByMerchant {
    let mut merchants: BTreeMap<String, Vec<&Line>> = BTreeMap::new();
    let mut kept = Vec::new();
    for line in lines {
        match line.after {
            After::Stays => kept.push(line),
            _ => merchants
                .entry(line.transaction.name().to_owned())
                .or_default()
                .push(line),
        }
    }
    let distinct: HashSet<&str> = lines.iter().map(|line| line.transaction.name()).collect();
    let mut summed: Vec<MerchantLine> = merchants
        .into_iter()
        .map(|(name, rows)| sum_lines(name, &rows))
        .collect();
    summed.sort_by(|a, b| {
        b.moves_income
            .cmp(&a.moves_income)
            .then(b.count.cmp(&a.count))
            .then_with(|| a.merchant.cmp(&b.merchant))
    });
    let rest: Vec<MerchantLine> = summed.split_off(summed.len().min(MERCHANTS_SHOWN));
    let rest = (!rest.is_empty()).then(|| {
        let merchants = rest.len();
        let mut line = rest[0].clone();
        line.merchant = String::new();
        line.highlight = None;
        line.bank_text = String::new();
        for other in &rest[1..] {
            line.count += other.count;
            line.total = match (line.total, other.total) {
                (Some(a), Some(b)) if a.currency() == b.currency() => Some(Money::new(
                    a.minor().saturating_add(b.minor()),
                    a.currency(),
                )),
                _ => None,
            };
            line.moves_income |= other.moves_income;
            if line.now != other.now {
                line.now = CategoryNow::Several;
            }
            if line.after != other.after {
                line.after = None;
            }
        }
        (line, merchants)
    });
    let kept = (!kept.is_empty()).then(|| {
        let mut line = sum_lines(String::new(), &kept);
        line.bank_text = String::new();
        line.highlight = None;
        line
    });
    ByMerchant {
        merchants: distinct.len(),
        shown: summed,
        rest,
        kept,
    }
}

/// Whether `after` puts a row (or a split line) in a spending category.
fn into_spending(after: &After) -> bool {
    match after {
        After::Category(category) => category.kind == CategoryKind::Spending,
        After::Split(lines) => lines
            .iter()
            .any(|(category, _)| category.kind == CategoryKind::Spending),
        _ => false,
    }
}

fn sum_lines(merchant: String, rows: &[&Line]) -> MerchantLine {
    let mut now: Option<CategoryNow> = None;
    let mut after: Option<Option<After>> = None;
    let mut total: Option<Money> = rows
        .first()
        .map(|line| Money::new(0, line.transaction.amount.currency()));
    let mut moves_income = false;
    for line in rows {
        let row_now = match line.transaction.now.category() {
            Some(category) => CategoryNow::Category(category.clone()),
            None => CategoryNow::Uncategorised,
        };
        now = Some(match now {
            Some(previous) if previous != row_now => CategoryNow::Several,
            _ => row_now,
        });
        let row_after = Some(line.after.clone());
        after = Some(match after {
            Some(previous) if previous != row_after => None,
            _ => row_after,
        });
        let amount = line.transaction.amount;
        total = total.and_then(|sum| {
            (sum.currency() == amount.currency())
                .then(|| Money::new(sum.minor().saturating_add(amount.minor()), sum.currency()))
        });
        moves_income |=
            is_income(line.transaction.now.category(), amount) && into_spending(&line.after);
    }
    let first = rows.first();
    MerchantLine {
        merchant,
        bank_text: first
            .map(|line| line.transaction.bank_text.clone())
            .unwrap_or_default(),
        highlight: first.and_then(|line| line.highlight.clone()),
        now: now.unwrap_or(CategoryNow::Uncategorised),
        after: after.flatten(),
        count: rows.len(),
        total,
        moves_income,
    }
}

fn alias_preview(
    db: &Database,
    id: SuggestionId,
    alias: &AliasDefinition,
    preview: &mut Preview,
) -> Result<(), ApproveError> {
    let lines = change::dry_run(db, |conn| -> Result<Vec<Line>, ApproveError> {
        let rows = all_transactions(conn)?;
        let raw = raw_text(conn)?;
        let Ok(approved) = approve_in(conn, id, ApproveOptions::default()) else {
            return Ok(Vec::new());
        };
        let Applied::MerchantAlias {
            alias: alias_id, ..
        } = approved.applied
        else {
            return Ok(Vec::new());
        };
        let set = AliasSet::load(conn).map_err(ApproveError::from)?;
        let mut names =
            conn.prepare("SELECT merchant_display FROM \"transaction\" WHERE id = ?1")?;
        let pattern = Pattern::new(alias.kind().match_kind(), alias.pattern()).ok();
        let mut lines = Vec::new();
        for row in rows {
            let merchant = raw
                .get(&row.id)
                .and_then(|(merchant, _)| merchant.as_deref());
            let Some(source) = merchant::source_text(merchant, &row.bank_text) else {
                continue;
            };
            let cleaned = merchant::clean(source);
            if set.resolve(&cleaned).alias != Some(alias_id) {
                continue;
            }
            let name: Option<String> = names.query_row([row.id], |r| r.get(0))?;
            let after = if row.name_kept {
                After::Stays
            } else if name == row.merchant {
                After::Unchanged
            } else {
                After::Name(name.unwrap_or_default())
            };
            let highlight = pattern
                .as_ref()
                .and_then(|pattern| highlight(None, pattern, &row));
            lines.push(Line {
                transaction: row,
                after,
                highlight,
            });
        }
        Ok(lines)
    })?;
    let now: BTreeSet<&str> = lines.iter().map(|line| line.transaction.name()).collect();
    preview.names_now = match now.len() {
        0 => None,
        1 => now.first().map(|name| NamesNow::One((*name).to_owned())),
        n => Some(NamesNow::Several(n)),
    };
    preview.names_merged = lines
        .iter()
        .filter(|line| line.after.changes())
        .map(|line| line.transaction.name())
        .collect::<HashSet<_>>()
        .len();
    preview.matches = lines.len();
    preview.kept = lines
        .iter()
        .filter(|line| line.after == After::Stays)
        .count();
    preview.lines = listed(lines);
    Ok(())
}

fn transfer_effect(subject: &Subject) -> Option<TransferEffect> {
    let Subject::TransferPair {
        outflow: Some(outflow),
        inflow: Some(inflow),
    } = subject
    else {
        return None;
    };
    Some(TransferEffect {
        month: Period::containing(outflow.date),
        income_less: is_income(inflow.now.category(), inflow.amount).then_some(inflow.amount),
        spending_less: is_spending(outflow.now.category(), outflow.amount)
            .then(|| Money::new(-outflow.amount.minor(), outflow.amount.currency())),
    })
}

fn link_effect(preview: &Preview) -> Option<LinkEffect> {
    let Subject::RefundLink {
        refund: Some(refund),
        purchase: Some(purchase),
    } = &preview.subject
    else {
        return None;
    };
    let Detail::RefundLink(link) = &preview.impact.detail else {
        return None;
    };
    let bought = -purchase.amount.minor();
    let purchase_category = purchase.now.category();
    Some(LinkEffect {
        month: Period::containing(refund.date),
        refunded: Money::new(bought - link.purchase_left.minor(), link.amount.currency()),
        income_less: is_income(refund.now.category(), refund.amount).then_some(link.amount),
        spending_less: is_spending(purchase_category, purchase.amount)
            .then(|| (purchase_category.cloned(), link.amount)),
    })
}

/// Where `pattern` matches `row`'s bank text, if it can be found there:
/// the part the app shows in `text-strong`. Cosmetic: a rule on the bank's
/// merchant name or category may match text the description doesn't
/// have, and then nothing is highlighted.
fn highlight(
    field: Option<MatchField>,
    pattern: &Pattern,
    row: &TransactionRef,
) -> Option<Range<usize>> {
    if matches!(
        field,
        Some(MatchField::SourceCategory | MatchField::SourceType)
    ) {
        return None;
    }
    let found = match pattern.kind() {
        MatchKind::Regex => pattern.regex()?.find(&row.bank_text),
        _ => literal(pattern.as_str())?.find(&row.bank_text),
    };
    found.filter(|m| !m.is_empty()).map(|m| m.range())
}

/// A case-insensitive regex for literal `text`, any run of whitespace in
/// it matching any other.
fn literal(text: &str) -> Option<Regex> {
    let words: Vec<String> = text.split_whitespace().map(regex::escape).collect();
    if words.is_empty() {
        return None;
    }
    RegexBuilder::new(&words.join(r"\s+"))
        .case_insensitive(true)
        .build()
        .ok()
}

// A categorisation another suggestion covers -----------------------------------

/// A pending rule that would settle a categorisation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Covered {
    pub rule: SuggestionId,
    pub definition: RuleDefinition,
    /// Past transactions the rule would recategorise besides the
    /// categorisation's own.
    pub others: usize,
}

/// The first pending rule whose approval would settle pending
/// categorisation `id` (decision record "Inbox impact preview", point 4),
/// so the Inbox can point at it. `None` for anything else.
pub fn covered_by(db: &Database, id: SuggestionId) -> Result<Option<Covered>, ApproveError> {
    let suggestion = get_in(db.conn(), id)?;
    let Payload::Categorization(items) = &suggestion.payload else {
        return Ok(None);
    };
    if suggestion.status != SuggestionStatus::Pending {
        return Ok(None);
    }
    let rules = super::list(
        db,
        &super::Filter {
            status: Some(SuggestionStatus::Pending),
            kind: Some(SuggestionKind::Rule),
        },
    )?;
    for rule in rules {
        let Payload::Rule(definition) = &rule.payload else {
            continue;
        };
        let changed = change::dry_run(db, |conn| -> Result<Option<usize>, ApproveError> {
            let Ok(approved) = approve_in(
                conn,
                rule.id,
                ApproveOptions {
                    apply_to_history: true,
                },
            ) else {
                return Ok(None);
            };
            if !approved.settled.contains(&id) {
                return Ok(None);
            }
            let Applied::Rule {
                history: Some(batch),
                ..
            } = approved.applied
            else {
                return Ok(None);
            };
            let rows: i64 = conn.query_row(
                "SELECT COUNT(DISTINCT transaction_id) FROM change_item \
                 WHERE batch_id = ?1 AND field IN ('category', 'split')",
                params![batch],
                |row| row.get(0),
            )?;
            Ok(Some(usize::try_from(rows).unwrap_or(0)))
        })?;
        if let Some(changed) = changed {
            return Ok(Some(Covered {
                rule: rule.id,
                definition: definition.clone(),
                others: changed.saturating_sub(items.len()),
            }));
        }
    }
    Ok(None)
}
