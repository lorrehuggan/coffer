//! Reference data: the category tree (`list_categories`), and later the
//! accounts, rules and saved reports (`list_accounts`, `list_rules`,
//! `list_reports`, Block C).
//!
//! `list_categories` is the pattern every tool follows: a [`ToolSpec`] with
//! its level and writes, arguments (here [`NoArgs`]), a result type made of
//! plain fields and [`crate::output`] types, and a handler that runs core
//! queries on [`Call::reader`] and says how many rows it returns.

use coffer_core::category::{Category, CategoryKind};
use schemars::JsonSchema;
use serde::Serialize;

use crate::registry::{AccessLevel, Call, NoArgs, Registry, Reply, ToolError, ToolSpec, Writes};

/// Adds this group's tools.
pub fn register(registry: &mut Registry) {
    registry.add(LIST_CATEGORIES, list_categories);
}

pub const LIST_CATEGORIES: ToolSpec = ToolSpec {
    name: "list_categories",
    description: "The user's categories as a two-level tree: groups (e.g. `food`) and their \
                  subcategories (e.g. `food.groceries`). Keys are stable; labels are what the \
                  user sees. Use the keys when you propose categories.",
    level: AccessLevel::Aggregate,
    writes: Writes::Nothing,
};

/// `list_categories`' result.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CategoryList {
    /// Every group, in the user's order, with its subcategories.
    pub groups: Vec<CategoryGroup>,
}

/// A top-level category and its subcategories.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CategoryGroup {
    /// Stable key, e.g. `food`.
    pub key: String,
    pub label: String,
    pub kind: Kind,
    pub children: Vec<Subcategory>,
}

/// A subcategory. It has its group's kind.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Subcategory {
    /// Stable key, `<group key>.<part>`, e.g. `food.groceries`.
    pub key: String,
    pub label: String,
}

/// What a category's amounts count as.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Money spent.
    Spending,
    /// Money earned or received.
    Income,
    /// Money moving between the user's own accounts: neither.
    Transfer,
}

impl From<CategoryKind> for Kind {
    fn from(kind: CategoryKind) -> Kind {
        match kind {
            CategoryKind::Spending => Kind::Spending,
            CategoryKind::Income => Kind::Income,
            CategoryKind::Transfer => Kind::Transfer,
        }
    }
}

fn list_categories(call: &Call<'_>, _args: NoArgs) -> Result<Reply<CategoryList>, ToolError> {
    let tree = call.reader().categories().map_err(ToolError::core)?;
    let subcategory = |category: &Category| Subcategory {
        key: category.key().as_str().to_owned(),
        label: category.label().to_owned(),
    };
    let groups: Vec<CategoryGroup> = tree
        .groups()
        .iter()
        .map(|group| {
            let category = group.category();
            CategoryGroup {
                key: category.key().as_str().to_owned(),
                label: category.label().to_owned(),
                kind: category.kind().into(),
                children: group.children().iter().map(subcategory).collect(),
            }
        })
        .collect();
    Ok(Reply::new(CategoryList { groups }, tree.len() as u64))
}
