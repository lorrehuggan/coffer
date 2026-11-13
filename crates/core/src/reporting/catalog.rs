//! Reference data: `list_accounts`, `list_rules`, `list_reports`.
//!
//! - [`accounts`]: every account with its latest balance (gross, from
//!   [`super::balance::latest_balance`]). Never an institution, account
//!   number or balance-snapshot note.
//! - [`rules`]: every rule in evaluation order, with what its actions do,
//!   minus what never leaves the app ([`ActionSummary`]): private tag
//!   names, note text and share labels.
//! - [`reports`]: the saved reports, newest first, capped; [`report`]: one
//!   by ID.
//!
//! Account names, rule patterns, merchant names and report text are the
//! user's (or an AI client's) own text: nothing here logs them.

use std::collections::HashSet;

use chrono::NaiveDate;
use rusqlite::{OptionalExtension, Row};

use crate::account::{self, AccountKind};
use crate::category::CategoryKey;
use crate::db::Reader;
use crate::id::{AccountId, RuleId};
use crate::money::Currency;
use crate::rule::{
    ActionKind, AmountBounds, ExcludeReason, InvalidRule, MatchField, MatchKind, Pattern,
    RuleAction, RuleDefinition, Share, SplitLine, StoreError,
};
use crate::tag::TagName;

use super::balance::{self, AccountBalance};
use super::{Capped, Error, Limit};

/// An account as the catalog lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSummary {
    pub id: AccountId,
    /// The user's label for the account.
    pub name: String,
    pub kind: AccountKind,
    pub currency: Currency,
    /// A pot's parent account; `None` for every other kind.
    pub parent: Option<AccountId>,
    pub closed_on: Option<NaiveDate>,
    /// The balance at the end of the account's last active day, gross
    /// ([`balance::latest_balance`]). `None` for an account with no
    /// transactions and no stated balance, and for a valuation account
    /// without valuations.
    pub latest: Option<AccountBalance>,
}

/// Every account in the order they were created, with its latest balance.
pub fn accounts(reader: &Reader) -> Result<Vec<AccountSummary>, Error> {
    balance::accounts(reader)?
        .into_iter()
        .map(|account| {
            let latest = balance::latest_balance(reader, &account)?;
            Ok(AccountSummary {
                id: account.id,
                name: account.name,
                kind: account.kind,
                currency: account.currency,
                parent: account.parent,
                closed_on: account.closed_on,
                latest,
            })
        })
        .collect()
}

/// A rule as the catalog lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSummary {
    pub id: RuleId,
    /// `rule.priority`: lower runs first. Only the order means anything.
    pub priority: i64,
    pub field: MatchField,
    pub kind: MatchKind,
    /// The pattern as the user wrote it.
    pub pattern: String,
    /// Inclusive bounds on the signed amount, in minor units of
    /// [`RuleSummary::currency`] or, without one, of the matched row's
    /// currency.
    pub amount: AmountBounds,
    /// The account the rule is limited to.
    pub account: Option<AccountId>,
    /// The currency of the rule's account, which its bounds are in; `None`
    /// for a rule on every account.
    pub currency: Option<Currency>,
    /// One per kind, in [`ActionKind`] order.
    pub actions: Vec<ActionSummary>,
}

/// A rule action as it may leave the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionSummary {
    SetCategory(CategoryKey),
    /// The display merchant the rule sets.
    SetMerchant(String),
    /// The tags the rule adds that aren't private. A private tag's name
    /// never leaves the app: only how many were left out.
    AddTag {
        tags: Vec<TagName>,
        hidden: usize,
    },
    /// The rule sets a note. Its text is a user note, and never leaves the
    /// app.
    SetNote,
    Exclude(ExcludeReason),
    /// Category keys and shares only.
    SplitTemplate(Vec<SplitLine>),
    /// The user's share. A share label is never part of a rule.
    MyShare(Share),
}

impl ActionSummary {
    pub const fn kind(&self) -> ActionKind {
        match self {
            ActionSummary::SetCategory(_) => ActionKind::SetCategory,
            ActionSummary::SetMerchant(_) => ActionKind::SetMerchant,
            ActionSummary::AddTag { .. } => ActionKind::AddTag,
            ActionSummary::SetNote => ActionKind::SetNote,
            ActionSummary::Exclude(_) => ActionKind::Exclude,
            ActionSummary::SplitTemplate(_) => ActionKind::SplitTemplate,
            ActionSummary::MyShare(_) => ActionKind::MyShare,
        }
    }

    fn new(action: &RuleAction, private: &HashSet<String>) -> ActionSummary {
        match action {
            RuleAction::SetCategory(key) => ActionSummary::SetCategory(key.clone()),
            RuleAction::SetMerchant(name) => ActionSummary::SetMerchant(name.as_str().to_owned()),
            RuleAction::AddTag(list) => {
                let (hidden, tags): (Vec<&TagName>, Vec<&TagName>) = list
                    .tags()
                    .iter()
                    .partition(|tag| private.contains(&tag.folded()));
                ActionSummary::AddTag {
                    tags: tags.into_iter().cloned().collect(),
                    hidden: hidden.len(),
                }
            }
            RuleAction::SetNote(_) => ActionSummary::SetNote,
            RuleAction::Exclude(reason) => ActionSummary::Exclude(*reason),
            RuleAction::SplitTemplate(template) => {
                ActionSummary::SplitTemplate(template.lines().to_vec())
            }
            RuleAction::MyShare(share) => ActionSummary::MyShare(*share),
        }
    }
}

/// Every rule in evaluation order (priority, then ID), validated as
/// [`crate::rule::list`] validates them: a stored rule that no longer
/// validates is [`Error::Rule`].
pub fn rules(reader: &Reader) -> Result<Vec<RuleSummary>, Error> {
    let conn = reader.conn();
    let private = private_tags(reader)?;
    let mut statement = conn.prepare_cached(
        "SELECT id, priority, match_field, match_kind, pattern, amount_min, amount_max, \
         account_id FROM rule ORDER BY priority, id",
    )?;
    let raw = statement
        .query_map([], RawRule::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    raw.into_iter()
        .map(|rule| rule.summarise(reader, &private))
        .collect()
}

/// The folded names of the private tags.
fn private_tags(reader: &Reader) -> Result<HashSet<String>, Error> {
    let mut statement = reader
        .conn()
        .prepare_cached("SELECT name FROM tag WHERE private = 1")?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    // A stored name is a valid `TagName` (the schema checks the same
    // form); fold the raw text if one somehow isn't.
    Ok(names
        .iter()
        .map(|name| {
            TagName::new(name)
                .map(|tag| tag.folded())
                .unwrap_or_else(|_| crate::rule::fold(name))
        })
        .collect())
}

/// A `rule` row before validation.
struct RawRule {
    id: RuleId,
    priority: i64,
    field: String,
    kind: String,
    pattern: String,
    amount_min: Option<i64>,
    amount_max: Option<i64>,
    account_id: Option<i64>,
}

impl RawRule {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<RawRule> {
        Ok(RawRule {
            id: row.get("id")?,
            priority: row.get("priority")?,
            field: row.get("match_field")?,
            kind: row.get("match_kind")?,
            pattern: row.get("pattern")?,
            amount_min: row.get("amount_min")?,
            amount_max: row.get("amount_max")?,
            account_id: row.get("account_id")?,
        })
    }

    /// Reads the rule's actions, validates the whole rule as the rule
    /// store does, and leaves out what never leaves the app.
    fn summarise(self, reader: &Reader, private: &HashSet<String>) -> Result<RuleSummary, Error> {
        let conn = reader.conn();
        let id = self.id;
        let invalid = |problem| Error::Rule(StoreError::Invalid { id, problem });
        let field: MatchField = self
            .field
            .parse()
            .map_err(|_| invalid(InvalidRule::MatchField))?;
        let kind: MatchKind = self
            .kind
            .parse()
            .map_err(|_| invalid(InvalidRule::MatchKind))?;
        let pattern =
            Pattern::new(kind, &self.pattern).map_err(|err| invalid(InvalidRule::Pattern(err)))?;
        let amount = AmountBounds::new(self.amount_min, self.amount_max)
            .map_err(|err| invalid(InvalidRule::Amount(err)))?;

        let mut statement = conn.prepare_cached(
            "SELECT kind, value_json FROM rule_action WHERE rule_id = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let actions = rows
            .iter()
            .map(|(kind, value_json)| {
                let kind: ActionKind =
                    kind.parse().map_err(|_| invalid(InvalidRule::ActionKind))?;
                RuleAction::decode(kind, value_json).map_err(|_| invalid(InvalidRule::Action(kind)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let definition = RuleDefinition::new(field, pattern, amount, self.account_id, actions)
            .map_err(|err| invalid(InvalidRule::Definition(err)))?;

        let account = definition.account_id().map(AccountId::new);
        let currency = match account {
            Some(account) => account::find(conn, account)?.map(|account| account.currency),
            None => None,
        };
        Ok(RuleSummary {
            id,
            priority: self.priority,
            field: definition.field(),
            kind: definition.pattern().kind(),
            pattern: definition.pattern().as_str().to_owned(),
            amount: definition.amount(),
            account,
            currency,
            actions: definition
                .actions()
                .iter()
                .map(|action| ActionSummary::new(action, private))
                .collect(),
        })
    }
}

/// A saved report (an AI client's review). Every text field is the
/// client's, untrusted when read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedReport {
    /// `report.id`.
    pub id: i64,
    /// The period it covers, as the client wrote it (e.g. `2026-10`).
    pub period: String,
    pub title: String,
    pub body_markdown: String,
    /// The client that saved it.
    pub source_client: String,
    /// When it was saved: a UTC timestamp, `YYYY-MM-DDTHH:MM:SS.sssZ`.
    pub created_at: String,
}

/// The saved reports, newest first (by `created_at`, then by ID, so of two
/// saved in the same millisecond the later insert comes first), at most
/// `limit`.
pub fn reports(reader: &Reader, limit: Limit) -> Result<Capped<SavedReport>, Error> {
    let mut statement = reader.conn().prepare_cached(
        "SELECT id, period, title, body_markdown, source_client, created_at FROM report \
         ORDER BY created_at DESC, id DESC LIMIT ?1",
    )?;
    // One more than the limit, to tell whether the list was cut.
    let fetch = i64::try_from(limit.get() + 1).unwrap_or(i64::MAX);
    let rows = statement
        .query_map([fetch], saved_report)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Capped::new(rows, limit))
}

/// Saved report `id`, or `None` when there is none.
pub fn report(reader: &Reader, id: i64) -> Result<Option<SavedReport>, Error> {
    Ok(reader
        .conn()
        .query_row(
            "SELECT id, period, title, body_markdown, source_client, created_at FROM report \
             WHERE id = ?1",
            [id],
            saved_report,
        )
        .optional()?)
}

fn saved_report(row: &Row<'_>) -> rusqlite::Result<SavedReport> {
    Ok(SavedReport {
        id: row.get(0)?,
        period: row.get(1)?,
        title: row.get(2)?,
        body_markdown: row.get(3)?,
        source_client: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests;
