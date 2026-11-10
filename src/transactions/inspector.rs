//! The inspector (`docs/DESIGN.md` → "Transactions"; Paper `1A0-0` for one
//! transaction, `1O7-0` for a multi-selection): 14px sections divided by
//! hairlines.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::agent_suggestion::AgentSuggestion;
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::field::{Field, FieldRow, SelectField, text_input};
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::TransactionsView;
use super::fixture::{Categorisation, DAYS, Sign, Transaction, gbp};

/// A section: 14px padding; below the first, a `hairline` above it.
fn section(divided: bool) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .p(spacing::S3_5)
        .when(divided, |this| {
            this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
        })
}

/// A section's title: section-title type in `text`.
fn section_title(title: &'static str) -> gpui_kit::Div {
    div()
        .text_role(typography::SECTION_TITLE)
        .text_color(color::TEXT)
        .child(title)
}

/// The header: the title (13/16 600), a display figure and a caption.
///
/// The title and figure are [`Tracked`] for their roles' tracking, so a
/// title too long for the inspector is clipped rather than wrapped (D5
/// finding; `docs/DESIGN.md` → "Letter spacing").
fn header(
    title: impl Into<gpui_kit::SharedString>,
    figure: String,
    caption: Option<AnyElement>,
) -> gpui_kit::Div {
    section(false)
        .gap(spacing::S1)
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title)),
        )
        .child(
            div()
                .flex()
                .text_color(color::TEXT)
                .child(Tracked::new(typography::DISPLAY_FIGURE, figure)),
        )
        .children(caption)
}

fn caption(text: impl IntoElement) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(text)
}

/// The category select for one transaction.
fn category_select(transaction: &Transaction) -> SelectField {
    let select = |value: String| SelectField::new("inspector-category", value);
    match transaction.categorisation {
        Categorisation::Assigned(category) => {
            select(category.name().into()).marker(category.marker())
        }
        Categorisation::Suggested { category, .. } => select(category.name().into()).suggested(),
        Categorisation::Uncategorised => select("Choose a category".into())
            .marker(Marker::None)
            .placeholder(true),
        Categorisation::Transfer { pot } => select(format!("Transfer · {pot}")),
    }
}

fn single(
    view: &TransactionsView,
    transaction: &Transaction,
    cx: &mut Context<TransactionsView>,
) -> Vec<AnyElement> {
    let id = transaction.id;
    let mut sections = vec![
        header(
            transaction.merchant,
            gbp(transaction.amount, Sign::Always),
            Some(
                caption(format!("{} · ", transaction.method))
                    .child(
                        div()
                            .text_role(typography::MONO_VALUE)
                            .child(transaction.time),
                    )
                    .into_any_element(),
            ),
        )
        .into_any_element(),
        section(true)
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Date",
                Field::new(DAYS[transaction.day].date).mono(true),
            ))
            .child(FieldRow::new(
                "Account",
                SelectField::new("inspector-account", transaction.account),
            ))
            .child(FieldRow::new("Category", category_select(transaction)))
            .child(FieldRow::new("Note", text_input(&view.note)))
            .into_any_element(),
    ];

    if let Categorisation::Suggested { category, reason } = transaction.categorisation {
        sections.push(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .px(spacing::S3_5)
                .pb(spacing::S3_5)
                .child(
                    AgentSuggestion::new(
                        "Claude",
                        "Claude Code · 2h ago",
                        "Categorise as",
                        category.name(),
                    )
                    .reason(reason)
                    .on_approve(
                        cx.listener(move |this, _, _, cx| this.resolve_suggestion(id, true, cx)),
                    )
                    .on_reject(
                        cx.listener(move |this, _, _, cx| this.resolve_suggestion(id, false, cx)),
                    ),
                )
                .into_any_element(),
        );
    }

    sections.push(
        section(true)
            .gap(spacing::S2)
            .child(section_title("Raw description"))
            .child(
                div()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_SECONDARY)
                    .child(transaction.raw_description),
            )
            .into_any_element(),
    );
    sections.push(
        section(true)
            .gap(spacing::S2)
            .child(section_title("Rule"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(spacing::S2)
                    .child(
                        div()
                            .text_role(typography::BODY)
                            .text_color(color::TEXT_MUTED)
                            .child("No rule matched"),
                    )
                    .child(
                        Button::new("create-rule", "Create rule from this")
                            .with_variant(ButtonVariant::Outline),
                    ),
            )
            .into_any_element(),
    );
    sections
}

/// What every transaction in `selected` shares, for the summary caption:
/// "Uncategorised · Current account".
fn shared(selected: &[&Transaction]) -> Vec<String> {
    let mut parts = Vec::new();
    let all = |same: &dyn Fn(&Transaction, &Transaction) -> bool| {
        selected.windows(2).all(|pair| same(pair[0], pair[1]))
    };
    if all(&|a, b| a.categorisation == b.categorisation) {
        parts.push(match selected[0].categorisation {
            Categorisation::Assigned(category) => category.name().to_owned(),
            Categorisation::Uncategorised => "Uncategorised".to_owned(),
            Categorisation::Suggested { category, .. } => format!("Suggested {}", category.name()),
            Categorisation::Transfer { .. } => "Transfers".to_owned(),
        });
    }
    if all(&|a, b| a.account == b.account) {
        parts.push(selected[0].account.to_owned());
    }
    parts
}

fn multiple(selected: &[&Transaction]) -> Vec<AnyElement> {
    let total: i64 = selected.iter().map(|transaction| transaction.amount).sum();
    let shared = shared(selected);
    let rows = selected.iter().map(|transaction| {
        div()
            .flex()
            .items_center()
            .gap(spacing::S3)
            .h(size::CONTROL)
            .child(
                div()
                    .flex_none()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_MUTED)
                    .child(DAYS[transaction.day].short),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_SECONDARY)
                    .child(transaction.merchant),
            )
            .child(
                div()
                    .flex_none()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(color::TEXT)
                    .child(gbp(transaction.amount, Sign::Always)),
            )
    });
    vec![
        header(
            format!("{} transactions", selected.len()),
            gbp(total, Sign::Always),
            (!shared.is_empty()).then(|| caption(shared.join(" · ")).into_any_element()),
        )
        .into_any_element(),
        section(true)
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Category",
                SelectField::new("inspector-category", "Choose a category")
                    .marker(Marker::None)
                    .placeholder(true),
            ))
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
        [] => empty(),
        [transaction] => single(view, transaction, cx),
        many => multiple(many),
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
    use crate::transactions::fixture::TRANSACTIONS;

    fn by_id(id: &str) -> &'static Transaction {
        TRANSACTIONS
            .iter()
            .find(|transaction| transaction.id == id)
            .expect("fixture transaction")
    }

    #[test]
    fn the_summary_names_what_the_selection_shares() {
        // The multi-select artboard's three rows.
        let selected = [
            by_id("txn-bluebottle-09"),
            by_id("txn-steam-07"),
            by_id("txn-vinted-06"),
        ];
        assert_eq!(shared(&selected), ["Uncategorised", "Current account"]);
        let total: i64 = selected.iter().map(|transaction| transaction.amount).sum();
        assert_eq!(gbp(total, Sign::Always), "−£29.94");
    }

    #[test]
    fn the_summary_leaves_out_what_differs() {
        let selected = [by_id("txn-pret-09"), by_id("txn-amazon-09")];
        assert!(shared(&selected).is_empty());
    }
}
