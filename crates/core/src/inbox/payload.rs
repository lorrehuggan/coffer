//! What each suggestion kind proposes: one strict JSON shape per
//! [`SuggestionKind`], parsed into a typed [`Payload`] and checked against
//! the database before a suggestion is filed (`inbox` module docs).
//!
//! | Kind | JSON |
//! |---|---|
//! | `categorization` | `{"items": [{"transaction_id": 12, "category": "food.groceries"}, {"split_id": 3, "category": "home.rent"}]}` |
//! | `rule` | `{"match_field": "merchant", "match_kind": "is", "pattern": "TESCO", "amount_min": -5000, "amount_max": 0, "account_id": 1, "actions": [{"kind": "set_category", "value": "food.groceries"}]}` |
//! | `transfer_pair` | `{"outflow_id": 40, "inflow_id": 41}` |
//! | `merchant_alias` | `{"match_kind": "exact", "pattern": "TESCO STORES 1234", "display_name": "Tesco"}` |
//! | `refund_link` | `{"refund_id": 52, "purchase_id": 17, "link_kind": "refund", "amount_minor": 1299}` |
//!
//! Every object refuses unknown or repeated fields, wrong types and floats.
//! `amount_min`, `amount_max` and `account_id` of a rule are optional, and so
//! are `match_kind` of an alias (default `exact`) and `amount_minor` of a
//! link (default: as much as both sides can still take, worked out when it
//! is approved). A rule action's `value` is the action's `value_json` shape
//! (`rule` module docs, decision record "rules", point 9).
//!
//! **Shape** ([`Payload::parse`]) is checked without the database:
//! - a categorisation has 1 to [`MAX_ITEMS`] items, each naming exactly one
//!   of a transaction or a split line, no target twice, and a valid
//!   category key;
//! - a rule is a valid [`RuleDefinition`], and an agent's rule only
//!   categorises, splits, names, or tags ([`AGENT_RULE_ACTIONS`]): it never
//!   excludes rows, writes the user's note or changes their share;
//! - a transfer pair is two different transactions;
//! - an alias is a valid [`AliasDefinition`] with a display name of at most
//!   [`MAX_DISPLAY_NAME_CHARS`] characters and no control characters;
//! - a link is a refund or a reimbursement (a settlement names a person,
//!   which no client sees) between two different transactions, with a
//!   positive amount if one is given.
//!
//! **References** ([`Payload::check`]) need the database: every transaction,
//! split line, account and category named exists; a categorised
//! transaction isn't split (its lines are categorised instead); a transfer
//! pair is money out and the same amount in, in one currency, on two
//! accounts, neither excluded nor split, and not already in a transfer
//! other than a suggestion of exactly this pair; a refund is money in and
//! its purchase money out in the same currency, and the amount fits both.
//! The rest (caps against other links, transfers, locks) is the approval's
//! to check, against the data as it is then.
//!
//! [`Payload::canonical_json`] is what is stored: one spelling per payload
//! (items sorted by target, actions in [`ActionKind`] order, defaults
//! written out), so identical proposals are identical text and are filed
//! once.
//!
//! Payloads are an agent's text, untrusted: errors here name IDs, keys and
//! limits, never a pattern or a name.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{Error, SuggestionKind};
use crate::category::CategoryKey;
use crate::id::{AccountId, SplitId, TransactionId};
use crate::link::LinkKind;
use crate::merchant::{AliasDefinition, AliasKind, DisplayName};
use crate::money::Currency;
use crate::rule::{
    ActionKind, AmountBounds, MatchField, MatchKind, Pattern, RuleAction, RuleDefinition,
};
use crate::transfer::TransferStatus;

/// The most items one `categorization` suggestion carries.
pub const MAX_ITEMS: usize = 100;

/// The longest display name an alias suggestion gives, in characters.
pub const MAX_DISPLAY_NAME_CHARS: usize = 100;

/// The actions an agent's `rule` suggestion may have. `exclude` would hide
/// rows (what a planted rule would want), `set_note` writes the user's own
/// words, and `my_share` changes how much of a cost is theirs: those stay
/// the user's to make in the app.
pub const AGENT_RULE_ACTIONS: &[ActionKind] = &[
    ActionKind::SetCategory,
    ActionKind::SplitTemplate,
    ActionKind::SetMerchant,
    ActionKind::AddTag,
];

/// A suggestion's payload, parsed and typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Categories for transactions or split lines, sorted by target.
    Categorization(Vec<CategorizationItem>),
    /// A new rule.
    Rule(RuleDefinition),
    /// Two transactions that are one transfer.
    TransferPair(TransferPair),
    /// A display name for the merchants an alias matches.
    MerchantAlias(AliasDefinition),
    /// Money in linked to the purchase it pays back.
    RefundLink(RefundLink),
}

/// What a categorisation item categorises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// An unsplit transaction.
    Transaction(TransactionId),
    /// One line of a split transaction.
    SplitLine(SplitId),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Transaction(id) => write!(f, "transaction {id}"),
            Target::SplitLine(id) => write!(f, "split line {id}"),
        }
    }
}

/// One categorisation: a target and the category it should have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorizationItem {
    pub target: Target,
    pub category: CategoryKey,
}

/// A proposed transfer: `outflow` is money out, `inflow` the same amount in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferPair {
    pub outflow: TransactionId,
    pub inflow: TransactionId,
}

/// A proposed refund or reimbursement link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundLink {
    /// The money in.
    pub refund: TransactionId,
    /// The purchase (money out) it pays back.
    pub purchase: TransactionId,
    /// [`LinkKind::Refund`] or [`LinkKind::Reimbursement`].
    pub kind: LinkKind,
    /// How much, in minor units of both rows' currency. `None`: as much as
    /// both sides can still take when it is approved.
    pub amount_minor: Option<i64>,
}

/// Why a payload was refused. Messages name IDs, keys and limits, never
/// the agent's text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("the {kind} payload isn't {}", expected_shape(*kind))]
    Shape { kind: SuggestionKind },

    #[error("a categorization has between 1 and {max} items")]
    ItemCount { max: usize },

    #[error("a categorization item names exactly one of transaction_id or split_id")]
    ItemTarget,

    #[error("{0} appears twice")]
    RepeatedTarget(Target),

    #[error("a category key is not valid")]
    CategoryKey,

    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    #[error("there is no transaction {0}")]
    TransactionNotFound(TransactionId),

    #[error("there is no split line {0}")]
    SplitLineNotFound(SplitId),

    #[error("there is no account {0}")]
    AccountNotFound(AccountId),

    #[error("transaction {0} is split: categorise its lines (split_id) instead")]
    Split(TransactionId),

    #[error("the rule isn't valid: {0}")]
    Rule(String),

    #[error(
        "a suggested rule can only set_category, split_template, set_merchant or add_tag, \
         not {0}"
    )]
    RuleAction(ActionKind),

    #[error("the two sides must be different transactions")]
    SameTransaction,

    #[error("transactions {0} and {1} aren't one transfer: {2}")]
    NotATransfer(TransactionId, TransactionId, &'static str),

    #[error("the merchant name isn't valid: {0}")]
    Alias(String),

    #[error(
        "a display name is at most {max} characters with no control characters",
        max = MAX_DISPLAY_NAME_CHARS
    )]
    DisplayName,

    #[error("a suggested link is a refund or a reimbursement")]
    LinkKind,

    #[error("transaction {0} isn't money in")]
    NotMoneyIn(TransactionId),

    #[error("transaction {0} isn't money out")]
    NotMoneyOut(TransactionId),

    #[error("transactions {0} and {1} are in different currencies")]
    CurrencyMismatch(TransactionId, TransactionId),

    #[error("a link's amount_minor is more than zero and at most what both sides moved")]
    LinkAmount,
}

fn expected_shape(kind: SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Categorization => {
            "{\"items\": [{\"transaction_id\" or \"split_id\": <integer>, \"category\": <key>}]}"
        }
        SuggestionKind::Rule => {
            "{\"match_field\", \"match_kind\", \"pattern\", optional \"amount_min\", \
             \"amount_max\", \"account_id\", \"actions\": [{\"kind\", \"value\"}]}"
        }
        SuggestionKind::TransferPair => "{\"outflow_id\": <integer>, \"inflow_id\": <integer>}",
        SuggestionKind::MerchantAlias => {
            "{\"pattern\": <text>, \"display_name\": <text>, optional \"match_kind\": \
             \"exact\" | \"prefix\" | \"regex\"}"
        }
        SuggestionKind::RefundLink => {
            "{\"refund_id\": <integer>, \"purchase_id\": <integer>, \"link_kind\": \"refund\" \
             | \"reimbursement\", optional \"amount_minor\": <integer>}"
        }
    }
}

// The JSON shapes --------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCategorization {
    items: Vec<RawItem>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transaction_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    split_id: Option<i64>,
    category: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    match_field: String,
    match_kind: String,
    pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount_min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount_max: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_id: Option<i64>,
    actions: Vec<RawAction>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    kind: String,
    value: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTransferPair {
    outflow_id: i64,
    inflow_id: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAlias {
    #[serde(default = "default_alias_kind")]
    match_kind: String,
    pattern: String,
    display_name: String,
}

fn default_alias_kind() -> String {
    AliasKind::Exact.as_str().to_owned()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRefundLink {
    refund_id: i64,
    purchase_id: i64,
    link_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    amount_minor: Option<i64>,
}

// Parsing -----------------------------------------------------------------------

impl Payload {
    /// The kind this payload is for.
    pub const fn kind(&self) -> SuggestionKind {
        match self {
            Payload::Categorization(_) => SuggestionKind::Categorization,
            Payload::Rule(_) => SuggestionKind::Rule,
            Payload::TransferPair(_) => SuggestionKind::TransferPair,
            Payload::MerchantAlias(_) => SuggestionKind::MerchantAlias,
            Payload::RefundLink(_) => SuggestionKind::RefundLink,
        }
    }

    /// Parses `json` as a payload of `kind` and checks its shape (module
    /// docs). Doesn't look at the database: see [`Payload::check`].
    pub fn parse(kind: SuggestionKind, json: &serde_json::Value) -> Result<Payload, Invalid> {
        fn raw<T: for<'de> Deserialize<'de>>(
            kind: SuggestionKind,
            json: &serde_json::Value,
        ) -> Result<T, Invalid> {
            // serde's message can quote the agent's text; keep only the kind.
            T::deserialize(json).map_err(|_| Invalid::Shape { kind })
        }

        match kind {
            SuggestionKind::Categorization => {
                parse_categorization(raw::<RawCategorization>(kind, json)?)
            }
            SuggestionKind::Rule => parse_rule(raw::<RawRule>(kind, json)?),
            SuggestionKind::TransferPair => {
                let pair = raw::<RawTransferPair>(kind, json)?;
                if pair.outflow_id == pair.inflow_id {
                    return Err(Invalid::SameTransaction);
                }
                Ok(Payload::TransferPair(TransferPair {
                    outflow: TransactionId::new(pair.outflow_id),
                    inflow: TransactionId::new(pair.inflow_id),
                }))
            }
            SuggestionKind::MerchantAlias => parse_alias(raw::<RawAlias>(kind, json)?),
            SuggestionKind::RefundLink => {
                let link = raw::<RawRefundLink>(kind, json)?;
                let link_kind = match LinkKind::from_key(&link.link_kind) {
                    Some(kind @ (LinkKind::Refund | LinkKind::Reimbursement)) => kind,
                    _ => return Err(Invalid::LinkKind),
                };
                if link.refund_id == link.purchase_id {
                    return Err(Invalid::SameTransaction);
                }
                if link.amount_minor.is_some_and(|amount| amount <= 0) {
                    return Err(Invalid::LinkAmount);
                }
                Ok(Payload::RefundLink(RefundLink {
                    refund: TransactionId::new(link.refund_id),
                    purchase: TransactionId::new(link.purchase_id),
                    kind: link_kind,
                    amount_minor: link.amount_minor,
                }))
            }
        }
    }

    /// Parses stored canonical JSON (`suggestion.payload_json`).
    pub fn from_stored(kind: SuggestionKind, json: &str) -> Result<Payload, Invalid> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|_| Invalid::Shape { kind })?;
        Payload::parse(kind, &value)
    }

    /// The canonical JSON stored in `suggestion.payload_json` (module docs).
    pub fn canonical_json(&self) -> String {
        let json = match self {
            Payload::Categorization(items) => serde_json::to_string(&RawCategorization {
                items: items
                    .iter()
                    .map(|item| {
                        let (transaction_id, split_id) = match item.target {
                            Target::Transaction(id) => (Some(id.get()), None),
                            Target::SplitLine(id) => (None, Some(id.get())),
                        };
                        RawItem {
                            transaction_id,
                            split_id,
                            category: item.category.as_str().to_owned(),
                        }
                    })
                    .collect(),
            }),
            Payload::Rule(rule) => serde_json::to_string(&RawRule {
                match_field: rule.field().as_str().to_owned(),
                match_kind: rule.pattern().kind().as_str().to_owned(),
                pattern: rule.pattern().as_str().to_owned(),
                amount_min: rule.amount().min(),
                amount_max: rule.amount().max(),
                account_id: rule.account_id(),
                actions: rule
                    .actions()
                    .iter()
                    .map(|action| RawAction {
                        kind: action.kind().as_str().to_owned(),
                        value: serde_json::from_str(&action.value_json())
                            .expect("an action's value_json is JSON"),
                    })
                    .collect(),
            }),
            Payload::TransferPair(pair) => serde_json::to_string(&RawTransferPair {
                outflow_id: pair.outflow.get(),
                inflow_id: pair.inflow.get(),
            }),
            Payload::MerchantAlias(alias) => serde_json::to_string(&RawAlias {
                match_kind: alias.kind().as_str().to_owned(),
                pattern: alias.pattern().to_owned(),
                display_name: alias.name().as_str().to_owned(),
            }),
            Payload::RefundLink(link) => serde_json::to_string(&RawRefundLink {
                refund_id: link.refund.get(),
                purchase_id: link.purchase.get(),
                link_kind: link.kind.as_str().to_owned(),
                amount_minor: link.amount_minor,
            }),
        };
        json.expect("payloads always encode")
    }

    /// Checks what the payload names against the database (module docs).
    /// Reads only. A refusal is [`Error::Invalid`].
    pub fn check(&self, conn: &Connection) -> Result<(), Error> {
        match self {
            Payload::Categorization(items) => {
                for item in items {
                    category_exists(conn, &item.category)?;
                    match item.target {
                        Target::Transaction(id) => {
                            if load(conn, id)?.split {
                                return Err(Invalid::Split(id).into());
                            }
                        }
                        Target::SplitLine(id) => {
                            let exists = conn
                                .prepare_cached("SELECT 1 FROM transaction_split WHERE id = ?1")?
                                .exists([id])?;
                            if !exists {
                                return Err(Invalid::SplitLineNotFound(id).into());
                            }
                        }
                    }
                }
                Ok(())
            }
            Payload::Rule(rule) => {
                if let Some(account) = rule.account_id() {
                    let exists = conn
                        .prepare_cached("SELECT 1 FROM account WHERE id = ?1")?
                        .exists([account])?;
                    if !exists {
                        return Err(Invalid::AccountNotFound(AccountId::new(account)).into());
                    }
                }
                for action in rule.actions() {
                    match action {
                        RuleAction::SetCategory(key) => category_exists(conn, key)?,
                        RuleAction::SplitTemplate(template) => {
                            for line in template.lines() {
                                category_exists(conn, line.category())?;
                            }
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            Payload::TransferPair(pair) => {
                let (out, inn) = (pair.outflow, pair.inflow);
                let a = load(conn, out)?;
                let b = load(conn, inn)?;
                let refuse = |why| Err(Invalid::NotATransfer(out, inn, why).into());
                if a.account == b.account {
                    return refuse("they are on the same account");
                }
                if a.currency != b.currency {
                    return refuse("they are in different currencies");
                }
                if a.amount >= 0 {
                    return refuse("the outflow isn't money out");
                }
                if a.amount.checked_neg() != Some(b.amount) {
                    return refuse("the inflow isn't the same amount in");
                }
                if a.excluded || b.excluded {
                    return refuse("one of them is excluded");
                }
                if a.split || b.split {
                    return refuse("one of them is split");
                }
                match (a.transfer_group, b.transfer_group) {
                    (None, None) => {}
                    (Some(x), Some(y))
                        if x == y && a.transfer_status == Some(TransferStatus::Suggested) => {}
                    _ => return refuse("one of them is already in a transfer"),
                }
                Ok(())
            }
            Payload::MerchantAlias(_) => Ok(()),
            Payload::RefundLink(link) => {
                let refund = load(conn, link.refund)?;
                let purchase = load(conn, link.purchase)?;
                if refund.amount <= 0 {
                    return Err(Invalid::NotMoneyIn(link.refund).into());
                }
                if purchase.amount >= 0 {
                    return Err(Invalid::NotMoneyOut(link.purchase).into());
                }
                if refund.currency != purchase.currency {
                    return Err(Invalid::CurrencyMismatch(link.refund, link.purchase).into());
                }
                if let Some(amount) = link.amount_minor
                    && (amount > refund.amount || purchase.amount.checked_neg() < Some(amount))
                {
                    return Err(Invalid::LinkAmount.into());
                }
                Ok(())
            }
        }
    }
}

fn parse_categorization(raw: RawCategorization) -> Result<Payload, Invalid> {
    if raw.items.is_empty() || raw.items.len() > MAX_ITEMS {
        return Err(Invalid::ItemCount { max: MAX_ITEMS });
    }
    let mut items = Vec::with_capacity(raw.items.len());
    let mut seen = BTreeSet::new();
    for item in raw.items {
        let target = match (item.transaction_id, item.split_id) {
            (Some(id), None) => Target::Transaction(TransactionId::new(id)),
            (None, Some(id)) => Target::SplitLine(SplitId::new(id)),
            _ => return Err(Invalid::ItemTarget),
        };
        if !seen.insert(target) {
            return Err(Invalid::RepeatedTarget(target));
        }
        let category = CategoryKey::new(&item.category).map_err(|_| Invalid::CategoryKey)?;
        items.push(CategorizationItem { target, category });
    }
    items.sort_by_key(|item| item.target);
    Ok(Payload::Categorization(items))
}

fn parse_rule(raw: RawRule) -> Result<Payload, Invalid> {
    let invalid = |error: &dyn fmt::Display| Invalid::Rule(error.to_string());
    let field = MatchField::from_str(&raw.match_field).map_err(|e| invalid(&e))?;
    let kind = MatchKind::from_str(&raw.match_kind).map_err(|e| invalid(&e))?;
    let pattern = Pattern::new(kind, &raw.pattern).map_err(|e| invalid(&e))?;
    let amount = AmountBounds::new(raw.amount_min, raw.amount_max).map_err(|e| invalid(&e))?;
    let mut actions = Vec::with_capacity(raw.actions.len());
    for action in raw.actions {
        let kind = ActionKind::from_str(&action.kind).map_err(|e| invalid(&e))?;
        if !AGENT_RULE_ACTIONS.contains(&kind) {
            return Err(Invalid::RuleAction(kind));
        }
        let action =
            RuleAction::decode(kind, &action.value.to_string()).map_err(|e| invalid(&e))?;
        actions.push(action);
    }
    let rule = RuleDefinition::new(field, pattern, amount, raw.account_id, actions)
        .map_err(|e| invalid(&e))?;
    Ok(Payload::Rule(rule))
}

fn parse_alias(raw: RawAlias) -> Result<Payload, Invalid> {
    let kind = AliasKind::from_str(&raw.match_kind).map_err(|e| Invalid::Alias(e.to_string()))?;
    let name = DisplayName::new(&raw.display_name).map_err(|e| Invalid::Alias(e.to_string()))?;
    if name.as_str().chars().count() > MAX_DISPLAY_NAME_CHARS
        || name.as_str().chars().any(char::is_control)
    {
        return Err(Invalid::DisplayName);
    }
    let alias = AliasDefinition::new(kind, &raw.pattern, name)
        .map_err(|e| Invalid::Alias(e.to_string()))?;
    Ok(Payload::MerchantAlias(alias))
}

// Database checks ----------------------------------------------------------------

/// What the checks need of a transaction.
struct Row {
    account: AccountId,
    amount: i64,
    currency: Currency,
    excluded: bool,
    split: bool,
    transfer_group: Option<i64>,
    transfer_status: Option<TransferStatus>,
}

fn load(conn: &Connection, id: TransactionId) -> Result<Row, Error> {
    let row = conn
        .prepare_cached(
            "SELECT t.account_id, t.amount_minor, t.currency, t.excluded_reason IS NOT NULL, \
             EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id), \
             t.transfer_group_id, g.status \
             FROM \"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
             WHERE t.id = ?1",
        )?
        .query_row([id], |row| {
            let status: Option<String> = row.get(6)?;
            Ok(Row {
                account: row.get(0)?,
                amount: row.get(1)?,
                currency: row.get(2)?,
                excluded: row.get(3)?,
                split: row.get(4)?,
                transfer_group: row.get(5)?,
                transfer_status: status.as_deref().and_then(TransferStatus::from_key),
            })
        })
        .optional()?;
    Ok(row.ok_or(Invalid::TransactionNotFound(id))?)
}

fn category_exists(conn: &Connection, key: &CategoryKey) -> Result<(), Error> {
    let exists = conn
        .prepare_cached("SELECT 1 FROM category WHERE key = ?1")?
        .exists([key.as_str()])?;
    if exists {
        Ok(())
    } else {
        Err(Invalid::UnknownCategory(key.clone()).into())
    }
}
