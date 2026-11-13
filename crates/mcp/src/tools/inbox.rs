//! Inbox writes: `propose_categorizations`, `propose_rule`,
//! `propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link`,
//! `save_report` (ROADMAP M2.3, M2.14). Registered with
//! [`Writes::Inbox`]; they write through [`Call::inbox`] only. `save_report`
//! is `aggregate`, the proposals `standard`.
//!
//! Each proposal turns its typed arguments into the JSON payload of its
//! suggestion kind, in exactly the shape `coffer_core::inbox::payload`
//! parses, and files it. Core checks the shape and every ID and key it
//! names; a refusal comes back to the client as an invalid-arguments error
//! naming IDs, keys and limits, and nothing is written. The source is
//! always the server's client name, never an argument.
//!
//! Results say what was filed, by ID, and that the user decides in Coffer.
//! They never echo the client's text (patterns, names, rationales, report
//! bodies): that is stored as untrusted text and shown only in Coffer.

use coffer_core::inbox::{NewReport, NewSuggestion, SuggestionKind};
use coffer_core::link::LinkKind;
use coffer_core::merchant::AliasKind;
use coffer_core::rule::{ActionKind, MatchField, MatchKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::registry::{AccessLevel, Call, Registry, Reply, ToolError, ToolSpec, Writes};

/// Adds this group's tools.
pub fn register(registry: &mut Registry) {
    registry.add(PROPOSE_CATEGORIZATIONS, propose_categorizations);
    registry.add(PROPOSE_RULE, propose_rule);
    registry.add(PROPOSE_TRANSFER_PAIR, propose_transfer_pair);
    registry.add(PROPOSE_MERCHANT_ALIAS, propose_merchant_alias);
    registry.add(PROPOSE_REFUND_LINK, propose_refund_link);
    registry.add(SAVE_REPORT, save_report);
}

pub const PROPOSE_CATEGORIZATIONS: ToolSpec = ToolSpec {
    name: "propose_categorizations",
    description: "Suggests categories for transactions (by transaction_id) or for lines of \
                  split transactions (by split_id), up to 100 at once, using category keys from \
                  list_categories. Nothing changes yet: the suggestion waits in Coffer's inbox \
                  until the user approves or rejects it. Proposing the same thing again returns \
                  the pending suggestion.",
    level: AccessLevel::Standard,
    writes: Writes::Inbox,
};

pub const PROPOSE_RULE: ToolSpec = ToolSpec {
    name: "propose_rule",
    description: "Suggests a rule that acts on matching transactions, now and at future \
                  imports. A suggested rule may only use the actions set_category, \
                  split_template, set_merchant and add_tag; it never excludes rows, writes \
                  notes or changes shares. Nothing changes yet: the rule waits in Coffer's inbox \
                  until the user approves it.",
    level: AccessLevel::Standard,
    writes: Writes::Inbox,
};

pub const PROPOSE_TRANSFER_PAIR: ToolSpec = ToolSpec {
    name: "propose_transfer_pair",
    description: "Suggests that two transactions are one transfer between the user's own \
                  accounts: money out of one account and the same amount into another, in the \
                  same currency. Nothing changes yet: the suggestion waits in Coffer's inbox \
                  until the user approves it.",
    level: AccessLevel::Standard,
    writes: Writes::Inbox,
};

pub const PROPOSE_MERCHANT_ALIAS: ToolSpec = ToolSpec {
    name: "propose_merchant_alias",
    description: "Suggests a clean display name for the merchants whose cleaned bank text \
                  matches a pattern (exactly, by prefix, or by regular expression). Nothing \
                  changes yet: the alias waits in Coffer's inbox until the user approves it.",
    level: AccessLevel::Standard,
    writes: Writes::Inbox,
};

pub const PROPOSE_REFUND_LINK: ToolSpec = ToolSpec {
    name: "propose_refund_link",
    description: "Suggests linking money in (a refund or a reimbursement) to the purchase it \
                  pays back, in the same currency. Nothing changes yet: the link waits in \
                  Coffer's inbox until the user approves it.",
    level: AccessLevel::Standard,
    writes: Writes::Inbox,
};

pub const SAVE_REPORT: ToolSpec = ToolSpec {
    name: "save_report",
    description: "Saves a written review of the user's finances for a period, such as a \
                  monthly summary, so the user can read it in Coffer. The body is plain \
                  markdown: Coffer shows it as AI-written text, without HTML, images or links.",
    level: AccessLevel::Aggregate,
    writes: Writes::Inbox,
};

// Results -----------------------------------------------------------------------

/// What a proposal did.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Proposed {
    /// The suggestion's ID in Coffer's inbox.
    pub suggestion_id: i64,
    /// Always `pending`: the user approves or rejects it in Coffer.
    pub status: Status,
    /// An identical suggestion was already pending: `suggestion_id` is that
    /// one, and nothing new was filed.
    pub already_pending: bool,
    /// What happens next.
    pub next_step: &'static str,
}

/// Where a filed suggestion stands.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting for the user.
    Pending,
}

const NEXT_STEP: &str = "The user reviews it in Coffer's inbox; nothing changes unless they \
                         approve it.";

/// What `save_report` did.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Saved {
    /// The report's ID.
    pub report_id: i64,
    /// What happens next.
    pub next_step: &'static str,
}

/// Files `payload` as a suggestion of `kind`.
fn file(
    call: &Call<'_>,
    kind: SuggestionKind,
    payload: Value,
    rationale: Option<String>,
) -> Result<Reply<Proposed>, ToolError> {
    let filed = call.inbox()?.file(&NewSuggestion {
        kind,
        payload,
        rationale,
    })?;
    Ok(Reply::new(
        Proposed {
            suggestion_id: filed.id.get(),
            status: Status::Pending,
            already_pending: filed.duplicate,
            next_step: NEXT_STEP,
        },
        1,
    ))
}

// propose_categorizations ----------------------------------------------------------

/// `propose_categorizations`' arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CategorizationArgs {
    /// The categories to suggest: 1 to 100 items, each naming one
    /// transaction or one split line, none twice.
    pub items: Vec<CategorizationItem>,
    /// Why, in a sentence or two (at most 1000 characters). The user reads
    /// it when deciding.
    #[serde(default)]
    pub rationale: Option<String>,
}

/// One suggested category.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CategorizationItem {
    /// The transaction to categorise. Give this or `split_id`, not both. A
    /// split transaction is categorised by its lines instead.
    #[serde(default)]
    pub transaction_id: Option<i64>,
    /// The split line to categorise. Give this or `transaction_id`, not
    /// both.
    #[serde(default)]
    pub split_id: Option<i64>,
    /// A category key from `list_categories`, e.g. `food.groceries`.
    pub category: String,
}

fn propose_categorizations(
    call: &Call<'_>,
    args: CategorizationArgs,
) -> Result<Reply<Proposed>, ToolError> {
    let items: Vec<Value> = args
        .items
        .into_iter()
        .map(|item| {
            let mut object = serde_json::Map::new();
            if let Some(id) = item.transaction_id {
                object.insert("transaction_id".into(), id.into());
            }
            if let Some(id) = item.split_id {
                object.insert("split_id".into(), id.into());
            }
            object.insert("category".into(), item.category.into());
            Value::Object(object)
        })
        .collect();
    file(
        call,
        SuggestionKind::Categorization,
        json!({ "items": items }),
        args.rationale,
    )
}

// propose_rule -------------------------------------------------------------------

/// `propose_rule`'s arguments: what the rule matches and what it does.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RuleArgs {
    /// The transaction field the pattern is tested against.
    pub match_field: RuleField,
    /// How the pattern is tested. Every kind ignores case.
    pub match_kind: RuleMatch,
    /// The text (or regular expression) to match.
    pub pattern: String,
    /// Only rows with an amount at least this, in integer minor units
    /// (money out is negative).
    #[serde(default)]
    pub amount_min: Option<i64>,
    /// Only rows with an amount at most this, in integer minor units
    /// (money out is negative).
    #[serde(default)]
    pub amount_max: Option<i64>,
    /// Only rows on this account (an ID from `list_accounts`).
    #[serde(default)]
    pub account_id: Option<i64>,
    /// What the rule does to a matching row: at least one action, each
    /// kind at most once.
    pub actions: Vec<RuleActionArg>,
    /// Why, in a sentence or two (at most 1000 characters). The user reads
    /// it when deciding.
    #[serde(default)]
    pub rationale: Option<String>,
}

/// The field a rule matches.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleField {
    /// The cleaned bank merchant name, before any merchant alias applies.
    Merchant,
    /// The bank's description text.
    Description,
    /// The category the bank's export gives the row.
    SourceCategory,
    /// The transaction type the bank's export gives the row.
    SourceType,
}

impl From<RuleField> for MatchField {
    fn from(field: RuleField) -> MatchField {
        match field {
            RuleField::Merchant => MatchField::Merchant,
            RuleField::Description => MatchField::Description,
            RuleField::SourceCategory => MatchField::SourceCategory,
            RuleField::SourceType => MatchField::SourceType,
        }
    }
}

/// How a rule's pattern is tested.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatch {
    /// The whole field equals the pattern.
    Is,
    /// The field starts with the pattern.
    StartsWith,
    /// The pattern appears anywhere in the field.
    Contains,
    /// The pattern is a regular expression (Rust `regex` syntax).
    Regex,
}

impl From<RuleMatch> for MatchKind {
    fn from(kind: RuleMatch) -> MatchKind {
        match kind {
            RuleMatch::Is => MatchKind::Is,
            RuleMatch::StartsWith => MatchKind::StartsWith,
            RuleMatch::Contains => MatchKind::Contains,
            RuleMatch::Regex => MatchKind::Regex,
        }
    }
}

/// One action of a suggested rule.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleActionArg {
    /// What the action does.
    pub kind: RuleActionKind,
    /// The action's value, by kind: `set_category` a category key
    /// (`"food.groceries"`); `set_merchant` the display name (`"Uber"`);
    /// `add_tag` a list of tag names (`["work", "travel"]`);
    /// `split_template` a list of lines whose integer basis points sum to
    /// 10000 (`[{"category": "food", "basis_points": 6000}, {"category":
    /// "home", "basis_points": 4000}]`).
    pub value: Value,
}

/// The actions a suggested rule may have.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleActionKind {
    /// Sets the category.
    SetCategory,
    /// Splits the row into category lines.
    SplitTemplate,
    /// Sets the display merchant.
    SetMerchant,
    /// Adds tags.
    AddTag,
}

impl From<RuleActionKind> for ActionKind {
    fn from(kind: RuleActionKind) -> ActionKind {
        match kind {
            RuleActionKind::SetCategory => ActionKind::SetCategory,
            RuleActionKind::SplitTemplate => ActionKind::SplitTemplate,
            RuleActionKind::SetMerchant => ActionKind::SetMerchant,
            RuleActionKind::AddTag => ActionKind::AddTag,
        }
    }
}

fn propose_rule(call: &Call<'_>, args: RuleArgs) -> Result<Reply<Proposed>, ToolError> {
    let actions: Vec<Value> = args
        .actions
        .into_iter()
        .map(|action| {
            json!({
                "kind": ActionKind::from(action.kind).as_str(),
                "value": action.value,
            })
        })
        .collect();
    let mut payload = serde_json::Map::new();
    payload.insert(
        "match_field".into(),
        MatchField::from(args.match_field).as_str().into(),
    );
    payload.insert(
        "match_kind".into(),
        MatchKind::from(args.match_kind).as_str().into(),
    );
    payload.insert("pattern".into(), args.pattern.into());
    for (key, value) in [
        ("amount_min", args.amount_min),
        ("amount_max", args.amount_max),
        ("account_id", args.account_id),
    ] {
        if let Some(value) = value {
            payload.insert(key.into(), value.into());
        }
    }
    payload.insert("actions".into(), actions.into());
    file(
        call,
        SuggestionKind::Rule,
        Value::Object(payload),
        args.rationale,
    )
}

// propose_transfer_pair ----------------------------------------------------------

/// `propose_transfer_pair`'s arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransferPairArgs {
    /// The money out: a transaction with a negative amount.
    pub outflow_id: i64,
    /// The money in on another account: the same amount, positive.
    pub inflow_id: i64,
    /// Why, in a sentence or two (at most 1000 characters). The user reads
    /// it when deciding.
    #[serde(default)]
    pub rationale: Option<String>,
}

fn propose_transfer_pair(
    call: &Call<'_>,
    args: TransferPairArgs,
) -> Result<Reply<Proposed>, ToolError> {
    file(
        call,
        SuggestionKind::TransferPair,
        json!({ "outflow_id": args.outflow_id, "inflow_id": args.inflow_id }),
        args.rationale,
    )
}

// propose_merchant_alias ---------------------------------------------------------

/// `propose_merchant_alias`'s arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MerchantAliasArgs {
    /// What to match in the cleaned bank merchant text, e.g. `TESCO STORES`.
    pub pattern: String,
    /// The name to show instead, e.g. `Tesco`: at most 100 characters.
    pub display_name: String,
    /// How the pattern is tested; `exact` if left out. Every kind ignores
    /// case.
    #[serde(default)]
    pub match_kind: Option<AliasMatch>,
    /// Why, in a sentence or two (at most 1000 characters). The user reads
    /// it when deciding.
    #[serde(default)]
    pub rationale: Option<String>,
}

/// How an alias's pattern is tested.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AliasMatch {
    /// The whole cleaned text equals the pattern.
    Exact,
    /// The cleaned text starts with the pattern.
    Prefix,
    /// The pattern is a regular expression (Rust `regex` syntax).
    Regex,
}

impl From<AliasMatch> for AliasKind {
    fn from(kind: AliasMatch) -> AliasKind {
        match kind {
            AliasMatch::Exact => AliasKind::Exact,
            AliasMatch::Prefix => AliasKind::Prefix,
            AliasMatch::Regex => AliasKind::Regex,
        }
    }
}

fn propose_merchant_alias(
    call: &Call<'_>,
    args: MerchantAliasArgs,
) -> Result<Reply<Proposed>, ToolError> {
    let kind = args.match_kind.map_or(AliasKind::Exact, AliasKind::from);
    file(
        call,
        SuggestionKind::MerchantAlias,
        json!({
            "match_kind": kind.as_str(),
            "pattern": args.pattern,
            "display_name": args.display_name,
        }),
        args.rationale,
    )
}

// propose_refund_link ------------------------------------------------------------

/// `propose_refund_link`'s arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RefundLinkArgs {
    /// The money in: a transaction with a positive amount.
    pub refund_id: i64,
    /// The purchase it pays back: money out, in the same currency.
    pub purchase_id: i64,
    /// What the money in is.
    pub link_kind: RefundKind,
    /// How much of it pays back the purchase, in integer minor units, more
    /// than zero. Leave it out for as much as both sides can still take.
    #[serde(default)]
    pub amount_minor: Option<i64>,
    /// Why, in a sentence or two (at most 1000 characters). The user reads
    /// it when deciding.
    #[serde(default)]
    pub rationale: Option<String>,
}

/// What kind of money in a link is.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefundKind {
    /// The merchant paid the purchase back.
    Refund,
    /// Someone else paid the user back, e.g. an employer for expenses.
    Reimbursement,
}

impl From<RefundKind> for LinkKind {
    fn from(kind: RefundKind) -> LinkKind {
        match kind {
            RefundKind::Refund => LinkKind::Refund,
            RefundKind::Reimbursement => LinkKind::Reimbursement,
        }
    }
}

fn propose_refund_link(
    call: &Call<'_>,
    args: RefundLinkArgs,
) -> Result<Reply<Proposed>, ToolError> {
    let mut payload = json!({
        "refund_id": args.refund_id,
        "purchase_id": args.purchase_id,
        "link_kind": LinkKind::from(args.link_kind).as_str(),
    });
    if let Some(amount) = args.amount_minor {
        payload["amount_minor"] = amount.into();
    }
    file(call, SuggestionKind::RefundLink, payload, args.rationale)
}

// save_report --------------------------------------------------------------------

/// `save_report`'s arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportArgs {
    /// The period it covers, e.g. `2026-10` or `2026-Q3`: at most 32
    /// characters, not blank.
    pub period: String,
    /// A short title: at most 200 characters, not blank.
    pub title: String,
    /// The report in plain markdown (headings, lists, emphasis, tables):
    /// at most 100000 characters. HTML, images and links aren't rendered.
    pub body_markdown: String,
}

fn save_report(call: &Call<'_>, args: ReportArgs) -> Result<Reply<Saved>, ToolError> {
    let id = call.inbox()?.save_report(&NewReport {
        period: args.period,
        title: args.title,
        body_markdown: args.body_markdown,
    })?;
    Ok(Reply::new(
        Saved {
            report_id: id,
            next_step: "The user reads it in Coffer.",
        },
        1,
    ))
}
