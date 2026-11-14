//! How the register's rows read in the table, the inspector and the menus
//! (`docs/DESIGN.md` → "Data table (Transactions)"): category cells and
//! markers, transfer labels, day headings and nets, and the options of the
//! category, account and date menus. Pure, so it's tested without a window.

use chrono::{Datelike as _, NaiveDate};
use coffer_core::account::AccountKind;
use coffer_core::id::AccountId;
use coffer_core::money::Money;
use coffer_core::reporting::register::{
    Colour, DatePreset, Register, RegisterCategory, RegisterRow, RowCategory,
};
use gpui_kit::SharedString;

use crate::format::{self, Sign};
use crate::theme::tokens::color;
use crate::ui::marker::Marker;
use crate::ui::menu::{MenuGroup, MenuOption};
use crate::ui::transaction_row::CategoryCell;

/// The menu key of the "All …" option of a filter menu.
pub const ALL: &str = "all";

/// A category colour's marker.
pub fn marker(colour: Colour) -> Marker {
    match colour {
        Colour::Chart(slot) => color::CHART
            .get(usize::from(slot).saturating_sub(1))
            .copied()
            .map_or(Marker::Other, Marker::Colour),
        Colour::Other => Marker::Other,
    }
}

/// The marker of a split row: halves in the two largest lines' colours when
/// both have one of the six, otherwise the largest line's marker
/// (`DESIGN.md` → "Split row in the table").
fn split_marker(
    register: &Register,
    parts: &[coffer_core::reporting::register::SplitPart],
) -> Marker {
    let mut lines: Vec<(
        &coffer_core::reporting::register::SplitPart,
        &RegisterCategory,
    )> = parts
        .iter()
        .filter_map(|part| Some((part, register.category(part.category?)?)))
        .collect();
    lines.sort_by_key(|(part, _)| std::cmp::Reverse(part.amount.minor().unsigned_abs()));
    match lines.as_slice() {
        [] => Marker::None,
        [(_, only)] => marker(only.colour),
        [(_, first), (_, second), ..] => match (marker(first.colour), marker(second.colour)) {
            (Marker::Colour(left), Marker::Colour(right)) => Marker::Split(left, right),
            (largest, _) => largest,
        },
    }
}

/// What a transfer's category cell says: "Transfer · Rainy day pot".
pub fn transfer_label(register: &Register, partners: &[AccountId]) -> String {
    let names: Vec<String> = partners
        .iter()
        .filter_map(|id| register.account(*id))
        .map(|account| match account.kind {
            AccountKind::Pot => format!("{} pot", account.name),
            _ => account.name.clone(),
        })
        .collect();
    if names.is_empty() {
        "Transfer".to_owned()
    } else {
        format!("Transfer · {}", names.join(", "))
    }
}

/// A row's category as one label: its category, its lines' categories, the
/// transfer, or "Uncategorised".
pub fn category_label(register: &Register, row: &RegisterRow) -> String {
    if let Some(partners) = &row.transfer {
        return transfer_label(register, partners);
    }
    let labels: Vec<&str> = register
        .row_categories(row)
        .iter()
        .map(|category| category.label.as_str())
        .collect();
    if labels.is_empty() {
        "Uncategorised".to_owned()
    } else {
        labels.join(", ")
    }
}

/// The row's category cell.
pub fn cell(register: &Register, row: &RegisterRow) -> CategoryCell {
    if row.transfer.is_some() {
        return CategoryCell::transfer(category_label(register, row));
    }
    match &row.category {
        RowCategory::Uncategorised => CategoryCell::uncategorised(),
        RowCategory::Category(id) => match register.category(*id) {
            Some(category) => CategoryCell::marked(category.label.clone(), marker(category.colour)),
            None => CategoryCell::uncategorised(),
        },
        RowCategory::Split(parts) => {
            CategoryCell::marked(category_label(register, row), split_marker(register, parts))
        }
    }
}

/// The marker a row's category shows in a select: as in the table.
pub fn row_marker(register: &Register, row: &RegisterRow) -> Marker {
    match &row.category {
        RowCategory::Uncategorised => Marker::None,
        RowCategory::Category(id) => register
            .category(*id)
            .map_or(Marker::None, |category| marker(category.colour)),
        RowCategory::Split(parts) => split_marker(register, parts),
    }
}

/// Whether the row's category can be set in place: not a transfer, not a
/// split (its lines are set in the split editor, M3.9).
pub fn can_recategorise(row: &RegisterRow) -> bool {
    row.transfer.is_none() && !matches!(row.category, RowCategory::Split(_))
}

/// A day's header: "Mon 9 Nov", with a two-digit year outside `today`'s.
pub fn day_heading(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        date.format("%a %-d %b").to_string()
    } else {
        date.format("%a %-d %b %y").to_string()
    }
}

/// A day's net, one amount per currency: "−£37.74", or "−£5.00 · −€12.00".
pub fn net(amounts: &[Money]) -> String {
    amounts
        .iter()
        .map(|amount| format::money(*amount, Sign::Always))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The amounts of `rows`, summed per currency, for the inspector's total.
pub fn total(rows: &[&RegisterRow]) -> Vec<Money> {
    let mut totals: Vec<Money> = Vec::new();
    for row in rows {
        match totals
            .iter_mut()
            .find(|total| total.currency() == row.amount.currency())
        {
            Some(total) => {
                *total = total.checked_add(row.amount).unwrap_or(*total);
            }
            None => totals.push(row.amount),
        }
    }
    totals
}

/// The category menu: each group under its own caps label, the group's own
/// category first, then its subcategories. Keys are category IDs.
pub fn category_groups(register: &Register) -> Vec<MenuGroup> {
    let mut groups: Vec<MenuGroup> = Vec::new();
    for category in register.categories() {
        let option = MenuOption::new(category.id.to_string(), category.label.clone())
            .marker(marker(category.colour));
        match category.group {
            None => groups.push(MenuGroup::named(category.label.clone(), vec![option])),
            Some(_) => match groups.last_mut() {
                Some(group) => group.options.push(option),
                None => groups.push(MenuGroup::new(None, vec![option])),
            },
        }
    }
    groups
}

/// The Category filter's menu: "All categories", then [`category_groups`].
pub fn category_filter_groups(register: &Register) -> Vec<MenuGroup> {
    let mut groups = vec![MenuGroup::new(
        None,
        vec![MenuOption::new(ALL, "All categories")],
    )];
    groups.extend(category_groups(register));
    groups
}

/// The Account filter's menu: "All accounts", then `accounts` (each pot
/// after its parent, as the panel lists them). Keys are account IDs.
pub fn account_groups(accounts: &[(AccountId, String)]) -> Vec<MenuGroup> {
    let mut options = vec![MenuOption::new(ALL, "All accounts")];
    options.extend(
        accounts
            .iter()
            .map(|(id, name)| MenuOption::new(id.to_string(), name.clone())),
    );
    vec![MenuGroup::new(None, options)]
}

/// What a date preset is called, in its menu and on the chip.
pub fn date_label(preset: DatePreset) -> &'static str {
    match preset {
        DatePreset::ThisMonth => "This month",
        DatePreset::LastMonth => "Last month",
        DatePreset::LastThreeMonths => "Last 3 months",
        DatePreset::ThisYear => "This year",
        DatePreset::AllTime => "All time",
    }
}

/// A date preset's menu key.
pub fn date_key(preset: DatePreset) -> &'static str {
    match preset {
        DatePreset::ThisMonth => "this_month",
        DatePreset::LastMonth => "last_month",
        DatePreset::LastThreeMonths => "last_3_months",
        DatePreset::ThisYear => "this_year",
        DatePreset::AllTime => "all_time",
    }
}

/// The Date filter's menu.
pub fn date_groups() -> Vec<MenuGroup> {
    vec![MenuGroup::new(
        None,
        DatePreset::ALL
            .into_iter()
            .map(|preset| MenuOption::new(date_key(preset), date_label(preset)))
            .collect(),
    )]
}

/// The preset with menu key `key`.
pub fn date_preset(key: &str) -> Option<DatePreset> {
    DatePreset::ALL
        .into_iter()
        .find(|preset| date_key(*preset) == key)
}

/// The undo bar's message: "Categorised 3 transactions as Shopping".
pub fn categorised_message(count: usize, label: &str) -> SharedString {
    let what = if count == 1 {
        "1 transaction".to_owned()
    } else {
        format!("{count} transactions")
    };
    format!("Categorised {what} as {label}").into()
}

#[cfg(test)]
mod tests {
    use coffer_core::money::Currency;

    use super::*;
    use crate::transactions::fixture;

    #[test]
    fn transfers_are_named_by_the_other_side() {
        let register = fixture::register();
        let row = register.row(fixture::id("txn-rainy-day-09")).unwrap();
        assert_eq!(category_label(&register, row), "Transfer · Rainy day pot");
        assert!(cell(&register, row).is_transfer());
        assert!(!can_recategorise(row));
    }

    #[test]
    fn markers_follow_the_colour_slots() {
        assert_eq!(marker(Colour::Chart(1)), Marker::Colour(color::CHART_1));
        assert_eq!(marker(Colour::Chart(6)), Marker::Colour(color::CHART_6));
        assert_eq!(marker(Colour::Other), Marker::Other);
        assert_eq!(marker(Colour::Chart(7)), Marker::Other);
    }

    #[test]
    fn days_and_nets_read_as_drawn() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert_eq!(day_heading(day, today), "Mon 9 Nov");
        let last_year = NaiveDate::from_ymd_opt(2025, 10, 2).unwrap();
        assert_eq!(day_heading(last_year, today), "Thu 2 Oct 25");
        assert_eq!(net(&[Money::new(-3774, Currency::GBP)]), "−£37.74");
        assert_eq!(
            net(&[
                Money::new(-1200, Currency::EUR),
                Money::new(-500, Currency::GBP)
            ]),
            "−€12.00 · −£5.00"
        );
        assert_eq!(net(&[]), "");
    }

    #[test]
    fn the_category_menu_puts_each_group_first() {
        let register = fixture::register();
        let groups = category_groups(&register);
        assert_eq!(groups.len(), 7, "the fixture's categories are all groups");
        assert_eq!(
            groups[0].label.as_ref().map(|l| l.as_ref()),
            Some("Groceries")
        );
        assert_eq!(groups[0].options[0].key.as_ref(), "1");
        assert_eq!(
            groups[0].options[0].marker,
            Some(Marker::Colour(color::CHART_1))
        );
        let filter = category_filter_groups(&register);
        assert_eq!(filter[0].options[0].key.as_ref(), ALL);
    }

    #[test]
    fn date_presets_round_trip_their_keys() {
        for preset in DatePreset::ALL {
            assert_eq!(date_preset(date_key(preset)), Some(preset));
        }
        assert_eq!(date_groups()[0].options.len(), 5);
    }

    #[test]
    fn the_undo_message_counts() {
        assert_eq!(
            categorised_message(3, "Shopping").as_ref(),
            "Categorised 3 transactions as Shopping"
        );
        assert_eq!(
            categorised_message(1, "Bills").as_ref(),
            "Categorised 1 transaction as Bills"
        );
    }
}
