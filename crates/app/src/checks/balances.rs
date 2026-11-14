//! Balance checks (`docs/DESIGN.md` → "Review queues"; Paper `98K-0`).
//!
//! - **One table section per account** with balances, failing accounts
//!   first, divided by hairlines: the account name and its lock, then
//!   DATE (72) · BALANCE · COFFER · DIFFERENCE (96 each); rows newest
//!   first, the oldest the starting point rather than a check.
//! - **Under a failed row**, the window: the last balance that matched,
//!   the transactions since, and the possible duplicate that explains the
//!   gap, with "Review pair".
//! - **At an account's foot**: the lock prompt when every check passes
//!   beyond the lock, or, with one balance only, that there's nothing to
//!   check yet and "Add balance…".
//! - **The inspector**: the selected balance ("This check"), what could
//!   explain a failure, and the account's lock with Lock… or Unlock….
//!
//! "Show N transactions" and "Edit balance…" wait for a date-window filter
//! in Transactions and for editing balances (user, 2026-11-14).

use chrono::NaiveDate;
use coffer_core::checks::{AccountReview, CheckRow};
use coffer_core::money::Money;
use coffer_core::reconciliation::Outcome;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Entity, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    Rgba, SharedString, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window,
    div,
};

use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::field::FieldRow;
use crate::ui::icon::Glyph;
use crate::ui::inspector::{header, section, section_title};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::Checks;
use super::duplicates::DATES_LANE;
use super::text::{self, NAME_SHOWN, Part, plain, rich};
use super::view::{click, empty_queue};

/// The preset whose exports carry no balances.
const MONZO: &str = "uk-monzo";

fn lane(width: gpui_kit::Pixels) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(width)
        .overflow_hidden()
        .whitespace_nowrap()
}

fn lane_label(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .text_color(color::TEXT_MUTED)
        .child(Tracked::new(typography::SECTION_LABEL, label))
}

/// The left edge of a row's text after the status lane: the row's
/// padding, the 16px lane and the gap.
fn after_status() -> gpui_kit::Pixels {
    spacing::S2 + size::ICON_LG + spacing::S3
}

/// What Coffer counts on a check's day.
fn counted(row: &CheckRow) -> Option<Money> {
    match row.check.outcome {
        Outcome::Passed { .. } => Some(row.check.stated),
        Outcome::Failed { .. } => row.check.computed(),
        _ => None,
    }
}

/// Coffer less the balance, for a failed check: negative when Coffer is
/// short.
fn difference(row: &CheckRow) -> Option<Money> {
    let minor = i64::try_from(row.check.difference_minor()?).ok()?;
    Some(Money::new(-minor, row.check.stated.currency()))
}

fn group_header(account: &AccountReview, today: NaiveDate, window: &Window) -> impl IntoElement {
    let lock = account.locked_through.map(|through| {
        div()
            .flex()
            .items_center()
            .gap(spacing::S1)
            .child(Glyph::Lock.render().text_color(color::TEXT_MUTED))
            .child(rich(
                window,
                typography::CAPTION,
                color::TEXT_MUTED,
                vec![
                    Part::text("Locked through "),
                    Part::mono(text::date(through, today)),
                ],
            ))
    });
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .child(div().flex_none().w(size::ICON_LG))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(spacing::S2)
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_role(typography::CAPTION)
                        .font_weight(weight::STRONG)
                        .text_color(color::TEXT_SECONDARY)
                        .child(plain(&account.name, NAME_SHOWN)),
                )
                .children(lock),
        )
        .child(lane(DATES_LANE).child(lane_label("Date")))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Balance")),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Coffer")),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(lane_label("Difference")),
        )
}

fn amount(value: String, tone: Rgba, strong: bool) -> impl IntoElement {
    lane(size::COLUMN_AMOUNT)
        .justify_end()
        .text_role(typography::MONO_AMOUNT)
        .text_color(tone)
        .when(strong, |this| this.font_weight(weight::STRONG))
        .child(value)
}

fn check_row(
    account: &AccountReview,
    row: &CheckRow,
    selected: bool,
    handle: &Entity<Checks>,
    focus: &FocusHandle,
    today: NaiveDate,
) -> AnyElement {
    let check = &row.check;
    let (status, source, source_tone): (Option<(Glyph, Rgba)>, String, Rgba) = match check.outcome {
        Outcome::Failed { .. } => (
            Some((Glyph::Alert, color::DANGER)),
            text::source(check.source).to_owned(),
            color::TEXT,
        ),
        Outcome::Passed { .. } => (
            Some((Glyph::Check, color::TEXT_MUTED)),
            text::source(check.source).to_owned(),
            color::TEXT,
        ),
        Outcome::StartingPoint => (
            None,
            format!("{} · starting point", text::source(check.source)),
            color::TEXT_MUTED,
        ),
        Outcome::Valuation => (
            None,
            format!("{} · sets the balance", text::source(check.source)),
            color::TEXT_MUTED,
        ),
    };
    let lifted = |tone: Rgba| {
        if selected && tone == color::TEXT {
            color::WHITE
        } else {
            tone
        }
    };
    let coffer: (String, Rgba) = match counted(row) {
        Some(value) if value.is_negative() => (text::balance(value), color::DANGER),
        Some(value) => (text::balance(value), lifted(color::TEXT)),
        None => ("—".to_owned(), color::TEXT_MUTED),
    };
    let (gap, gap_tone, gap_strong) = match check.outcome {
        Outcome::Failed { .. } => match difference(row) {
            Some(value) => (text::signed(value), color::DANGER, true),
            None => ("—".to_owned(), color::DANGER, true),
        },
        Outcome::Passed { .. } => (
            text::balance(Money::new(0, check.stated.currency())),
            color::TEXT_MUTED,
            false,
        ),
        _ => ("—".to_owned(), color::TEXT_MUTED, false),
    };
    let (account_id, balance) = (account.account, check.balance);
    let (select, focus) = (handle.clone(), focus.clone());
    div()
        .id(SharedString::from(format!(
            "checks-balance-{account_id}-{}",
            balance_key(balance)
        )))
        .test_support()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
        .map(|this| {
            if selected {
                this.bg(color::MOLTEN_SOFT)
            } else {
                this.hover(|style| style.bg(color::HOVER))
            }
        })
        .on_click(click(move |_, window, cx| {
            focus.focus(window, cx);
            select.update(cx, |checks, cx| {
                checks.toggle_check(account_id, balance, cx)
            });
        }))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(size::ICON_LG)
                .children(status.map(|(glyph, tone)| glyph.render().text_color(tone))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::BODY)
                .text_color(lifted(source_tone))
                .child(source),
        )
        .child(
            lane(DATES_LANE)
                .text_role(typography::MONO_VALUE)
                .text_color(if selected {
                    color::TEXT_SECONDARY
                } else {
                    color::TEXT_MUTED
                })
                .child(text::date(check.date, today)),
        )
        .child(amount(
            text::balance(check.stated),
            lifted(color::TEXT),
            false,
        ))
        .child(amount(coffer.0, coffer.1, false))
        .child(amount(gap, gap_tone, gap_strong))
        .into_any_element()
}

/// A stable key for a balance in an element ID.
fn balance_key(balance: coffer_core::reconciliation::BalanceRef) -> String {
    match balance {
        coffer_core::reconciliation::BalanceRef::Opening => "opening".to_owned(),
        coffer_core::reconciliation::BalanceRef::Snapshot(id) => id.to_string(),
    }
}

/// Whether counting started on `since` from the starting point (or a
/// valuation) rather than a balance that passed.
fn from_start(account: &AccountReview, since: NaiveDate) -> bool {
    account
        .rows
        .iter()
        .rev()
        .find(|row| row.check.date == since && !row.check.outcome.is_failed())
        .is_some_and(|row| !row.check.outcome.is_passed())
}

/// The window under a failed row.
fn failing_window(
    checks: &Checks,
    account: &AccountReview,
    row: &CheckRow,
    handle: &Entity<Checks>,
    today: NaiveDate,
    window: &Window,
) -> Option<AnyElement> {
    let since = row.since()?;
    let mut parts = vec![
        Part::text("The gap opened after "),
        Part::mono(text::date(since, today)),
        Part::text(if from_start(account, since) {
            ", where Coffer starts counting: "
        } else {
            ", the last balance that matched: "
        }),
        Part::text(text::count(row.transactions, "transaction", "transactions"))
            .color(color::TEXT_SECONDARY),
        Part::text(" to look at."),
    ];
    let pair = row
        .explained_by
        .and_then(|id| checks.data().review.pair(id));
    if let Some(pair) = pair {
        parts.push(Part::text(" A possible duplicate of ").color(color::TEXT_SECONDARY));
        parts.push(Part::mono(text::signed(pair.rows[1].amount)).color(color::TEXT_SECONDARY));
        parts.push(Part::text(" explains all of it.").color(color::TEXT_SECONDARY));
    }
    let review = pair.map(|pair| {
        let (handle, id) = (handle.clone(), pair.candidate);
        Button::new("checks-review-pair", "Review pair")
            .with_variant(ButtonVariant::Outline)
            .on_click(click(move |_, _, cx| {
                handle.update(cx, |checks, cx| checks.review_pair(id, cx))
            }))
    });
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .pt(spacing::S1_5)
            .pb(spacing::S2_5)
            .pl(after_status())
            .pr(spacing::S2)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_role(typography::CAPTION)
                    .child(rich(window, typography::CAPTION, color::TEXT_MUTED, parts)),
            )
            .children(review)
            .into_any_element(),
    )
}

/// The account's foot: the lock prompt, or "nothing to check yet".
fn account_foot(
    account: &AccountReview,
    handle: &Entity<Checks>,
    today: NaiveDate,
    window: &Window,
) -> Option<AnyElement> {
    let id = account.account;
    let handle = handle.clone();
    let (words, button): (Vec<Part>, Button) = if !account.has_checks() {
        let mut words = vec![Part::text(
            "One balance so far, so there’s nothing to check against yet.",
        )];
        words.push(Part::text(if account.preset.as_deref() == Some(MONZO) {
            " Monzo exports carry no balances: add today’s from the Monzo app."
        } else {
            " Add a later one, or import a file that has balances."
        }));
        (
            words,
            Button::new(
                SharedString::from(format!("checks-add-balance-{id}")),
                "Add balance…",
            )
            .with_variant(ButtonVariant::Outline)
            .on_click(click(move |_, _, cx| {
                handle.update(cx, |checks, cx| checks.request_add_balance(id, cx))
            })),
        )
    } else {
        let through = *account.lockable.first()?;
        (
            vec![
                Part::text("Every check passes through "),
                Part::mono(text::date(through, today)),
                Part::text(
                    ". Lock it so a later import or edit can’t quietly change that history.",
                ),
            ],
            Button::new(
                SharedString::from(format!("checks-lock-{id}")),
                format!("Lock through {}…", text::date(through, today)),
            )
            .with_variant(ButtonVariant::Outline)
            .icon(Glyph::Lock)
            .on_click(click(move |_, _, cx| {
                handle.update(cx, |checks, cx| checks.request_lock(id, cx))
            })),
        )
    };
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .pt(spacing::S1_5)
            .pl(after_status())
            .pr(spacing::S2)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_role(typography::CAPTION)
                    .child(rich(window, typography::CAPTION, color::TEXT_MUTED, words)),
            )
            .child(button)
            .into_any_element(),
    )
}

/// The canvas under the toolbar.
pub fn render_body(
    handle: &Entity<Checks>,
    checks: &Checks,
    focus: &FocusHandle,
    window: &Window,
) -> AnyElement {
    let today = checks.today();
    let review = &checks.data().review;
    if let Some(error) = checks.load_error() {
        return empty_queue(
            Glyph::BalanceLarge,
            "Balance checks couldn’t be read",
            error.clone(),
        )
        .into_any_element();
    }
    let not_checked = text::not_checked(
        &review.not_checked.no_balance,
        &review.not_checked.valued,
        review.not_checked.pots,
    );
    if review.accounts.is_empty() {
        return empty_queue(
            Glyph::BalanceLarge,
            "No balances to check",
            SharedString::from(format!(
                "Add a balance to an account, or import a file that has balances, and Coffer \
                 checks each one against the transactions.{}",
                not_checked
                    .as_ref()
                    .map(|words| format!(" {words}"))
                    .unwrap_or_default()
            )),
        )
        .into_any_element();
    }

    let mut sections: Vec<AnyElement> = Vec::new();
    for (index, account) in review.accounts.iter().enumerate() {
        let mut rows: Vec<AnyElement> = Vec::new();
        for row in account.rows.iter().rev() {
            let selected = checks.is_check_selected(account.account, row.check.balance);
            rows.push(check_row(account, row, selected, handle, focus, today));
            if row.check.outcome.is_failed() {
                rows.extend(failing_window(checks, account, row, handle, today, window));
            }
        }
        sections.push(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pb(spacing::S3)
                .when(index > 0, |this| {
                    this.pt(spacing::S2)
                        .border_t(size::HAIRLINE)
                        .border_color(color::HAIRLINE)
                })
                .child(group_header(account, today, window))
                .children(rows)
                .children(account_foot(account, handle, today, window))
                .into_any_element(),
        );
    }
    div()
        .id("checks-balances-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .pt(spacing::S2)
                .px(spacing::S2)
                .pb(spacing::S4)
                .children(sections)
                .children(not_checked.map(|words| {
                    div()
                        .pt(spacing::S4)
                        .px(spacing::S2)
                        .border_t(size::HAIRLINE)
                        .border_color(color::HAIRLINE)
                        .text_role(typography::CAPTION)
                        .text_color(color::TEXT_MUTED)
                        .child(words)
                })),
        )
        .into_any_element()
}

// The inspector ----------------------------------------------------------------

fn caption(words: impl Into<SharedString>) -> AnyElement {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(words.into())
        .into_any_element()
}

fn rich_caption(window: &Window, parts: Vec<Part>) -> AnyElement {
    div()
        .text_role(typography::CAPTION)
        .child(rich(window, typography::CAPTION, color::TEXT_MUTED, parts))
        .into_any_element()
}

fn value(window: &Window, parts: Vec<Part>) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .px(spacing::S2)
        .text_role(typography::BODY)
        .child(rich(window, typography::BODY, color::TEXT, parts))
        .into_any_element()
}

fn this_check(row: &CheckRow, today: NaiveDate, window: &Window) -> AnyElement {
    let check = &row.check;
    let mut rows: Vec<AnyElement> = vec![
        FieldRow::new(
            "Balance",
            value(
                window,
                vec![
                    Part::mono(text::balance(check.stated)),
                    Part::text(format!(" {}", text::source_after(check.source)))
                        .color(color::TEXT_MUTED),
                ],
            ),
        )
        .into_any_element(),
    ];
    if !check.outcome.is_check() {
        return section(true)
            .gap(spacing::S2)
            .child(section_title("This balance"))
            .children(rows)
            .child(caption(match check.outcome {
                Outcome::Valuation => {
                    "A valuation sets the balance: Coffer counts the balances after it from here."
                }
                _ => {
                    "Coffer starts counting from here: every later balance is checked against \
                     it plus the transactions since."
                }
            }))
            .into_any_element();
    }
    if let Some(counted) = counted(row) {
        rows.push(
            FieldRow::new(
                "Coffer",
                value(
                    window,
                    vec![
                        Part::mono(text::balance(counted)).color(if counted.is_negative() {
                            color::DANGER
                        } else {
                            color::TEXT
                        }),
                        Part::text(" from transactions").color(color::TEXT_MUTED),
                    ],
                ),
            )
            .into_any_element(),
        );
    }
    if let (Some(base), Some(since)) = (row.base, row.since()) {
        rows.push(
            FieldRow::new(
                "Matched",
                value(
                    window,
                    vec![
                        Part::mono(text::balance(base)),
                        Part::text(" on ").color(color::TEXT_MUTED),
                        Part::mono(text::date(since, today)),
                    ],
                ),
            )
            .into_any_element(),
        );
    }
    if let Some((from, to)) = row.days() {
        rows.push(
            FieldRow::new(
                "Window",
                value(
                    window,
                    vec![Part::mono(format!(
                        "{} – {}",
                        text::date(from, today),
                        text::date(to, today)
                    ))],
                ),
            )
            .into_any_element(),
        );
    }
    let mut words = vec![Part::text(
        "Coffer starts from the last balance that matched and adds every transaction since.",
    )];
    match difference(row) {
        Some(gap) => {
            words.push(Part::text(" The "));
            words.push(Part::mono(text::unsigned(gap)));
            words.push(Part::text(format!(
                " is somewhere in those {}.",
                text::count(row.transactions, "transaction", "transactions")
            )));
        }
        None => words.push(Part::text(format!(
            " The {} since add up.",
            text::count(row.transactions, "transaction", "transactions")
        ))),
    }
    section(true)
        .gap(spacing::S2)
        .child(section_title("This check"))
        .children(rows)
        .child(rich_caption(window, words))
        .into_any_element()
}

fn what_could_explain(
    checks: &Checks,
    handle: &Entity<Checks>,
    row: &CheckRow,
    today: NaiveDate,
    window: &Window,
) -> AnyElement {
    let pair = row
        .explained_by
        .and_then(|id| checks.data().review.pair(id));
    let card = pair.map(|pair| {
        let (handle, id) = (handle.clone(), pair.candidate);
        div()
            .flex()
            .flex_col()
            .gap(spacing::S1)
            .p(spacing::S3)
            .rounded(radius::SURFACE)
            .bg(color::FLOAT)
            .border(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S2)
                    .child(Glyph::Duplicate.render().text_color(color::TEXT_STRONG))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_role(typography::BODY)
                            .font_weight(weight::MEDIUM)
                            .text_color(color::TEXT)
                            .child("Possible duplicate"),
                    )
                    .child(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(color::TEXT)
                            .child(text::signed(pair.rows[1].amount)),
                    ),
            )
            .child(div().pl(spacing::S5).child(rich_caption(
                window,
                vec![
                    Part::text(format!("{} on ", text::name(&pair.rows[0]))),
                    Part::mono(text::date(pair.rows[0].date, today)),
                    Part::text(format!(" and {} on ", text::name(&pair.rows[1]))),
                    Part::mono(text::date(pair.rows[1].date, today)),
                    Part::text(". Merging it closes the gap exactly."),
                ],
            )))
            .child(
                div().flex().pl(spacing::S5).pt(spacing::S1).child(
                    Button::new("checks-explain-review", "Review pair")
                        .with_variant(ButtonVariant::Outline)
                        .on_click(click(move |_, _, cx| {
                            handle.update(cx, |checks, cx| checks.review_pair(id, cx))
                        })),
                ),
            )
    });
    let lead = if pair.is_some() {
        "Otherwise: a transaction missing from an import, one dated on the other side of "
    } else {
        "Usually a transaction missing from an import, one dated on the other side of "
    };
    section(true)
        .gap(spacing::S2)
        .child(section_title("What could explain it"))
        .children(card)
        .child(rich_caption(
            window,
            vec![
                Part::text(lead),
                Part::mono(text::date(row.check.date, today)),
                Part::text(", or a balance typed wrong."),
            ],
        ))
        .into_any_element()
}

fn lock_section(
    account: &AccountReview,
    row: Option<&CheckRow>,
    handle: &Entity<Checks>,
    today: NaiveDate,
    window: &Window,
) -> AnyElement {
    let id = account.account;
    let handle = handle.clone();
    let (title, button): (Vec<Part>, Option<Button>) = match account.locked_through {
        Some(through) => (
            vec![
                Part::text("Locked through "),
                Part::mono(text::date(through, today)),
            ],
            Some(
                Button::new("checks-unlock", "Unlock…")
                    .with_variant(ButtonVariant::Outline)
                    .on_click(click(move |_, _, cx| {
                        handle.update(cx, |checks, cx| checks.request_unlock(id, cx))
                    })),
            ),
        ),
        None => (
            vec![Part::text("Not locked")],
            account.lockable.first().map(|through| {
                Button::new(
                    "checks-lock",
                    format!("Lock through {}…", text::date(*through, today)),
                )
                .with_variant(ButtonVariant::Outline)
                .icon(Glyph::Lock)
                .on_click(click(move |_, _, cx| {
                    handle.update(cx, |checks, cx| checks.request_lock(id, cx))
                }))
            }),
        ),
    };
    let mut words: Vec<Part> = match account.locked_through {
        Some(through) => vec![
            Part::text("An import, edit, merge or undo that touches "),
            Part::mono(text::date(through, today)),
            Part::text(" or earlier is flagged and waits for an unlock."),
        ],
        None => vec![Part::text(
            "Locking an account flags any later import, edit, merge or undo that would change \
             its checked history.",
        )],
    };
    let failing = row.filter(|row| row.check.outcome.is_failed());
    match (failing, account.lockable.first()) {
        (Some(row), _) => {
            words.push(Part::text(" Once this check passes you can lock through "));
            words.push(Part::mono(text::date(row.check.date, today)));
            words.push(Part::text("."));
        }
        (None, Some(through)) if account.locked_through.is_some() => {
            words.push(Part::text(" Every check passes through "));
            words.push(Part::mono(text::date(*through, today)));
            words.push(Part::text(", so you can lock through it."));
        }
        (None, None) if account.locked_through.is_none() => {
            words.push(Part::text(
                " Once a balance passes its check you can lock through it.",
            ));
        }
        _ => {}
    }
    section(true)
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL_SM)
                .child(Glyph::Locked.render().text_color(color::TEXT_STRONG))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_role(typography::BODY)
                        .font_weight(weight::STRONG)
                        .child(rich(window, typography::SECTION_TITLE, color::TEXT, title)),
                )
                .children(button),
        )
        .child(rich_caption(window, words))
        .into_any_element()
}

/// The inspector.
pub fn render_inspector(handle: &Entity<Checks>, checks: &Checks, window: &Window) -> AnyElement {
    let today = checks.today();
    let mut sections: Vec<AnyElement> = Vec::new();
    match checks.selected_check() {
        Some((account, row)) => {
            let check = &row.check;
            let name = plain(&account.name, NAME_SHOWN);
            let (figure, words) = match check.outcome {
                Outcome::Failed { .. } => {
                    let gap = difference(row);
                    let short = gap.is_some_and(|gap| gap.is_negative());
                    (
                        gap.map(text::signed),
                        format!(
                            "Check on {} failed · Coffer is {}",
                            text::date(check.date, today),
                            if short { "short" } else { "over" }
                        ),
                    )
                }
                Outcome::Passed { .. } => (
                    Some(text::balance(check.stated)),
                    format!("Check on {} passed", text::date(check.date, today)),
                ),
                Outcome::StartingPoint => (
                    Some(text::balance(check.stated)),
                    format!("Starting point on {}", text::date(check.date, today)),
                ),
                Outcome::Valuation => (
                    Some(text::balance(check.stated)),
                    format!("Valuation on {}", text::date(check.date, today)),
                ),
            };
            sections.push(header(name, figure, Some(caption(words))).into_any_element());
            sections.push(this_check(row, today, window));
            if check.outcome.is_failed() {
                sections.push(what_could_explain(checks, handle, row, today, window));
            }
            sections.push(lock_section(account, Some(row), handle, today, window));
        }
        None => sections.push(
            section(false)
                .child(caption(
                    "Select a balance to see how Coffer counted it, and lock the account once \
                     its checks pass.",
                ))
                .into_any_element(),
        ),
    }
    div()
        .id("checks-inspector")
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .children(sections)
        .into_any_element()
}
