//! The inspector (`docs/DESIGN.md` → "Transactions"; Paper `1A0-0` for one
//! transaction, `1O7-0` for a multi-selection): 14px sections divided by
//! hairlines.
//!
//! One transaction: its merchant, amount and how it was paid; Details (D6.3,
//! Paper `2Q3-0`, `3WT-0`): Date and Account read-only with a lock (imported
//! source fields never change), the Merchant name, the Category select
//! (which sets the category through core like the table's in-place select;
//! a split row has none, its lines do), the Tags, the bank's note read-only
//! above the user's Note; the Split section (M3.9: the split editor, or a
//! Split button); the raw description; and whether a rule set the category.
//! A transfer shows its category read-only (M3.8) and has no Split section.
//! Several transactions: a count, the total, what they share, one Category
//! select that sets all of them, and the list.

use coffer_core::reporting::register::{RegisterRow, RowCategory};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::format::{self, Sign};
use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::{color, font, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::field::{
    self as fields, FieldRow, LockedValue, SelectField, error_caption, text_input,
};
use crate::ui::icon::Glyph;
use crate::ui::inspector::{caption, header, section, section_title, section_title_row};
use crate::ui::marker::Marker;
use crate::ui::menu::{self, Select, filter_groups};
use crate::ui::tag::{AddTagPill, TagPill};
use crate::ui::text::TextRoleExt as _;

use super::editor::{EditPlace, LineFields};
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

/// The error of the last inspector edit at `place`.
fn editor_error(view: &TransactionsView, place: EditPlace) -> Option<AnyElement> {
    view.editor
        .error_at(place)
        .map(|error| error_caption(error).into_any_element())
}

/// The Tags row (Paper `2Q3-0`): the label in a 28px lane at the top, the
/// pills wrapping in a box at field width, 4px apart, 2px block padding.
fn tags_row(
    view: &TransactionsView,
    row: &RegisterRow,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let pills: Vec<AnyElement> = row
        .tags
        .iter()
        .filter_map(|id| {
            let tag = view.register.tag(*id)?;
            let tag_id = *id;
            Some(
                TagPill::new(SharedString::from(format!("tag-{id}")), tag.name.clone())
                    .private(tag.private)
                    .on_remove(cx.listener(move |this, _, _, cx| this.remove_tag(tag_id, cx)))
                    .into_any_element(),
            )
        })
        .collect();
    let open = view.menu == Some(Menu::Tags);
    let add = AddTagPill::new("add-tag", &view.editor.tag_focus)
        .open(open)
        .on_click(cx.listener(TransactionsView::keyboard_toggle(Menu::Tags)));
    let add = div()
        .relative()
        .flex_none()
        .child(view.menu_trigger(Menu::Tags, add, cx))
        .when(open, |this| {
            this.child(menu::below(
                size::CONTROL_SM + spacing::S1,
                view.menu_list(Menu::Tags, cx),
            ))
        });
    div()
        .flex()
        .flex_none()
        .items_start()
        .gap(spacing::S2)
        .w_full()
        .min_h(size::CONTROL)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(size::CONTROL)
                .w(size::FIELD_LABEL)
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child("Tags"),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .flex_wrap()
                .items_center()
                .gap(spacing::S1)
                .py(spacing::S0_5)
                .children(pills)
                .child(add),
        )
        .into_any_element()
}

/// One split line: the category select (fills), the 96px amount field and
/// the 24px remove button, 6px apart.
fn split_line(
    view: &TransactionsView,
    line: &LineFields,
    index: usize,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let key = line.key;
    let menu = Menu::SplitLine(key);
    let category = view
        .line_category(key)
        .and_then(|id| view.register.category(id));
    let field = match category {
        Some(category) => SelectField::new(
            SharedString::from(format!("split-category-field-{key}")),
            category.label.clone(),
        )
        .marker(present::marker(category.colour)),
        None => SelectField::new(
            SharedString::from(format!("split-category-field-{key}")),
            "Category",
        )
        .marker(Marker::None)
        .placeholder(true),
    };
    let groups = if view.menu == Some(menu) {
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
    let select = Select::new(
        SharedString::from(format!("split-category-{key}")),
        format!("Line {} category", index + 1),
        field,
        groups,
        view.chosen_key(menu),
        &line.select,
    )
    .filter(&view.menu_filter)
    .max_height(size::CONTROL * super::MENU_ROWS)
    .on_toggle(move |open, window, cx| {
        toggled
            .update(cx, |this, cx| {
                if open != (this.menu == Some(menu)) {
                    if open {
                        this.open(menu, window, cx);
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
    });
    div()
        .id(SharedString::from(format!("split-line-{key}")))
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .h(size::CONTROL)
        .child(div().flex().flex_1().min_w_0().child(select))
        .child(
            text_input(&line.amount)
                .flex_none()
                .w(size::COLUMN_AMOUNT)
                .font_family(font::MONO)
                .text_right(),
        )
        .child(
            IconButton::new(
                SharedString::from(format!("split-remove-{key}")),
                Glyph::Reject,
                format!("Remove line {}", index + 1),
            )
            .with_size(IconButtonSize::Small)
            .on_click(cx.listener(move |this, _, _, cx| this.remove_split_line(key, cx))),
        )
        .into_any_element()
}

/// The split editor's footer: "+ Add line" on the left; "Remaining", the
/// signed remainder and the status icon in a 24px lane on the right.
fn split_footer(
    view: &TransactionsView,
    footer: &present::SplitFooter,
    window: &Window,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let focus = &view.editor.add_line_focus;
    let (label, value, icon) = match footer.tone {
        present::SplitTone::Balanced => (
            color::TEXT_MUTED,
            color::TEXT,
            Some(Glyph::Check.render().text_color(color::TEXT_MUTED)),
        ),
        present::SplitTone::Pending => (color::TEXT_MUTED, color::TEXT, None),
        present::SplitTone::Unbalanced => (
            color::WARNING,
            color::WARNING,
            Some(Glyph::Alert.render().text_color(color::WARNING)),
        ),
    };
    // Composed for M3.9 (the ghost button has no component yet): 24px,
    // radius 6, 8px padding pulled 8px left so the icon lines up with the
    // fields, a 12px plus and the caption 500 label in `text-secondary`.
    let add_line = div()
        .id("split-add-line")
        .test_support()
        .track_focus(focus)
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .h(size::CONTROL_SM)
        .px(spacing::S2)
        .ml(-spacing::S2)
        .rounded(radius::MD)
        .text_color(color::TEXT_SECONDARY)
        .hover(|style| style.bg(color::HOVER))
        .active(|style| style.bg(color::CONTROL_ACTIVE))
        .on_click(cx.listener(|this, _, window, cx| this.add_split_line(window, cx)))
        .child(Glyph::Plus.render().text_color(color::TEXT_SECONDARY))
        .child(
            div()
                .whitespace_nowrap()
                .text_role(typography::CAPTION)
                .font_weight(weight::MEDIUM)
                .child("Add line"),
        )
        .focus_visible_ring(focus, FocusRingPlacement::Outside, window);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .h(size::CONTROL)
        .child(add_line)
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .text_role(typography::BODY)
                .text_color(label)
                .child("Remaining"),
        )
        .child(
            div()
                .flex_none()
                .text_role(typography::MONO_AMOUNT)
                .text_color(value)
                .child(footer.remaining.clone()),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(size::CONTROL_SM)
                .children(icon),
        )
        .into_any_element()
}

/// The Split section: the editor while the row is split or being split,
/// otherwise the Split button and what it does.
fn split_section(
    view: &TransactionsView,
    window: &Window,
    cx: &mut Context<TransactionsView>,
) -> AnyElement {
    let section = section(true).gap(spacing::S2);
    let Some(split) = &view.editor.split else {
        return section
            .child(section_title_row(
                "Split",
                Some(
                    Button::new("split", "Split")
                        .with_variant(ButtonVariant::Outline)
                        .on_click(cx.listener(|this, _, window, cx| this.start_split(window, cx)))
                        .into_any_element(),
                ),
            ))
            .child(fields::caption("Divide this amount across categories."))
            .children(editor_error(view, EditPlace::Split))
            .into_any_element();
    };
    let footer = present::split_footer(&split.draft);
    let lines: Vec<AnyElement> = split
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| split_line(view, line, index, cx))
        .collect();
    section
        .child(section_title_row(
            "Split",
            Some(
                Button::new("remove-split", "Remove split")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(cx.listener(|this, _, _, cx| this.remove_split(cx)))
                    .into_any_element(),
            ),
        ))
        .children(lines)
        .child(split_footer(view, &footer, window, cx))
        .children(footer.caption.map(fields::caption))
        .children(editor_error(view, EditPlace::Split))
        .into_any_element()
}

fn single(
    view: &TransactionsView,
    row: &RegisterRow,
    window: &Window,
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
    let split = matches!(row.category, RowCategory::Split(_));
    // A split row's categories are its lines', in the split editor.
    let category: Option<AnyElement> = if present::can_recategorise(row) {
        let value = register
            .row_categories(row)
            .first()
            .map(|category| (category.label.clone(), present::marker(category.colour)));
        Some(category_select(view, value, cx))
    } else if split {
        None
    } else {
        Some(LockedValue::new(present::category_label(register, row)).into_any_element())
    };
    let bank_note = row.bank_note.clone().filter(|note| !note.trim().is_empty());

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
            .child(FieldRow::new(
                "Merchant",
                text_input(&view.editor.merchant).w_full(),
            ))
            .when_some(category, |this, category| {
                this.child(FieldRow::new("Category", category))
                    .children(edit_error(view))
            })
            .child(tags_row(view, row, cx))
            .when_some(bank_note, |this, note| {
                this.child(FieldRow::new("Bank note", LockedValue::new(note)))
            })
            .child(FieldRow::new(
                "Note",
                text_input(&view.editor.note).w_full(),
            ))
            .children(editor_error(view, EditPlace::Details))
            .into_any_element(),
    ];
    // Every row that can be split has the section (`DESIGN.md`: "Every
    // unsplit transaction"); a transfer can't be.
    if split || row.splittable {
        sections.push(split_section(view, window, cx));
    }
    sections.push(
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
    );
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
    window: &mut Window,
    cx: &mut Context<TransactionsView>,
) -> impl IntoElement {
    let selected = view.selected();
    let sections = match selected.as_slice() {
        [] => match view.account_inspector(cx) {
            Some(account) => vec![account.into_any_element()],
            None => empty(),
        },
        [row] => single(view, row, window, cx),
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
