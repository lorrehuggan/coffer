//! What approving a suggestion would change (decision record "Inbox impact
//! preview"). M3.14 reuses it.
//!
//! [`impact`] is computed against today's data each time it is called
//! (point 6), so a later import or edit can't make a stale preview look
//! smaller than the real change. It runs the approval itself in a DB
//! transaction that is rolled back ([`change::dry_run`]), so what it says
//! would be refused ([`Impact::blocked`]) is what [`approve`](super::approve)
//! refuses, and it plans with the same code the approval runs:
//!
//! - **categorization**: how many targets would change, how many of those
//!   have a category the user set (approving an explicit item overwrites
//!   it), how many already have it, and how many are gone;
//! - **rule**: stored first, as approving stores it, then planned against
//!   history ([`rule::preview_apply`]): how many past transactions it
//!   matches, how many it would recategorise if applied to history (only
//!   uncategorised rows and rules' categories, point 1), how many have a
//!   category the user set and are left alone, how many distinct
//!   `merchant_display` values the matches span, and whether any match is
//!   income (money in, or in an income category) it would put in a spending
//!   category. Such a rule, or one over [`WIDE_MERCHANTS`] merchants, is
//!   [wide](RuleImpact::wide) (point 2);
//! - **transfer_pair**: the amount, the two accounts, and that both rows
//!   leave spending and income;
//! - **merchant_alias**: how many rows would be renamed, and that names are
//!   cosmetic: they don't change which rules match (point 5);
//! - **refund_link**: the amount it would link and what each side has left
//!   after.
//!
//! Counts, IDs and amounts only: never a description, note, merchant name
//! or pattern. For the app and the CLI, not MCP.

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use rusqlite::Connection;

use super::approve::{
    Applied, ApproveError, ApproveOptions, Approved, approve_in, plan_categorization,
};
use super::payload::{CategorizationItem, RefundLink, TransferPair};
use super::{Payload, SuggestionId, SuggestionKind, SuggestionStatus, get_in};
use crate::change;
use crate::db::Database;
use crate::id::{AccountId, TransactionId};
use crate::link::{self, LinkKind};
use crate::money::{Currency, Money};
use crate::rule::{self, MatchInput, RuleAction, RuleDefinition, RuleSet};

#[cfg(test)]
#[path = "impact_tests.rs"]
mod tests;

/// A rule whose matches span more distinct merchants than this is wide
/// (decision record "Inbox impact preview", point 2).
pub const WIDE_MERCHANTS: usize = 5;

/// What approving a suggestion would change, now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Impact {
    pub id: SuggestionId,
    pub detail: Detail,
    /// Why approving it now would be refused: the approval's own error
    /// message, which names IDs, keys and amounts only. `None` when it
    /// would go through.
    pub blocked: Option<String>,
}

impl Impact {
    pub const fn kind(&self) -> SuggestionKind {
        match self.detail {
            Detail::Categorization(_) => SuggestionKind::Categorization,
            Detail::Rule(_) => SuggestionKind::Rule,
            Detail::TransferPair(_) => SuggestionKind::TransferPair,
            Detail::MerchantAlias(_) => SuggestionKind::MerchantAlias,
            Detail::RefundLink(_) => SuggestionKind::RefundLink,
        }
    }

    /// Whether it is a wide rule, which "Approve all" skips (point 3).
    pub const fn wide(&self) -> bool {
        matches!(&self.detail, Detail::Rule(rule) if rule.wide)
    }
}

/// The impact by kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detail {
    Categorization(CategorizationImpact),
    Rule(RuleImpact),
    TransferPair(TransferImpact),
    MerchantAlias(AliasImpact),
    RefundLink(LinkImpact),
}

/// What a categorisation would change, target by target (a transaction or
/// a split line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CategorizationImpact {
    /// The suggestion's items.
    pub targets: usize,
    /// Targets whose category would change.
    pub changes: usize,
    /// Of those, targets with a category the user set (`user` or
    /// `ai_approved`; any category on a split line): approving overwrites
    /// them.
    pub user_set: usize,
    /// Targets that already have the category: left as they are.
    pub unchanged: usize,
    /// Targets gone since (deleted, undone, split again) or a category
    /// gone: approving is refused while there are any.
    pub missing: usize,
}

/// What a rule would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleImpact {
    /// Past transactions it matches.
    pub matches: usize,
    /// Of those, rows applying it to history would recategorise (or split):
    /// uncategorised rows and rows with a rule's or the preset's category.
    pub recategorises: usize,
    /// Rows applying it to history would change in any way (names and tags
    /// too).
    pub changes: usize,
    /// Matched rows with a category the user set that it would otherwise
    /// change: left alone (point 1).
    pub user_set: usize,
    /// Matched rows that are split: skipped by an apply.
    pub split_rows: usize,
    /// Distinct `merchant_display` values among the matches.
    pub merchants: usize,
    /// Whether it would put a match that is income (money in, or in an
    /// income category) into a spending category.
    pub moves_income: bool,
    /// More than [`WIDE_MERCHANTS`] merchants, or moves income.
    pub wide: bool,
    /// Why applying it to history would be refused, when storing it alone
    /// wouldn't be.
    pub history_blocked: Option<String>,
}

/// What a transfer pair would confirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferImpact {
    /// What moves: positive.
    pub amount: Money,
    /// The account money leaves.
    pub from_account: AccountId,
    /// The account it arrives in.
    pub to_account: AccountId,
    /// Detection already suggested this pair: approving confirms it.
    pub suggested: bool,
}

/// What a merchant name would rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AliasImpact {
    /// Transactions whose shown name would change.
    pub renames: usize,
}

/// What a refund or reimbursement link would link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkImpact {
    pub kind: LinkKind,
    /// The amount it would link: the suggestion's, or as much as both
    /// sides can take now.
    pub amount: Money,
    /// What the money in would have left to link after.
    pub refund_left: Money,
    /// What the purchase could still take after.
    pub purchase_left: Money,
}

/// What approving pending suggestion `id` would change now. Reads only:
/// whatever it runs is rolled back.
///
/// A decided suggestion is [`ApproveError::NotPending`]. A transfer, link or
/// rule whose references no longer fit has nothing to preview and is
/// [`ApproveError::Invalid`]; a categorisation counts its missing targets
/// instead.
pub fn impact(db: &Database, id: SuggestionId) -> Result<Impact, ApproveError> {
    let suggestion = get_in(db.conn(), id)?;
    if suggestion.status != SuggestionStatus::Pending {
        return Err(ApproveError::NotPending {
            id,
            status: suggestion.status,
        });
    }
    let approval = change::dry_run(db, |conn| {
        Ok::<_, ApproveError>(approve_in(conn, id, ApproveOptions::default()))
    })?;
    let detail = match &suggestion.payload {
        Payload::Categorization(items) => Detail::Categorization(categorization(db.conn(), items)?),
        Payload::Rule(definition) => {
            suggestion.payload.check(db.conn())?;
            Detail::Rule(rule_impact(db, definition)?)
        }
        Payload::TransferPair(pair) => {
            suggestion.payload.check(db.conn())?;
            Detail::TransferPair(transfer(db.conn(), pair)?)
        }
        Payload::MerchantAlias(_) => Detail::MerchantAlias(alias_impact(&approval)),
        Payload::RefundLink(refund) => {
            suggestion.payload.check(db.conn())?;
            Detail::RefundLink(refund_link(db.conn(), refund)?)
        }
    };
    Ok(Impact {
        id,
        detail,
        blocked: approval.err().map(|error| error.to_string()),
    })
}

fn categorization(
    conn: &Connection,
    items: &[CategorizationItem],
) -> Result<CategorizationImpact, ApproveError> {
    let plan = plan_categorization(conn, items)?;
    Ok(CategorizationImpact {
        targets: items.len(),
        changes: plan.changes,
        user_set: plan.user_set,
        unchanged: plan.unchanged,
        missing: plan.missing,
    })
}

fn rule_impact(db: &Database, definition: &RuleDefinition) -> Result<RuleImpact, ApproveError> {
    change::dry_run(db, |conn| {
        let rule = rule::create_at_in(conn, definition, 0)?;
        let preview = rule::preview_apply_in(conn, rule)?;

        let is_category = |target: &rule::Target| {
            matches!(
                target,
                rule::Target::Category { .. } | rule::Target::Split { .. }
            )
        };
        let mut recategorises = 0;
        let mut user_set = 0;
        for row in &preview.rows {
            let category = row.changes.iter().filter(|change| is_category(&change.new));
            let mut automatic = false;
            let mut users = false;
            for change in category {
                automatic |= change.applies(false);
                users |= change.user_set;
            }
            recategorises += usize::from(automatic);
            user_set += usize::from(users);
        }

        let spending = sets_spending(conn, definition)?;
        let rules = RuleSet::load(conn)?;
        let mut matches = 0;
        let mut merchants = HashSet::new();
        let mut moves_income = false;
        let mut statement = conn.prepare(
            "SELECT t.account_id, t.amount_minor, t.merchant, t.description_raw, \
             t.source_category, t.source_type, t.merchant_display, c.kind \
             FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id ORDER BY t.id",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let merchant: Option<String> = row.get(2)?;
            let description: String = row.get(3)?;
            let source_category: Option<String> = row.get(4)?;
            let source_type: Option<String> = row.get(5)?;
            let input = MatchInput {
                account_id: row.get(0)?,
                amount_minor: row.get(1)?,
                merchant: merchant.as_deref(),
                description: &description,
                source_category: source_category.as_deref(),
                source_type: source_type.as_deref(),
            };
            if rules.evaluate_rule(rule, &input).is_none() {
                continue;
            }
            matches += 1;
            if let Some(name) = row.get::<_, Option<String>>(6)? {
                merchants.insert(name);
            }
            let kind: Option<String> = row.get(7)?;
            moves_income |=
                spending && (input.amount_minor > 0 || kind.as_deref() == Some("income"));
        }
        drop(rows);
        drop(statement);

        let history_blocked = rule::commit_apply_in(conn, &preview, &BTreeSet::new())
            .err()
            .map(|error| ApproveError::from(error).to_string());
        Ok(RuleImpact {
            matches,
            recategorises,
            changes: preview.automatic_rows().count(),
            user_set,
            split_rows: preview.split_rows.len(),
            merchants: merchants.len(),
            moves_income,
            wide: moves_income || merchants.len() > WIDE_MERCHANTS,
            history_blocked,
        })
    })
}

/// Whether `definition` puts a row (or a split line) in a spending category.
fn sets_spending(conn: &Connection, definition: &RuleDefinition) -> rusqlite::Result<bool> {
    let mut keys = Vec::new();
    for action in definition.actions() {
        match action {
            RuleAction::SetCategory(key) => keys.push(key.clone()),
            RuleAction::SplitTemplate(template) => {
                keys.extend(template.lines().iter().map(|line| line.category().clone()));
            }
            _ => {}
        }
    }
    let mut statement =
        conn.prepare_cached("SELECT 1 FROM category WHERE key = ?1 AND kind = 'spending'")?;
    for key in keys {
        if statement.exists([key.as_str()])? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn transfer(conn: &Connection, pair: &TransferPair) -> Result<TransferImpact, ApproveError> {
    let side = |id: TransactionId| -> rusqlite::Result<(AccountId, i64, Currency, Option<i64>)> {
        conn.query_row(
            "SELECT account_id, amount_minor, currency, transfer_group_id \
             FROM \"transaction\" WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
    };
    let (from_account, _, _, out_group) = side(pair.outflow)?;
    let (to_account, amount, currency, in_group) = side(pair.inflow)?;
    Ok(TransferImpact {
        amount: Money::new(amount, currency),
        from_account,
        to_account,
        suggested: out_group.is_some() && out_group == in_group,
    })
}

fn alias_impact(approval: &Result<Approved, ApproveError>) -> AliasImpact {
    let renames = match approval {
        Ok(Approved {
            applied: Applied::MerchantAlias { renamed, .. },
            ..
        }) => *renamed,
        _ => 0,
    };
    AliasImpact { renames }
}

fn refund_link(conn: &Connection, refund: &RefundLink) -> Result<LinkImpact, ApproveError> {
    let inflow_left = link::inflow_left_of(conn, refund.refund).map_err(ApproveError::from)?;
    let purchase_left =
        link::purchase_left_of(conn, refund.purchase).map_err(ApproveError::from)?;
    let amount = refund
        .amount_minor
        .unwrap_or_else(|| inflow_left.minor().min(purchase_left.minor()))
        .max(0);
    let currency = inflow_left.currency();
    Ok(LinkImpact {
        kind: refund.kind,
        amount: Money::new(amount, currency),
        refund_left: Money::new(inflow_left.minor() - amount, currency),
        purchase_left: Money::new(purchase_left.minor() - amount, currency),
    })
}

// Display -----------------------------------------------------------------------

/// `count` and the noun, singular or plural.
fn count(f: &mut fmt::Formatter<'_>, count: usize, one: &str, many: &str) -> fmt::Result {
    write!(f, "{count} {}", if count == 1 { one } else { many })
}

/// "has" or "have" for `count`.
fn has(count: usize) -> &'static str {
    if count == 1 { "has" } else { "have" }
}

impl fmt::Display for Impact {
    /// One line for the user, e.g. "recategorises 143 past transactions; 12
    /// have a category you set (left alone)".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            Detail::Categorization(c) => {
                f.write_str("sets the category of ")?;
                count(f, c.changes, "target", "targets")?;
                if c.user_set > 0 {
                    write!(
                        f,
                        "; {} {} a category you set, which approving overwrites",
                        c.user_set,
                        has(c.user_set)
                    )?;
                }
                if c.unchanged > 0 {
                    write!(f, "; {} already {} it", c.unchanged, has(c.unchanged))?;
                }
                if c.missing > 0 {
                    write!(
                        f,
                        "; {} {} gone",
                        c.missing,
                        if c.missing == 1 { "is" } else { "are" }
                    )?;
                }
            }
            Detail::Rule(r) => {
                f.write_str("matches ")?;
                count(f, r.matches, "past transaction", "past transactions")?;
                f.write_str(" across ")?;
                count(f, r.merchants, "merchant", "merchants")?;
                f.write_str("; recategorises ")?;
                count(f, r.recategorises, "past transaction", "past transactions")?;
                f.write_str(" if applied to history")?;
                if r.user_set > 0 {
                    write!(
                        f,
                        "; {} {} a category you set (left alone)",
                        r.user_set,
                        has(r.user_set)
                    )?;
                }
                if r.split_rows > 0 {
                    write!(f, "; {} split ones are skipped", r.split_rows)?;
                }
                if r.wide {
                    f.write_str("; wide: ")?;
                    match (r.merchants > WIDE_MERCHANTS, r.moves_income) {
                        (true, true) => {
                            f.write_str("more than 5 merchants, and it moves income into spending")?
                        }
                        (true, false) => f.write_str("more than 5 merchants")?,
                        _ => f.write_str("it moves income into spending")?,
                    }
                }
                if let Some(reason) = &r.history_blocked {
                    write!(f, "; can't be applied to history now: {reason}")?;
                }
            }
            Detail::TransferPair(t) => {
                write!(
                    f,
                    "marks {} from account {} to account {} as a transfer: both rows leave \
                     spending and income",
                    t.amount, t.from_account, t.to_account
                )?;
                if t.suggested {
                    f.write_str(" (Coffer suggested this pair too)")?;
                }
            }
            Detail::MerchantAlias(a) => {
                f.write_str("renames ")?;
                count(f, a.renames, "transaction", "transactions")?;
                f.write_str("; names are cosmetic and don't change which rules match")?;
            }
            Detail::RefundLink(l) => {
                write!(
                    f,
                    "links {} as a {}; {} left on the money in, {} left on the purchase",
                    l.amount,
                    l.kind.as_str(),
                    l.refund_left,
                    l.purchase_left
                )?;
            }
        }
        if let Some(reason) = &self.blocked {
            write!(f, "; can't be approved now: {reason}")?;
        }
        Ok(())
    }
}
