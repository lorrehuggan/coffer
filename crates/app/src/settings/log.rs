//! Settings · access log (ROADMAP M3.15; `docs/DESIGN.md` → "Settings" →
//! **Access log**; Paper `DC6-0`).
//!
//! Every tool call, newest first, in day groups: time · client · tool (a
//! 12px kind lane: empty for a read, `inbox` for a write, `refused` for a
//! refused call) · the purpose in the client's words · rows. Filtered by
//! client and period; earlier calls load as the list is scrolled to its
//! end. The inspector shows the selected call: rows, client and level,
//! kind, the redacted arguments, and the purpose.
//!
//! What it shows is what `coffer ai-access log` shows, and no more
//! (decided with the user): never the result hash; the purpose and
//! argument keys cleaned of invisible characters, the row's purpose cut as
//! the CLI cuts it, the inspector's whole. A refused call is neutral
//! (`text-muted`), not `danger`. The purpose is untrusted: shown, never
//! acted on.
//!
//! Composed for M3.15 (not drawn): a failed call reads "The tool failed" in
//! `text-muted` with "—" rows; a row summarising refused calls the pending
//! file had no room for says how many; the filters are selects drawn as
//! outline compact buttons; an empty period says so; the inspector of a
//! refused call has "—" as its figure.

use coffer_core::access_log::activity::{ArgumentValue, arguments};
use coffer_core::access_log::{LoggedCall, Outcome};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::actions::{SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::cli::ai_access::{PURPOSE_CHARS, one_line};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::icon::Glyph;
use crate::ui::menu::{MenuGroup, MenuOption, Select};
use crate::ui::text::TextRoleExt as _;

use super::clients::level_label;
use super::data::{LogRow, Period};
use super::table::{
    self, caption, cell, column_label, count, group, group_title, mono, plain, rich, row, section,
    selectable_row, toolbar, value_row,
};
use super::{SettingsView, store};

/// The time lane: 48px.
fn time_lane() -> gpui_kit::Pixels {
    spacing::S6 * 2.
}

/// The tool lane: 200px.
fn tool_lane() -> gpui_kit::Pixels {
    size::COLUMN_CATEGORY + spacing::S5
}

/// The client filter's key for every client.
const ALL_CLIENTS: &str = "*";

/// A filter's label: 11/14 500.
const FILTER: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::CAPTION
};

/// Why a refused or failed call returned nothing, as its row and
/// inspector say it.
pub fn refusal(call: &LoggedCall) -> Option<String> {
    let reason = match call.outcome {
        Outcome::Ok => return None,
        Outcome::RefusedOff => "AI access was off".to_owned(),
        Outcome::RefusedNotEnabled => format!("{} isn't enabled", call.client),
        Outcome::RefusedLevel => format!("the tool is above {}'s level", call.client),
        Outcome::RefusedRestoring => "a backup was being restored".to_owned(),
        Outcome::RefusedUnavailable => "the database couldn't be opened".to_owned(),
        Outcome::Error => return Some("The tool failed".to_owned()),
    };
    Some(match call.summarised_calls {
        Some(calls) => format!("Refused {} calls: {reason}", count(calls)),
        None => format!("Refused: {reason}"),
    })
}

/// A day group's title: `Today, Tue 10 Nov`, `Mon 9 Nov`, with the year
/// when it isn't this one.
pub fn day_title(date: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    use chrono::Datelike as _;
    if date == today {
        date.format("Today, %a %-d %b").to_string()
    } else if date.year() == today.year() {
        date.format("%a %-d %b").to_string()
    } else {
        date.format("%a %-d %b %Y").to_string()
    }
}

/// An outline compact button with a chevron: a filter's closed select
/// (Paper `E2S-0`).
fn filter_trigger(id: &'static str, label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .h(size::CONTROL_COMPACT)
        .px(spacing::S2)
        .rounded(radius::MD)
        .border(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .hover(|style| style.bg(color::HOVER))
        .child(
            div()
                .whitespace_nowrap()
                .text_role(FILTER)
                .text_color(color::TEXT_SECONDARY)
                .child(label.into()),
        )
        .child(Glyph::ChevronDown.render().text_color(color::ICON_FAINT))
}

/// Which of the log's filters.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    Client,
    Period,
}

impl SettingsView {
    fn filter_groups(&self, which: Filter) -> Vec<MenuGroup> {
        match which {
            Filter::Client => {
                let mut names: Vec<String> = self.ai.log_clients.clone();
                for client in &self.ai.clients {
                    names.push(client.name.to_string());
                }
                if let Some(chosen) = &self.log_filter.client {
                    names.push(chosen.clone());
                }
                names.sort();
                names.dedup();
                vec![
                    MenuGroup::new(None, vec![MenuOption::new(ALL_CLIENTS, "All clients")]),
                    MenuGroup::new(
                        Some("Clients"),
                        names
                            .into_iter()
                            .map(|name| MenuOption::new(name.clone(), name))
                            .collect(),
                    ),
                ]
            }
            Filter::Period => vec![MenuGroup::new(
                None,
                Period::ALL
                    .into_iter()
                    .map(|period| MenuOption::new(period.key(), period.label()))
                    .collect(),
            )],
        }
    }

    fn filter_chosen(&self, which: Filter) -> SharedString {
        match which {
            Filter::Client => self
                .log_filter
                .client
                .clone()
                .unwrap_or_else(|| ALL_CLIENTS.to_owned())
                .into(),
            Filter::Period => self.log_filter.period.option_key(),
        }
    }

    fn filter_state(&mut self, which: Filter) -> &mut crate::ui::menu::SelectState {
        match which {
            Filter::Client => &mut self.client_filter,
            Filter::Period => &mut self.period_filter,
        }
    }

    fn filter_select(&self, which: Filter, cx: &mut Context<Self>) -> Select {
        let (id, name, label) = match which {
            Filter::Client => (
                "log-client-filter",
                "Client",
                self.log_filter
                    .client
                    .clone()
                    .unwrap_or_else(|| "All clients".to_owned()),
            ),
            Filter::Period => (
                "log-period-filter",
                "Period",
                self.log_filter.period.label().to_owned(),
            ),
        };
        let groups = self.filter_groups(which);
        let moved = groups.clone();
        let chosen = self.filter_chosen(which);
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let state = match which {
            Filter::Client => &self.client_filter,
            Filter::Period => &self.period_filter,
        };
        Select::new(
            id,
            name,
            crate::ui::field::SelectField::new(format!("{id}-field"), label.clone()),
            groups,
            Some(chosen),
            state,
        )
        .trigger(filter_trigger(
            match which {
                Filter::Client => "log-client-trigger",
                Filter::Period => "log-period-trigger",
            },
            label,
        ))
        .menu_width(size::COLUMN_CATEGORY)
        .max_height(size::CONTROL * 10.)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                let chosen = this.filter_chosen(which);
                this.filter_state(which)
                    .set_open(open, Some(&chosen), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.filter_state(which).set_open(false, None, window, cx);
                let mut filter = this.log_filter.clone();
                match which {
                    Filter::Client => {
                        filter.client = (key.as_ref() != ALL_CLIENTS).then(|| key.to_string());
                    }
                    Filter::Period => {
                        if let Some(period) = Period::from_key(key) {
                            filter.period = period;
                        }
                    }
                }
                this.set_log_filter(filter, cx);
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.filter_state(which).move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }

    fn log_row(&self, row: &LogRow, cx: &mut Context<Self>) -> AnyElement {
        let call = &row.call;
        let selected = self.selected_call == Some(call.id);
        let refused = call.outcome != Outcome::Ok;
        let id = call.id;
        let kind = if store::refused(call.outcome) {
            Some(Glyph::Refused)
        } else if row.writes && !refused {
            Some(Glyph::Inbox)
        } else {
            None
        };
        let purpose: AnyElement = match (refusal(call), &call.purpose) {
            (Some(text), _) => div()
                .text_color(color::TEXT_MUTED)
                .child(text)
                .into_any_element(),
            (None, Some(purpose)) => div()
                .text_color(color::TEXT_SECONDARY)
                .child(format!("“{}”", one_line(purpose, PURPOSE_CHARS)))
                .into_any_element(),
            (None, None) => div()
                .text_color(color::TEXT_MUTED)
                .child("No purpose given")
                .into_any_element(),
        };
        selectable_row(format!("call-{id}"), selected)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.list_focus.focus(window, cx);
                this.select_call(id, cx)
            }))
            .child(
                cell(Some(time_lane()))
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_SECONDARY)
                    .child(row.local.format("%H:%M").to_string()),
            )
            .child(
                cell(Some(size::COLUMN_ACCOUNT))
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(if refused {
                        color::TEXT_MUTED
                    } else {
                        color::TEXT
                    })
                    .child(call.client.clone()),
            )
            .child(
                cell(Some(tool_lane()))
                    .gap(spacing::S1_5)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .justify_center()
                            .w(size::ICON_SM)
                            .when_some(kind, |this, glyph| {
                                this.child(glyph.render().text_color(color::TEXT_MUTED))
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_role(typography::MONO_VALUE)
                            .text_color(if refused {
                                color::TEXT_MUTED
                            } else {
                                color::TEXT
                            })
                            .child(call.tool.clone()),
                    ),
            )
            .child(
                cell(None)
                    .text_role(typography::BODY)
                    .child(div().min_w_0().text_ellipsis().child(purpose)),
            )
            .child(
                cell(Some(size::COLUMN_AMOUNT))
                    .justify_end()
                    .text_role(typography::MONO_AMOUNT)
                    .text_color(if refused {
                        color::TEXT_MUTED
                    } else {
                        color::TEXT
                    })
                    .child(if refused {
                        "—".to_owned()
                    } else {
                        count(call.row_count)
                    }),
            )
            .test_support()
            .into_any_element()
    }

    pub(super) fn render_log(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let filters = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(self.filter_select(Filter::Client, cx))
            .child(self.filter_select(Filter::Period, cx));

        // Day groups, newest first, as the rows come.
        let mut days: Vec<(chrono::NaiveDate, Vec<LogRow>)> = Vec::new();
        for row in &self.log {
            match days.last_mut() {
                Some((date, rows)) if *date == row.local.date() => rows.push(row.clone()),
                _ => days.push((row.local.date(), vec![row.clone()])),
            }
        }
        let first_lane = time_lane() + spacing::S3 + size::COLUMN_ACCOUNT;
        let groups: Vec<AnyElement> = days
            .iter()
            .enumerate()
            .map(|(index, (date, rows))| {
                let header = row().child(group_title(
                    day_title(*date, self.clock.today),
                    Some(count(rows.len() as u64)),
                    Some(first_lane),
                ));
                // The lanes are labelled once, in the first group.
                let header = if index == 0 {
                    header
                        .child(column_label("Tool", Some(tool_lane()), false))
                        .child(column_label("Purpose, in the client's words", None, false))
                        .child(column_label("Rows", Some(size::COLUMN_AMOUNT), true))
                } else {
                    header
                };
                let rows: Vec<AnyElement> = rows.iter().map(|row| self.log_row(row, cx)).collect();
                group(index > 0)
                    .child(header)
                    .children(rows)
                    .into_any_element()
            })
            .collect();

        let empty = self.log.is_empty() && self.log_error.is_none();
        let closing = div()
            .pt(spacing::S3)
            .px(spacing::S2)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(caption(if self.log_more {
                "Coffer's server writes one line per call, including refused ones; a client \
                 can't edit or skip it. Purposes are the client's own words, shown as text and \
                 never acted on. Earlier days load as you scroll."
            } else {
                "Coffer's server writes one line per call, including refused ones; a client \
                 can't edit or skip it. Purposes are the client's own words, shown as text and \
                 never acted on."
            }));
        let scroll = self.log_scroll.clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(toolbar(
                "Access log",
                "Every tool call: which client, why it says it asked, and how many rows went back · never the data itself",
                filters,
            ))
            .child(
                table::body("log-body")
                    .key_context(table::LIST_CONTEXT)
                    .track_focus(&self.list_focus.clone().tab_stop(true))
                    .track_scroll(&self.log_scroll)
                    .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.select_next(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.select_next(1, cx)))
                    .on_scroll(cx.listener(move |this, _, _, cx| {
                        // Near the end: the next page.
                        let left = scroll.max_offset().y + scroll.offset().y;
                        if left < size::CONTROL * 4. {
                            this.load_more_log(cx);
                        }
                    }))
                    .when_some(self.log_error.clone(), |this, error| {
                        this.child(
                            div()
                                .pt(spacing::S3)
                                .px(spacing::S2)
                                .text_role(typography::CAPTION)
                                .text_color(color::DANGER)
                                .child(error),
                        )
                    })
                    .when(empty, |this| {
                        this.child(
                            div().pt(spacing::S3).px(spacing::S2).child(caption(
                                match &self.log_filter.client {
                                    Some(client) => format!(
                                        "No calls from {client} in {}.",
                                        self.log_filter.period.label().to_lowercase()
                                    ),
                                    None => format!(
                                        "No calls in {}.",
                                        self.log_filter.period.label().to_lowercase()
                                    ),
                                },
                            )),
                        )
                    })
                    .children(groups)
                    .when(!empty, |this| this.child(closing))
                    .child(div().flex_none().h(px(0.))),
            )
            .into_any_element()
    }

    pub(super) fn render_call_inspector(&mut self, _: &mut Context<Self>) -> Option<AnyElement> {
        let row = self
            .selected_call
            .and_then(|id| self.log.iter().find(|row| row.call.id == id))?
            .clone();
        let call = &row.call;
        let ok = call.outcome == Outcome::Ok;
        let when = if row.local.date() == self.clock.today {
            "today at ".to_owned()
        } else {
            format!("{} at ", row.local.format("%-d %b %Y"))
        };
        let header = table::inspector_header(
            table::mono_title(call.tool.clone()),
            Some(if ok {
                count(call.row_count)
            } else {
                "—".to_owned()
            }),
            rich(
                typography::CAPTION,
                color::TEXT_MUTED,
                vec![
                    plain(if ok {
                        "Rows returned · "
                    } else {
                        "Nothing returned · "
                    }),
                    plain(when),
                    mono(row.local.format("%H:%M:%S").to_string()),
                ],
            )
            .into_any_element(),
        );

        let muted = |text: String| plain(text).color(color::TEXT_MUTED);
        let level = match call.access_level {
            Some(level) => format!(" · {} access", level_label(level).to_lowercase()),
            None => " · no access".to_owned(),
        };
        let kind = match (call.outcome, row.writes) {
            (Outcome::Ok, true) => vec![plain("Wrote"), muted(" · to the Inbox".into())],
            (Outcome::Ok, false) => vec![plain("Read"), muted(" · changes nothing".into())],
            (Outcome::Error, _) => vec![plain("Failed"), muted(" · nothing returned".into())],
            _ => vec![plain("Refused"), muted(" · before the tool ran".into())],
        };
        let mut details = section(None)
            .child(value_row(
                "Client",
                rich(
                    typography::BODY,
                    color::TEXT,
                    vec![mono(call.client.clone()), muted(level)],
                ),
            ))
            .child(value_row("Kind", rich(typography::BODY, color::TEXT, kind)));
        if let Some(refusal) = refusal(call) {
            details = details.child(caption(format!("{refusal}.")));
        } else {
            details = details.child(caption(
                "The rows themselves aren't stored: the log keeps how many went back, never \
                 the data.",
            ));
        }

        let lines = arguments(&call.args_redacted_json);
        let key_width = lines
            .iter()
            .map(|line| line.key.chars().count())
            .max()
            .unwrap_or(0);
        let block = div()
            .flex()
            .flex_col()
            .p(spacing::S2)
            .rounded(radius::MD)
            .bg(color::FIELD)
            .text_role(typography::MONO_VALUE)
            .children(lines.into_iter().map(|line| {
                let key = format!("{:width$} ", line.key, width = key_width);
                let (value, tone) = match line.value {
                    ArgumentValue::Text { length } => (
                        format!(
                            "text, {length} {}",
                            if length == 1 {
                                "character"
                            } else {
                                "characters"
                            }
                        ),
                        color::TEXT_MUTED,
                    ),
                    ArgumentValue::Kept(value) => (value, color::TEXT),
                    ArgumentValue::Undeclared(count) => {
                        (format!("{count} not stored"), color::TEXT_MUTED)
                    }
                };
                div()
                    .flex()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .flex_none()
                            .text_color(color::TEXT_SECONDARY)
                            .child(key),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_color(tone)
                            .child(value),
                    )
            }));
        let args = section(Some("Arguments"))
            .map(|this| {
                if call.args_redacted_json.trim() == "{}" || call.summarised_calls.is_some() {
                    this.child(caption("None."))
                } else {
                    this.child(block)
                }
            })
            .child(caption(
                "Dates, keys and numbers are kept. Free text is stored as its length only, so \
                 the log can't leak what was searched for.",
            ));

        let purpose = section(Some("Purpose"))
            .child(match &call.purpose {
                Some(purpose) => div()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_SECONDARY)
                    .child(format!("“{}”", one_line(purpose, usize::MAX)))
                    .into_any_element(),
                None => div()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_MUTED)
                    .child("No purpose given")
                    .into_any_element(),
            })
            .child(caption(
                "Written by the client and stored as plain text. Coffer never acts on it, so \
                 treat it as the client's claim, not a fact.",
            ));

        Some(
            table::inspector("call-inspector")
                .child(header)
                .child(details)
                .child(args)
                .child(purpose)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn day_titles_read_as_on_paper() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        assert_eq!(day_title(today, today), "Today, Tue 10 Nov");
        assert_eq!(
            day_title(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(), today),
            "Mon 9 Nov"
        );
        assert_eq!(
            day_title(NaiveDate::from_ymd_opt(2025, 11, 9).unwrap(), today),
            "Sun 9 Nov 2025"
        );
    }

    fn call(outcome: Outcome) -> LoggedCall {
        LoggedCall {
            id: 1,
            at: "2026-11-10T10:41:00.000Z".into(),
            client: "my-agent".into(),
            tool: "list_accounts".into(),
            access_level: None,
            outcome,
            purpose: None,
            args_redacted_json: "{}".into(),
            row_count: 0,
            result_hash: None,
            withheld_count: 0,
            summarised_calls: None,
        }
    }

    #[test]
    fn refusals_say_why() {
        assert_eq!(refusal(&call(Outcome::Ok)), None);
        assert_eq!(
            refusal(&call(Outcome::RefusedNotEnabled)).as_deref(),
            Some("Refused: my-agent isn't enabled")
        );
        let summary = LoggedCall {
            summarised_calls: Some(1_200),
            ..call(Outcome::RefusedRestoring)
        };
        assert_eq!(
            refusal(&summary).as_deref(),
            Some("Refused 1,200 calls: a backup was being restored")
        );
    }
}
