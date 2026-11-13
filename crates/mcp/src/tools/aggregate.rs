//! Totals: `monthly_summary`, `spending_by_category`, `budget_status`,
//! `net_worth_history` (ROADMAP M2.2). No transaction rows: these are
//! `aggregate`-level tools.
//!
//! Every result is totals only: no transaction, transaction ID, merchant,
//! description, note, tag or share label. Money is per currency, never
//! converted or added across currencies, as [`Amount`] lists. The queries
//! and their rules (the user's share, refunds netting against their
//! category, liabilities, closed accounts) are in
//! [`coffer_core::reporting`]; these handlers only parse the months and
//! shape the results.

use coffer_core::reporting::budget_status::{self as core_budget, BudgetLine};
use coffer_core::reporting::net_worth::{self, AccountWorth, NetWorthMonth};
use coffer_core::reporting::spending::{self, GroupSpending, SubcategorySpending};
use coffer_core::reporting::summary::{self, MonthSummary};
use coffer_core::reporting::{self, Period, Totals};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::output::Amount;
use crate::registry::{AccessLevel, Call, Registry, Reply, ToolError, ToolSpec, Writes};

/// Adds this group's tools.
pub fn register(registry: &mut Registry) {
    registry.add(MONTHLY_SUMMARY, monthly_summary);
    registry.add(SPENDING_BY_CATEGORY, spending_by_category);
    registry.add(BUDGET_STATUS, budget_status);
    registry.add(NET_WORTH_HISTORY, net_worth_history);
}

/// A reporting error as the client sees it: the caller's mistake (a
/// malformed month, an unknown category) as invalid arguments with core's
/// message, which never holds data from the database; anything else as
/// the generic core error. Shared by the `catalog`, `aggregate` and `rows`
/// tools.
pub(crate) fn reporting_error(error: reporting::Error) -> ToolError {
    if error.is_invalid_input() {
        ToolError::InvalidArguments(error.to_string())
    } else {
        ToolError::core(error)
    }
}

/// Reads a `YYYY-MM` argument.
fn month(text: &str) -> Result<Period, ToolError> {
    text.parse::<Period>().map_err(reporting_error)
}

/// Per-currency totals as amounts, ordered by currency code.
fn amounts(call: &Call<'_>, totals: &Totals) -> Vec<Amount> {
    totals.iter().map(|money| call.amount(money)).collect()
}

// monthly_summary -------------------------------------------------------------

pub const MONTHLY_SUMMARY: ToolSpec = ToolSpec {
    name: "monthly_summary",
    description: "Income, spending and net for every month from `from` through `to` (both \
                  `YYYY-MM`, at most 200 months), empty months included. Totals only. Amounts \
                  are the user's own share (a shared cost counts as their part, a purchase \
                  being claimed back as nothing), per currency and never converted. Refunds \
                  net against spending in the month the money came back. Excluded rows and \
                  transfers between the user's own accounts are left out; uncategorised money \
                  out counts as spending and money in as income.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

/// `monthly_summary`'s arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MonthRange {
    /// The first month, `YYYY-MM`, e.g. `2026-01`.
    pub from: String,
    /// The last month, `YYYY-MM`, included; not before `from`.
    pub to: String,
}

/// `monthly_summary`'s result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MonthlySummary {
    /// One entry per month, oldest first.
    pub months: Vec<SummaryMonth>,
    /// The range was longer than 200 months and was cut after the 200th.
    pub truncated: bool,
}

/// One month's totals, one amount per currency (an empty list: nothing in
/// that month).
#[derive(Debug, Serialize, JsonSchema)]
pub struct SummaryMonth {
    /// `YYYY-MM`.
    pub month: String,
    /// Money earned or received, positive.
    pub income: Vec<Amount>,
    /// Money spent, positive; refunds reduce it (below zero only when they
    /// outweigh purchases).
    pub spending: Vec<Amount>,
    /// Income minus spending.
    pub net: Vec<Amount>,
}

fn monthly_summary(call: &Call<'_>, args: MonthRange) -> Result<Reply<MonthlySummary>, ToolError> {
    let (from, to) = (month(&args.from)?, month(&args.to)?);
    let summary = summary::monthly_summary(call.reader(), from, to).map_err(reporting_error)?;
    let months: Vec<SummaryMonth> = summary
        .items
        .iter()
        .map(|month: &MonthSummary| SummaryMonth {
            month: month.period.key(),
            income: amounts(call, &month.income),
            spending: amounts(call, &month.spending),
            net: amounts(call, &month.net),
        })
        .collect();
    let rows = months.len() as u64;
    Ok(Reply::new(
        MonthlySummary {
            months,
            truncated: summary.truncated,
        },
        rows,
    ))
}

// spending_by_category --------------------------------------------------------

pub const SPENDING_BY_CATEGORY: ToolSpec = ToolSpec {
    name: "spending_by_category",
    description: "What was spent in one month (`YYYY-MM`) per spending category, nested: each \
                  group with what was filed under the group itself, its subcategories and a \
                  group total, plus uncategorised money out and an overall total. Totals only; \
                  only categories with spending appear, in the user's order. Amounts are \
                  positive amounts spent, the user's own share, per currency and never \
                  converted. A refund nets against its purchase's category in the month the \
                  money came back, so a total can go below zero.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

/// `spending_by_category`'s arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OneMonth {
    /// The month, `YYYY-MM`, e.g. `2026-11`.
    pub period: String,
}

/// `spending_by_category`'s result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CategorySpending {
    /// `YYYY-MM`.
    pub period: String,
    /// Groups with spending in the month.
    pub groups: Vec<GroupSpent>,
    /// Uncategorised money out, per currency.
    pub uncategorized: Vec<Amount>,
    /// Every group total plus uncategorised, per currency.
    pub total: Vec<Amount>,
}

/// A top-level spending category's spending.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GroupSpent {
    /// Stable key, e.g. `food`.
    pub key: String,
    /// The user's label.
    pub label: String,
    /// Spent on lines filed under the group itself (not a subcategory);
    /// empty when there are none.
    pub spent_directly: Vec<Amount>,
    /// Subcategories with spending.
    pub subcategories: Vec<SubcategorySpent>,
    /// `spent_directly` plus every subcategory, per currency.
    pub total: Vec<Amount>,
}

/// A subcategory's spending.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SubcategorySpent {
    /// Stable key, e.g. `food.groceries`.
    pub key: String,
    /// The user's label.
    pub label: String,
    /// Spent, per currency.
    pub spent: Vec<Amount>,
}

fn spending_by_category(
    call: &Call<'_>,
    args: OneMonth,
) -> Result<Reply<CategorySpending>, ToolError> {
    let period = month(&args.period)?;
    let spending =
        spending::spending_by_category(call.reader(), period).map_err(reporting_error)?;
    let subcategory = |child: &SubcategorySpending| SubcategorySpent {
        key: child.key.as_str().to_owned(),
        label: child.label.clone(),
        spent: amounts(call, &child.spent),
    };
    let group = |group: &GroupSpending| GroupSpent {
        key: group.key.as_str().to_owned(),
        label: group.label.clone(),
        spent_directly: amounts(call, &group.own),
        subcategories: group.children.iter().map(subcategory).collect(),
        total: amounts(call, &group.total),
    };
    let groups: Vec<GroupSpent> = spending.groups.iter().map(group).collect();
    let rows = groups
        .iter()
        .map(|group| 1 + group.subcategories.len() as u64)
        .sum();
    Ok(Reply::new(
        CategorySpending {
            period: period.key(),
            groups,
            uncategorized: amounts(call, &spending.uncategorized),
            total: amounts(call, &spending.total),
        },
        rows,
    ))
}

// budget_status ---------------------------------------------------------------

pub const BUDGET_STATUS: ToolSpec = ToolSpec {
    name: "budget_status",
    description: "Each budget for one month (`YYYY-MM`) against what was spent: the budget, \
                  spent, remaining (below zero when over), its group (fixed, flex, \
                  non_monthly) and whether it rolls over (reported only: rollover isn't \
                  applied yet). A month's own budget replaces the standing one. Spent is the \
                  user's own share in the budget's currency, a group's budget covering its \
                  subcategories; refunds net against their category in the month the money \
                  came back. Spending in other currencies is listed separately, never \
                  converted. Totals only.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

/// `budget_status`' arguments.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BudgetMonth {
    /// The month, `YYYY-MM`, e.g. `2026-11`.
    pub month: String,
}

/// `budget_status`' result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BudgetStatus {
    /// `YYYY-MM`.
    pub month: String,
    /// Every category with a budget for the month, in the user's category
    /// order.
    pub budgets: Vec<BudgetState>,
}

/// One category's budget against its spending.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BudgetState {
    /// The category's stable key, e.g. `food.groceries`.
    pub category: String,
    /// The user's label for it.
    pub label: String,
    pub group_kind: BudgetGroupKind,
    /// Whether the budget rolls over. Reported only: this month's figures
    /// don't include a carry-in yet.
    pub rollover: bool,
    /// Where the budget comes from.
    pub applies: BudgetSource,
    /// What may be spent in the month.
    pub budget: Amount,
    /// Spent in the budget's currency, positive; refunds reduce it.
    pub spent: Amount,
    /// Budget minus spent; below zero when over budget.
    pub remaining: Amount,
    /// Spent in the category in other currencies, positive, one amount per
    /// currency. Not counted against the budget.
    pub spent_other_currencies: Vec<Amount>,
}

/// Which kind of budget it is.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetGroupKind {
    /// The same every month (rent, subscriptions).
    Fixed,
    /// Day-to-day spending.
    Flex,
    /// Spent a few times a year, set aside monthly.
    NonMonthly,
}

/// Where a month's budget comes from.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSource {
    /// The budget set for this month only.
    Month,
    /// The standing budget, for every month without its own.
    Standing,
}

impl From<coffer_core::budget::BudgetGroup> for BudgetGroupKind {
    fn from(group: coffer_core::budget::BudgetGroup) -> Self {
        use coffer_core::budget::BudgetGroup;
        match group {
            BudgetGroup::Fixed => BudgetGroupKind::Fixed,
            BudgetGroup::Flex => BudgetGroupKind::Flex,
            BudgetGroup::NonMonthly => BudgetGroupKind::NonMonthly,
        }
    }
}

fn budget_status(call: &Call<'_>, args: BudgetMonth) -> Result<Reply<BudgetStatus>, ToolError> {
    let period = month(&args.month)?;
    let status = core_budget::budget_status(call.reader(), period).map_err(reporting_error)?;
    let state = |line: &BudgetLine| BudgetState {
        category: line.category.as_str().to_owned(),
        label: line.label.clone(),
        group_kind: line.group.into(),
        rollover: line.rollover,
        applies: if line.for_month {
            BudgetSource::Month
        } else {
            BudgetSource::Standing
        },
        budget: call.amount(line.budget),
        spent: call.amount(line.spent),
        remaining: call.amount(line.remaining),
        spent_other_currencies: amounts(call, &line.spent_other_currencies),
    };
    let budgets: Vec<BudgetState> = status.budgets.iter().map(state).collect();
    let rows = budgets.len() as u64;
    Ok(Reply::new(
        BudgetStatus {
            month: period.key(),
            budgets,
        },
        rows,
    ))
}

// net_worth_history -----------------------------------------------------------

pub const NET_WORTH_HISTORY: ToolSpec = ToolSpec {
    name: "net_worth_history",
    description: "Net worth at the end of every month from `from` through `to` (both \
                  `YYYY-MM`, at most 200 months): each account's balance on the month's last \
                  day and the total per currency, never converted. Balances are gross, like \
                  the bank's. Pots are accounts of their own; asset and liability accounts \
                  hold their last valuation, and a liability always lowers the total. Closed \
                  accounts are left out after closing. An account marked `unanchored` has no \
                  known balance, so its figure is its transactions summed from zero.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

/// `net_worth_history`' result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NetWorthHistory {
    /// One entry per month, oldest first.
    pub months: Vec<NetWorthAt>,
    /// The range was longer than 200 months and was cut after the 200th.
    pub truncated: bool,
}

/// Net worth at one month's end.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NetWorthAt {
    /// `YYYY-MM`.
    pub month: String,
    /// The day the balances are taken: the month's last, `YYYY-MM-DD`.
    pub date: String,
    /// Every account open that day with a known balance, in the order the
    /// user added them.
    pub accounts: Vec<AccountBalance>,
    /// The sum of every account's `counted`, one amount per currency.
    pub total: Vec<Amount>,
}

/// One account's balance at a month's end.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AccountBalance {
    /// The account's ID, as `list_accounts` gives it.
    pub account_id: i64,
    /// The user's name for the account.
    pub name: String,
    /// `current`, `savings`, `credit_card`, `pot`, `cash`, `asset` or
    /// `liability`.
    pub kind: String,
    /// The balance, gross, in the account's currency. A liability's is
    /// what is owed, below zero.
    pub balance: Amount,
    /// What the account adds to net worth: the balance, except a
    /// liability, which always subtracts what is owed.
    pub counted: Amount,
    /// The account has no known balance (opening balance, statement or
    /// valuation): its transactions are summed from zero.
    pub unanchored: bool,
}

fn net_worth_history(
    call: &Call<'_>,
    args: MonthRange,
) -> Result<Reply<NetWorthHistory>, ToolError> {
    let (from, to) = (month(&args.from)?, month(&args.to)?);
    let history = net_worth::net_worth_history(call.reader(), from, to).map_err(reporting_error)?;
    let account = |worth: &AccountWorth| AccountBalance {
        account_id: worth.account.get(),
        name: worth.name.clone(),
        kind: worth.kind.as_str().to_owned(),
        balance: call.amount(worth.balance),
        counted: call.amount(worth.counted),
        unanchored: worth.unanchored,
    };
    let months: Vec<NetWorthAt> = history
        .items
        .iter()
        .map(|month: &NetWorthMonth| NetWorthAt {
            month: month.period.key(),
            date: month.date.to_string(),
            accounts: month.accounts.iter().map(account).collect(),
            total: amounts(call, &month.total),
        })
        .collect();
    let rows = months.len() as u64;
    Ok(Reply::new(
        NetWorthHistory {
            months,
            truncated: history.truncated,
        },
        rows,
    ))
}
