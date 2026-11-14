//! How the register's rows read in the table, the inspector and the menus
//! (`docs/DESIGN.md` → "Data table (Transactions)"): category cells and
//! markers, transfer labels, day headings and nets, and the options of the
//! category, account and date menus; the tag menu and the split editor's
//! footer (M3.9). Pure, so it's tested without a window.

use chrono::{Datelike as _, NaiveDate};
use coffer_core::account::AccountKind;
use coffer_core::id::{AccountId, TagId};
use coffer_core::money::Money;
use coffer_core::reporting::register::{
    Colour, DatePreset, Register, RegisterCategory, RegisterRow, RowCategory,
};
use coffer_core::split::draft::{DraftStatus, SplitDraft};
use coffer_core::tag::{TagName, TagNameError};
use gpui_kit::SharedString;

use crate::format::{self, Sign};
use crate::theme::tokens::color;
use crate::ui::icon::Glyph;
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

/// A row's category as one label: its category, every line's category
/// ("Uncategorised" for a line without one, as Paper `2Q3-0` draws it), the
/// transfer, or "Uncategorised".
pub fn category_label(register: &Register, row: &RegisterRow) -> String {
    if let Some(partners) = &row.transfer {
        return transfer_label(register, partners);
    }
    if let RowCategory::Split(parts) = &row.category {
        return parts
            .iter()
            .map(|part| {
                part.category
                    .and_then(|id| register.category(id))
                    .map_or("Uncategorised", |category| category.label.as_str())
            })
            .collect::<Vec<_>>()
            .join(", ");
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

/// The tag menu's "Create “…”" option: a public tag named as typed.
pub const NEW_TAG: &str = "new-tag";
/// The tag menu's "Create “…” as private" option.
pub const NEW_PRIVATE_TAG: &str = "new-private-tag";

/// A tag's key in the tag menu.
pub fn tag_key(id: TagId) -> String {
    format!("tag-{id}")
}

/// The tag a tag-menu key names, if it names one.
pub fn tag_from_key(key: &str) -> Option<TagId> {
    key.strip_prefix("tag-")?.parse().ok().map(TagId::new)
}

/// The tag menu (composed for M3.9, user, 2026-11-14): every tag A–Z,
/// ticked when it's in `on_row`, a private one with the lock in the icon
/// slot, narrowed by `query` as the category menu is. When `query` isn't a
/// tag's name (ignoring case, as tag names are compared), two last options
/// create one: "Create “…”" and "Create “…” as private".
pub fn tag_groups(register: &Register, on_row: &[TagId], query: &str) -> Vec<MenuGroup> {
    let options: Vec<MenuOption> = register
        .tags()
        .iter()
        .map(|tag| {
            let option = MenuOption::new(tag_key(tag.id), tag.name.clone())
                .checked(on_row.contains(&tag.id));
            if tag.private {
                option.icon(Glyph::Lock)
            } else {
                option
            }
        })
        .collect();
    let mut groups = crate::ui::menu::filter_groups(&[MenuGroup::new(None, options)], query);
    let typed = query.trim();
    if typed.is_empty() {
        return groups;
    }
    let creates = match TagName::new(typed) {
        Ok(name) => {
            let taken = register
                .tags()
                .iter()
                .any(|tag| TagName::new(&tag.name).is_ok_and(|existing| existing.same_name(&name)));
            if taken {
                return groups;
            }
            vec![
                MenuOption::new(NEW_TAG, format!("Create “{}”", name.as_str())).icon(Glyph::Plus),
                MenuOption::new(
                    NEW_PRIVATE_TAG,
                    format!("Create “{}” as private", name.as_str()),
                )
                .icon(Glyph::Lock),
            ]
        }
        Err(error) => vec![
            MenuOption::new(NEW_TAG, "Create a tag")
                .icon(Glyph::Plus)
                .hint(tag_name_problem(&error))
                .disabled(true),
        ],
    };
    groups.push(MenuGroup::new(None, creates));
    groups
}

/// Why a typed tag name can't be used, as the menu's hint says it.
fn tag_name_problem(error: &TagNameError) -> String {
    match error {
        TagNameError::Blank => "type a name".to_owned(),
        TagNameError::TooLong { max } => format!("at most {max} characters"),
        TagNameError::ControlCharacter => "no line breaks or tabs".to_owned(),
    }
}

/// How the split editor's footer looks (`DESIGN.md` → "Split states").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitTone {
    /// Saved: "Remaining" in `text-muted`, the amount in `text`, a `check`.
    Balanced,
    /// Not saved, nothing wrong yet (one line): as balanced, no icon.
    Pending,
    /// Not balanced: "Remaining" and the amount in `warning`, an `alert`.
    Unbalanced,
}

/// The split editor's footer and the caption under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitFooter {
    /// The signed remainder: "−£4.00", "+£3.60", "£0.00".
    pub remaining: String,
    pub tone: SplitTone,
    pub caption: Option<String>,
}

/// What the split editor's footer says about `draft` (the states board,
/// `4XF-0`; the missing, zero and share cases are composed for M3.9).
pub fn split_footer(draft: &SplitDraft) -> SplitFooter {
    const SAVED_WHEN: &str = "Saved when Remaining is £0.00.";
    let currency = draft.parent().currency();
    let zero = format::money(Money::new(0, currency), Sign::Always);
    let saved_when = SAVED_WHEN.replace("£0.00", &zero);
    let remaining = draft
        .remaining()
        .map_or_else(|| zero.clone(), |rest| format::money(rest, Sign::Always));
    let short = format!("{saved_when} A new line starts with the remaining amount.");
    let (tone, caption) = match draft.status() {
        DraftStatus::Balanced => (SplitTone::Balanced, None),
        DraftStatus::OneLine => (SplitTone::Pending, Some(short)),
        DraftStatus::Unbalanced {
            remaining: rest,
            over: true,
        } => {
            let over = rest.checked_abs().unwrap_or(rest);
            (
                SplitTone::Unbalanced,
                Some(format!(
                    "The lines are {} more than the transaction. {saved_when}",
                    format::money(over, Sign::Negative)
                )),
            )
        }
        DraftStatus::Unbalanced { over: false, .. } => (SplitTone::Unbalanced, Some(short)),
        DraftStatus::MissingAmount { line } => (
            SplitTone::Unbalanced,
            Some(format!("Line {line} has no amount. {saved_when}")),
        ),
        DraftStatus::ZeroLine { line } => (
            SplitTone::Unbalanced,
            Some(format!(
                "Line {line} is {zero}: give it an amount or remove it."
            )),
        ),
        DraftStatus::ShareOutsideLine { line } => (
            SplitTone::Unbalanced,
            Some(format!("Line {line} is less than the share you set on it.")),
        ),
        DraftStatus::Overflow => (
            SplitTone::Unbalanced,
            Some("The lines add up to more than Coffer can hold.".to_owned()),
        ),
    };
    SplitFooter {
        remaining,
        tone,
        caption,
    }
}

#[cfg(test)]
mod tests {
    use coffer_core::category::CategoryKey;
    use coffer_core::money::Currency;
    use coffer_core::split::SplitLine;

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
    fn a_split_row_names_every_line() {
        let register = fixture::split_register();
        let row = register.row(fixture::id("txn-waitrose-07")).unwrap();
        assert_eq!(
            category_label(&register, row),
            "Groceries, Shopping, Uncategorised"
        );
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

    #[test]
    fn the_tag_menu_ticks_the_rows_tags_and_offers_to_create() {
        let register = fixture::split_register();
        let on_row = [fixture::WEEKLY_SHOP_TAG];
        let groups = tag_groups(&register, &on_row, "");
        assert_eq!(groups.len(), 1);
        let labels: Vec<_> = groups[0]
            .options
            .iter()
            .map(|option| (option.label.to_string(), option.checked, option.icon))
            .collect();
        assert_eq!(
            labels,
            [
                ("Mum's birthday".to_owned(), false, Some(Glyph::Lock)),
                ("Weekly shop".to_owned(), true, None),
            ]
        );
        assert_eq!(
            tag_from_key(&groups[0].options[1].key),
            Some(fixture::WEEKLY_SHOP_TAG)
        );

        // A new name: the matches, then the two ways to create it.
        let groups = tag_groups(&register, &on_row, " shop");
        assert_eq!(groups[0].options.len(), 1);
        let creates = &groups[1].options;
        assert_eq!(creates[0].key.as_ref(), NEW_TAG);
        assert_eq!(creates[0].label.as_ref(), "Create “shop”");
        assert_eq!(creates[1].key.as_ref(), NEW_PRIVATE_TAG);
        assert_eq!(creates[1].label.as_ref(), "Create “shop” as private");

        // An existing name in another case isn't offered again.
        let groups = tag_groups(&register, &on_row, "WEEKLY SHOP");
        assert_eq!(groups.len(), 1);
        // A name a tag can't have says why.
        let long = "x".repeat(65);
        let groups = tag_groups(&register, &on_row, &long);
        let option = &groups.last().unwrap().options[0];
        assert!(option.disabled);
        assert_eq!(option.hint.as_deref(), Some("at most 64 characters"));
        assert_eq!(tag_from_key(NEW_TAG), None);
    }

    #[test]
    fn the_split_footer_reads_as_the_states_board() {
        let gbp = |minor| Money::new(minor, Currency::GBP);
        let key = |key: &str| Some(CategoryKey::new(key).unwrap());
        let mut draft = SplitDraft::from_lines(
            gbp(-5216),
            &[
                SplitLine::new(gbp(-3866), key("groceries")),
                SplitLine::new(gbp(-950), key("shopping")),
            ],
        );
        let footer = split_footer(&draft);
        assert_eq!(footer.remaining, "−£4.00");
        assert_eq!(footer.tone, SplitTone::Unbalanced);
        assert_eq!(
            footer.caption.as_deref(),
            Some("Saved when Remaining is £0.00. A new line starts with the remaining amount.")
        );

        draft.set_amount(1, Some(gbp(-1710)));
        let footer = split_footer(&draft);
        assert_eq!(footer.remaining, "+£3.60");
        assert_eq!(
            footer.caption.as_deref(),
            Some("The lines are £3.60 more than the transaction. Saved when Remaining is £0.00.")
        );

        draft.set_amount(1, Some(gbp(-950)));
        draft.add_line();
        let footer = split_footer(&draft);
        assert_eq!(
            (footer.remaining.as_str(), footer.tone, footer.caption),
            ("£0.00", SplitTone::Balanced, None)
        );

        let start = SplitDraft::start(gbp(-5216), None);
        let footer = split_footer(&start);
        assert_eq!(footer.tone, SplitTone::Pending);
        assert_eq!(footer.remaining, "£0.00");

        draft.set_amount(2, None);
        assert_eq!(
            split_footer(&draft).caption.as_deref(),
            Some("Line 3 has no amount. Saved when Remaining is £0.00.")
        );
        draft.set_amount(2, Some(gbp(0)));
        assert_eq!(
            split_footer(&draft).caption.as_deref(),
            Some("Line 3 is £0.00: give it an amount or remove it.")
        );
    }
}
