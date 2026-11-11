//! The default category set, as `migrations/V2__default_categories.sql`
//! seeds it. A test keeps this list and the migration equal, so code that
//! can't open a database (bundled presets' tests, documentation) can check a
//! key against the set every new database starts with.

use super::CategoryKind::{self, Income, Spending, Transfer};

/// One of the categories every new database starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultCategory {
    /// Its stable key, e.g. `food.groceries`.
    pub key: &'static str,
    /// Its English (en-GB) label, until the user renames it.
    pub label: &'static str,
    /// What its amounts count as; a subcategory has its group's kind.
    pub kind: CategoryKind,
}

const fn category(key: &'static str, label: &'static str, kind: CategoryKind) -> DefaultCategory {
    DefaultCategory { key, label, kind }
}

/// The default categories in display order: each group, then its
/// subcategories. Keys never change once shipped; later defaults are added
/// at the end of their group, in a new migration.
pub const DEFAULT_CATEGORIES: &[DefaultCategory] = &[
    category("income", "Income", Income),
    category("income.salary", "Salary", Income),
    category("income.self_employment", "Self-employment", Income),
    category("income.benefits", "Benefits", Income),
    category("income.pension", "Pension", Income),
    category("income.interest", "Interest", Income),
    category("income.investments", "Investment income", Income),
    category("income.rental", "Rental income", Income),
    category("income.gifts", "Gifts received", Income),
    category("home", "Home", Spending),
    category("home.rent", "Rent", Spending),
    category("home.mortgage", "Mortgage", Spending),
    category("home.property_tax", "Council tax", Spending),
    category("home.maintenance", "Repairs and maintenance", Spending),
    category("home.household", "Household", Spending),
    category("bills", "Bills", Spending),
    category("bills.energy", "Energy", Spending),
    category("bills.water", "Water", Spending),
    category("bills.phone", "Phone", Spending),
    category("bills.internet", "Internet", Spending),
    category("bills.tv", "TV", Spending),
    category("bills.insurance", "Insurance", Spending),
    category("bills.subscriptions", "Subscriptions", Spending),
    category("food", "Food", Spending),
    category("food.groceries", "Groceries", Spending),
    category("food.eating_out", "Eating out", Spending),
    category("food.takeaway", "Takeaways", Spending),
    category("transport", "Transport", Spending),
    category("transport.public", "Public transport", Spending),
    category("transport.taxi", "Taxis", Spending),
    category("transport.fuel", "Fuel", Spending),
    category("transport.parking", "Parking and tolls", Spending),
    category("transport.vehicle", "Vehicle costs", Spending),
    category("shopping", "Shopping", Spending),
    category("shopping.clothing", "Clothing", Spending),
    category("shopping.electronics", "Electronics", Spending),
    category("entertainment", "Entertainment", Spending),
    category("entertainment.events", "Events and tickets", Spending),
    category("entertainment.hobbies", "Hobbies", Spending),
    category("health", "Health", Spending),
    category("health.medical", "Medical", Spending),
    category("health.fitness", "Fitness", Spending),
    category("health.personal_care", "Personal care", Spending),
    category("travel", "Travel", Spending),
    category("travel.flights", "Flights", Spending),
    category("travel.accommodation", "Accommodation", Spending),
    category("family", "Family", Spending),
    category("family.childcare", "Childcare", Spending),
    category("family.children", "Children", Spending),
    category("family.education", "Education", Spending),
    category("family.pets", "Pets", Spending),
    category("giving", "Giving", Spending),
    category("giving.gifts", "Gifts", Spending),
    category("giving.charity", "Charity", Spending),
    category("finance", "Finance", Spending),
    category("finance.fees", "Fees and charges", Spending),
    category("finance.interest", "Interest paid", Spending),
    category("finance.loans", "Loan repayments", Spending),
    category("finance.tax", "Tax", Spending),
    category("cash", "Cash", Spending),
    category("other", "Other", Spending),
    category("transfer", "Transfers", Transfer),
    category("transfer.savings", "Savings", Transfer),
    category("transfer.investments", "Investments", Transfer),
    category("transfer.credit_card", "Credit card payments", Transfer),
];

/// The default category with `key`, if there is one.
pub fn default_category(key: &str) -> Option<&'static DefaultCategory> {
    DEFAULT_CATEGORIES
        .iter()
        .find(|category| category.key == key)
}
