//! The inspector (`docs/DESIGN.md` → "Transactions"; Paper `1A0-0` for one
//! transaction, `1O7-0` for a multi-selection): 14px sections divided by
//! hairlines.
//!
//! One transaction: its merchant, amount and how it was paid; Date and
//! Account read-only with a lock (D6.3's Details: imported source fields
//! never change); the Category select, which sets the category through core
//! like the table's in-place select; the raw description; and whether a
//! rule set the category. A transfer or a split row shows its category
//! read-only: transfers are confirmed in M3.8 and split lines are set in the
//! split editor (M3.9). Several transactions: a count, the total, what they
//! share, one Category select that sets all of them, and the list.

use coffer_core::reporting::register::RegisterRow;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::format::{self, Sign};
use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::field::{FieldRow, LockedValue, SelectField, error_caption};
use crate::ui::inspector::{caption, header, section, section_title};
use crate::ui::marker::Marker;
use crate::ui::menu::{Select, filter_groups};
use crate::ui::text::TextRoleExt as _;

use super::{Menu, TransactionsView, present};

/// The Category select for the selection: `value` with `marker`, or a
/// placeholder.
fn category_select(
    view: &TransactionsView,
    value: Option<(String, Marker)>,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let field = match value {
        Some((label, marker)) => SelectField::new("inspector-category-field", label).marker(marker),
        None => SelectField::new("inspector-category-field", "Choose a category")
            .marker(Marker::None)
            .placeholder(true),
    };
    let groups = if view.menu == Some(Menu::Inspector) {
        filter_groups(
            &present::category_groups(&view.register),
            &view.menu_filter.read(cx).value(),
        )
    } else {
        present::category_groups(&view.register)
    };
    let (toggled, chosen, moved) = (
        cx.entity().downgrade(),
        cx.entity().downgrade(),
        cx.entity().downgrade(),
    );
    Select::new(
        "inspector-category",
        "Category",
        field,
        groups,
        view.chosen_key(Menu::Inspector),
        &view.inspector_select,
    )
    .filter(&view.menu_filter)
    .max_height(size::CONTROL * super::MENU_ROWS)
    .on_toggle(move |open, window, cx| {
        toggled
            .update(cx, |this, cx| {
                if open != (this.menu == Some(Menu::Inspector)) {
                    if open {
                        this.open(Menu::Inspector, window, cx);
                    } else {
                        this.close(window, cx);
                    }
                }
            })
            .ok();
    })
    .on_choose(move |key, window, cx| {
        chosen
            .update(cx, |this, cx| this.choose(key, window, cx))
            .ok();
    })
    .on_move(move |step, _, cx| {
        moved
            .update(cx, |this, cx| this.move_highlight(step, cx))
            .ok();
    })
    .into_any_element()
}

/// The error under the Category select, when the last change failed.
fn edit_error(view: &TransactionsView) -> Option<AnyElement> {
    view.edit_error
        .clone()
        .map(|error| error_caption(error).into_any_element())
}

fn single(
    view: &TransactionsView,
    row: &RegisterRow,
    cx: &mut Context<TransactionsView>,
) -> Vec<AnyElement> {
    let register = &view.register;
    let method = row
        .payment_type
        .clone()
        .filter(|text| !text.trim().is_empty());
    let time = row.time.map(|time| time.format("%H:%M").to_string());
    let caption_line = match (method, time) {
        (None, None) => None,
        (method, time) => Some(
            caption(
                div()
                    .flex()
                    .items_center()
                    .when_some(method.clone(), |this, method| this.child(method))
                    .when(method.is_some() && time.is_some(), |this| this.child(" · "))
                    .when_some(time, |this, time| {
                        this.child(div().text_role(typography::MONO_VALUE).child(time))
                    }),
            )
            .into_any_element(),
        ),
    };
    let account = register
        .account(row.account)
        .map_or(String::new(), |account| account.name.clone());
    let category: AnyElement = if present::can_recategorise(row) {
        let value = register
            .row_categories(row)
            .first()
            .map(|category| (category.label.clone(), present::marker(category.colour)));
        category_select(view, value, cx)
    } else {
        LockedValue::new(present::category_label(register, row)).into_any_element()
    };

    let mut sections = vec![
        header(
            row.merchant.clone(),
            Some(format::money(row.amount, Sign::Always)),
            caption_line,
        )
        .into_any_element(),
        section(true)
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Date",
                LockedValue::new(format::field_date(row.date)).mono(true),
            ))
            .child(FieldRow::new("Account", LockedValue::new(account)))
            .child(FieldRow::new("Category", category))
            .children(edit_error(view))
            .into_any_element(),
        section(true)
            .gap(spacing::S2)
            .child(section_title("Raw description"))
            .child(
                div()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_SECONDARY)
                    .child(row.description.clone()),
            )
            .into_any_element(),
    ];
    if row.transfer.is_none() {
        sections.push(
            section(true)
                .gap(spacing::S2)
                .child(section_title("Rule"))
                .child(
                    div()
                        .text_role(typography::BODY)
                        .text_color(color::TEXT_MUTED)
                        .child(if row.rule.is_some() {
                            "A rule set this category"
                        } else {
                            "No rule matched"
                        }),
                )
                .into_any_element(),
        );
    }
    sections
}

/// What every row in `selected` shares, for the summary caption:
/// "Uncategorised · Current account".
fn shared(view: &TransactionsView, selected: &[&RegisterRow]) -> Vec<String> {
    let mut parts = Vec::new();
    let all = |same: &dyn Fn(&RegisterRow, &RegisterRow) -> bool| {
        selected.windows(2).all(|pair| same(pair[0], pair[1]))
    };
    if all(&|a, b| a.category == b.category && a.transfer.is_some() == b.transfer.is_some()) {
        let first = selected[0];
        parts.push(if first.transfer.is_some() {
            "Transfers".to_owned()
        } else {
            present::category_label(&view.register, first)
        });
    }
    if all(&|a, b| a.account == b.account)
        && let Some(account) = view.register.account(selected[0].account)
    {
        parts.push(account.name.clone());
    }
    parts
}

fn multiple(
    view: &TransactionsView,
    selected: &[&RegisterRow],
    cx: &mut Context<TransactionsView>,
) -> Vec<AnyElement> {
    let totals = present::total(selected);
    let figure = (totals.len() == 1).then(|| format::money(totals[0], Sign::Always));
    let shared = shared(view, selected);
    let today = view.today;
    let rows = selected.iter().map(|row| {
        div()
            .id(SharedString::from(format!("selected-{}", row.id)))
            .flex()
            .items_center()
            .gap(spacing::S3)
            .h(size::CONTROL)
            .child(
                div()
                    .flex_none()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_MUTED)
                    .child(format::list_date(row.date, today)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_SECONDARY)
                    .child(row.merchant.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT)
                    .child(format::money(row.amount, Sign::Always)),
            )
    });
    let value = view
        .chosen_key(Menu::Inspector)
        .and_then(|key| key.parse::<i64>().ok())
        .and_then(|id| view.register.category(id))
        .map(|category| (category.label.clone(), present::marker(category.colour)));
    let editable = selected.iter().any(|row| present::can_recategorise(row));
    vec![
        header(
            format!("{} transactions", selected.len()),
            figure,
            (!shared.is_empty()).then(|| caption(shared.join(" · ")).into_any_element()),
        )
        .into_any_element(),
        section(true)
            .gap(spacing::S2)
            .when(editable, |this| {
                this.child(FieldRow::new("Category", category_select(view, value, cx)))
                    .children(edit_error(view))
            })
            .into_any_element(),
        section(true)
            .gap(spacing::S2)
            .child(section_title("Selected"))
            .child(div().flex().flex_col().children(rows))
            .into_any_element(),
    ]
}

fn empty() -> Vec<AnyElement> {
    vec![
        section(false)
            .child(
                div()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_MUTED)
                    .child("No transaction selected"),
            )
            .into_any_element(),
    ]
}

pub(super) fn render(
    view: &TransactionsView,
    _: &mut Window,
    cx: &mut Context<TransactionsView>,
) -> impl IntoElement {
    let selected = view.selected();
    let sections = match selected.as_slice() {
        [] => match view.account_inspector(cx) {
            Some(account) => vec![account.into_any_element()],
            None => empty(),
        },
        [row] => single(view, row, cx),
        many => multiple(view, many, cx),
    };
    div()
        .id("inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::fixture;

    #[gpui_kit::test]
    fn the_summary_names_what_the_selection_shares(cx: &mut gpui_kit::TestAppContext) {
        let view = super::super::tests::fixture_view(cx);
        cx.update(|cx| {
            let view = view.read(cx);
            let rows: Vec<_> = ["txn-bluebottle-09", "txn-steam-07", "txn-vinted-06"]
                .into_iter()
                .map(|name| view.register.row(fixture::id(name)).unwrap())
                .collect();
            assert_eq!(shared(view, &rows), ["Uncategorised", "Current account"]);
            let totals = present::total(&rows);
            assert_eq!(format::money(totals[0], Sign::Always), "−£29.94");

            let rows: Vec<_> = ["txn-pret-09", "txn-amazon-09"]
                .into_iter()
                .map(|name| view.register.row(fixture::id(name)).unwrap())
                .collect();
            assert!(shared(view, &rows).is_empty());
        });
    }
}
