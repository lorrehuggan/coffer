//! The left panel (`docs/DESIGN.md` → "Proposed: finance screens" → "Left
//! panel"; Paper `19T-0`): net worth, the accounts with their pots, and the
//! categories with this month's counts.
//!
//! No account is selected, which means "All accounts", so the table shows
//! its Account column. Filtering by account or category arrives with the
//! database (M1).

use gpui_kit::{
    InteractiveElement as _, IntoElement, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font, size, spacing, typography};
use crate::ui::button::IconButton;
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::marker::Marker;
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use super::fixture::{self, ACCOUNTS, Category, Sign, gbp};

/// The net worth figure: Geist Mono 13/16 600.
const NET_WORTH: TextRole = TextRole {
    family: font::MONO,
    ..typography::CARD_TITLE
};

/// A list under a section label: 6px inline, 4px top and 10px bottom
/// padding.
fn list_section(label: SectionLabelRow) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(spacing::S1)
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(label)
}

pub(super) fn render() -> impl IntoElement {
    let net_worth = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S0_5)
        .pt(spacing::S3)
        .px(spacing::S3_5)
        .pb(spacing::S2)
        .child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child("Net worth"),
        )
        .child(
            div()
                .text_role(NET_WORTH)
                .text_color(color::TEXT)
                .child(gbp(fixture::net_worth(), Sign::Negative)),
        );

    let accounts = list_section(SectionLabelRow::new("Accounts").action(IconButton::new(
        "add-account",
        Glyph::Plus,
        "Add account",
    )))
    .children(ACCOUNTS.iter().map(|account| {
        ListRow::new(account.id, account.name)
            .icon(if account.pot {
                Glyph::Pot
            } else {
                Glyph::Account
            })
            .nested(account.pot)
            .trailing(gbp(account.balance, Sign::Negative))
            .trailing_tone(if account.balance < 0 {
                TrailingTone::Danger
            } else {
                TrailingTone::Muted
            })
    }));

    let categories = list_section(SectionLabelRow::new("Categories").action(IconButton::new(
        "add-category",
        Glyph::Plus,
        "Add category",
    )))
    .children(Category::ALL.into_iter().map(|category| {
        ListRow::new(category.name(), category.name())
            .marker(category.marker())
            .trailing(category.count().to_string())
            .trailing_tone(TrailingTone::Faint)
    }))
    .child(
        ListRow::new("uncategorised", "Uncategorised")
            .marker(Marker::None)
            .dimmed(true)
            .trailing(fixture::UNCATEGORISED_COUNT.to_string())
            .trailing_tone(TrailingTone::Faint),
    );

    div()
        .id("left-panel")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::PANEL)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_r(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(net_worth)
        .child(accounts)
        .child(categories)
}
